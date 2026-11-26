//! «Sync by audio»: two selected clips (each with its linked group) lined up
//! by the sound they both recorded.

use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::mpsc;

use vv_core::{ClipId, ClipSource, FrameIdx, TimelineId, TrackKind};

use crate::VenturiApp;

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
    reference_start: FrameIdx,
    moved: Vec<(usize, ClipId)>,
    moved_start: FrameIdx,
    fps: f64,
    rx: mpsc::Receiver<Result<(i64, f32), String>>,
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
    /// Starts lining up the two selected groups: the one with video (or on
    /// the upper track) stays, the other moves.
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
            let members: Vec<(usize, ClipId)> = match clip.linked_group {
                Some(group) => tl.clips_in_group(group),
                None => vec![(track, id)],
            };
            if !groups.iter().any(|g| g.contains(&(track, id))) {
                groups.push(members);
            }
        }
        if groups.len() != 2 {
            self.toast = Some((t!("sync.pick_two").into_owned(), std::time::Instant::now()));
            return;
        }
        let has_video = |g: &Vec<(usize, ClipId)>| {
            g.iter()
                .any(|&(t, _)| tl.tracks[t].kind == TrackKind::Video)
        };
        if !has_video(&groups[0]) && has_video(&groups[1])
            || (has_video(&groups[0]) == has_video(&groups[1]) && groups[1][0].0 < groups[0][0].0)
        {
            groups.swap(0, 1);
        }
        let side = |g: &Vec<(usize, ClipId)>| -> Option<(Side, FrameIdx)> {
            let fps = tl.fps.as_f64();
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
        let (Some((reference, reference_start)), Some((moved, moved_start))) =
            (side(&groups[0]), side(&groups[1]))
        else {
            self.toast = Some((t!("sync.no_audio").into_owned(), std::time::Instant::now()));
            return;
        };
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("audio-sync".into())
            .spawn(move || {
                let _ = tx.send(lag_of(&reference, &moved));
            })
            .expect("spawning the audio sync thread");
        self.toast = Some((t!("sync.working").into_owned(), std::time::Instant::now()));
        self.audio_sync.job = Some(Job {
            timeline: timeline_id,
            reference_start,
            moved: groups[1].clone(),
            moved_start,
            fps: tl.fps.as_f64(),
            rx,
        });
    }

    pub(crate) fn poll_audio_sync(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.audio_sync.job else {
            return;
        };
        let result = match job.rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => Err("worker stopped".into()),
        };
        let Some(job) = self.audio_sync.job.take() else {
            return;
        };
        match result {
            Ok((_, confidence)) if confidence < MIN_CONFIDENCE => {
                self.toast = Some((t!("sync.no_match").into_owned(), std::time::Instant::now()));
            }
            Ok((lag, _)) => {
                let lag_secs = lag as f64 / vv_audio::sync::ENVELOPE_RATE as f64;
                let target = job.reference_start + (lag_secs * job.fps).round() as FrameIdx;
                let delta = target - job.moved_start;
                let Some(tl) = self.session.project.timelines.get(job.timeline) else {
                    return;
                };
                let moves: Vec<_> = job
                    .moved
                    .iter()
                    .filter_map(|&(t, id)| {
                        let c = tl.clip(t, id)?;
                        Some((id, t, t, (c.timeline_start + delta).max(0)))
                    })
                    .collect();
                if delta != 0 && !moves.is_empty() {
                    vv_core::edit::move_clips(
                        &mut self.session.project,
                        &mut self.session.history,
                        job.timeline,
                        moves,
                    );
                }
                self.toast = Some((
                    t!("sync.done", frames = delta).into_owned(),
                    std::time::Instant::now(),
                ));
            }
            Err(e) => self.toast = Some((e, std::time::Instant::now())),
        }
    }
}

#[cfg(test)]
#[path = "tests/audio_sync.rs"]
mod tests;
