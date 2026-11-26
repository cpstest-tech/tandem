//! «Sync by audio»: two selected clips (each with its linked group) lined up
//! by the sound they both recorded.

use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::mpsc;

use vv_core::{ClipId, ClipSource, FrameIdx, TimelineId, TrackKind};

use crate::{VenturiApp, timeline_ui};

/// Decoding rate: the envelope needs nothing finer.
const RATE: u32 = 8000;
/// Below this the match is not trusted.
const MIN_CONFIDENCE: f32 = 0.25;

struct Side {
    path: PathBuf,
    stream: usize,
    from_secs: f64,
    to_secs: f64,
}

pub(crate) struct Job {
    timeline: TimelineId,
    reference: Vec<(usize, ClipId)>,
    reference_start: FrameIdx,
    /// Each other group with the start of the clip its audio was read from.
    moved: Vec<(Vec<(usize, ClipId)>, FrameIdx)>,
    fps: f64,
    rx: mpsc::Receiver<Vec<Result<(i64, f32), String>>>,
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) job: Option<Job>,
}

fn decode(side: &Side) -> Result<Vec<f32>, String> {
    let from = (side.from_secs * RATE as f64).max(0.0) as u64;
    let to = (side.to_secs * RATE as f64) as u64;
    let mut mono = Vec::new();
    vv_media::audio::decode_audio_streams_from(
        &side.path,
        &[side.stream],
        RATE,
        from,
        |_, channels, at, chunk| {
            let channels = channels.max(1) as usize;
            for (i, frame) in chunk.chunks(channels).enumerate() {
                let t = at + i as u64;
                if t >= from && t < to {
                    mono.push(frame.iter().sum::<f32>() / channels as f32);
                }
            }
            if at + (chunk.len() / channels) as u64 >= to {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        },
    )
    .map_err(|e| e.to_string())?;
    Ok(mono)
}

fn lag_of(reference: &Side, moved: &Side) -> Result<(i64, f32), String> {
    let a = vv_audio::sync::envelope(&decode(reference)?, 1, RATE);
    let b = vv_audio::sync::envelope(&decode(moved)?, 1, RATE);
    vv_audio::sync::best_lag(&a, &b).ok_or_else(|| "too short".to_string())
}

impl VenturiApp {
    /// Starts lining up the selected groups by their sound: the one with
    /// video on the upper track stays, the others move, each onto tracks of
    /// its own (a multicam stack).
    pub(crate) fn sync_selected_by_audio(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let tl = &self.session.project.timelines[timeline_id];
        let mut groups: Vec<Vec<(usize, ClipId)>> = Vec::new();
        for &(track, id) in &self.timeline_state.selected {
            let Some(clip) = tl.clip(track, id) else {
                continue;
            };
            if groups.iter().any(|g| g.contains(&(track, id))) {
                continue;
            }
            groups.push(match clip.linked_group {
                Some(group) => tl.clips_in_group(group),
                None => vec![(track, id)],
            });
        }
        if groups.len() < 2 {
            self.toast = Some((t!("sync.pick_two").into_owned(), std::time::Instant::now()));
            return;
        }
        let has_video = |g: &Vec<(usize, ClipId)>| {
            g.iter()
                .any(|&(t, _)| tl.tracks[t].kind == TrackKind::Video)
        };
        // Video first, then the upper tracks.
        groups.sort_by_key(|g| (!has_video(g), g.iter().map(|c| c.0).min()));
        let fps = tl.fps.as_f64();
        let side = |g: &Vec<(usize, ClipId)>| -> Option<(Side, FrameIdx)> {
            // The audio clip of the group if any, else the video one's file.
            let &(track, id) = g
                .iter()
                .find(|&&(t, _)| tl.tracks[t].kind == TrackKind::Audio)
                .or_else(|| g.first())?;
            let clip = tl.clip(track, id)?;
            let ClipSource::Media(media) = clip.source else {
                return None;
            };
            let item = self.session.project.media_pool.get(media)?;
            if !item.meta.has_audio || item.compound.is_some() {
                return None;
            }
            Some((
                Side {
                    path: item.path.clone(),
                    stream: clip.audio_stream_index,
                    from_secs: clip.media_secs_at(clip.timeline_start, fps),
                    to_secs: clip.media_secs_at(clip.timeline_end(), fps),
                },
                clip.timeline_start,
            ))
        };
        let sides: Option<Vec<(Side, FrameIdx)>> = groups.iter().map(side).collect();
        let Some(mut sides) = sides else {
            self.toast = Some((t!("sync.no_audio").into_owned(), std::time::Instant::now()));
            return;
        };
        let (reference, reference_start) = sides.remove(0);
        let moved_starts: Vec<FrameIdx> = sides.iter().map(|s| s.1).collect();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("audio-sync".into())
            .spawn(move || {
                let lags = sides.iter().map(|(s, _)| lag_of(&reference, s)).collect();
                let _ = tx.send(lags);
            })
            .expect("spawning the audio sync thread");
        self.toast = Some((t!("sync.working").into_owned(), std::time::Instant::now()));
        let reference_group = groups.remove(0);
        self.audio_sync.job = Some(Job {
            timeline: timeline_id,
            reference: reference_group,
            reference_start,
            moved: groups.into_iter().zip(moved_starts).collect(),
            fps,
            rx,
        });
    }

    pub(crate) fn poll_audio_sync(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.audio_sync.job else {
            return;
        };
        let results = match job.rx.try_recv() {
            Ok(results) => results,
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => Vec::new(),
        };
        let Some(job) = self.audio_sync.job.take() else {
            return;
        };
        if self.session.project.timelines.get(job.timeline).is_none() {
            return;
        }
        let group = self.session.history.begin_group();
        let mut taken: Vec<usize> = job.reference.iter().map(|c| c.0).collect();
        let (mut synced, mut failed) = (0, 0);
        for ((members, moved_start), result) in job.moved.iter().zip(results) {
            let lag = match result {
                Ok((lag, confidence)) if confidence >= MIN_CONFIDENCE => lag,
                _ => {
                    failed += 1;
                    continue;
                }
            };
            let lag_secs = lag as f64 / vv_audio::sync::ENVELOPE_RATE as f64;
            let target = job.reference_start + (lag_secs * job.fps).round() as FrameIdx;
            let delta = target - moved_start;
            let mut moves = Vec::new();
            for &(track, id) in members {
                let tl = &self.session.project.timelines[job.timeline];
                let Some(start) = tl.clip(track, id).map(|c| c.timeline_start) else {
                    continue;
                };
                // Another angle already sits there: this one gets its own track.
                let to = if taken.contains(&track) {
                    let kind = tl.tracks[track].kind;
                    timeline_ui::add_track(
                        &mut self.session.project,
                        &mut self.session.history,
                        job.timeline,
                        kind,
                    )
                } else {
                    track
                };
                taken.push(to);
                moves.push((id, track, to, (start + delta).max(0)));
            }
            if !moves.is_empty() {
                vv_core::edit::move_clips(
                    &mut self.session.project,
                    &mut self.session.history,
                    job.timeline,
                    moves,
                );
            }
            synced += 1;
        }
        self.session
            .history
            .end_group_as(group, vv_core::CommandLabel::MoveClips);
        let message = if failed > 0 && synced == 0 {
            t!("sync.no_match").into_owned()
        } else {
            t!("sync.done_count", count = synced, failed = failed).into_owned()
        };
        self.toast = Some((message, std::time::Instant::now()));
    }
}

#[cfg(test)]
#[path = "tests/audio_sync.rs"]
mod tests;
