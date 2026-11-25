//! «Stabilize» section of the Video tab: the clip's motion measured on a
//! worker, then a strength the smoothing is recomputed from.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

use vv_core::stabilize::{Luma, SetStabilization, Stabilization, frame_motion};
use vv_core::{ClipId, ClipSource, FrameIdx, TimelineId};

use crate::properties_panel::{param_row, section_header, slider_field};
use crate::{PanelTarget, VenturiApp};

const DEFAULT_STRENGTH: f32 = 0.6;

type ClipKey = (TimelineId, usize, ClipId);

struct Job {
    clip: ClipKey,
    start: FrameIdx,
    fps: f64,
    done: Arc<AtomicUsize>,
    total: usize,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<Vec<[f32; 3]>, String>>,
}

#[derive(Default)]
pub(crate) struct State {
    job: Option<Job>,
    error: Option<(ClipKey, String)>,
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
}

/// The motion between consecutive frames of `frames` of the file at `path`.
fn analyze(
    path: PathBuf,
    frames: std::ops::Range<FrameIdx>,
    done: &AtomicUsize,
    cancel: &AtomicBool,
) -> Result<Vec<[f32; 3]>, String> {
    let mut decoder = vv_media::Decoder::open(&path).map_err(|e| e.to_string())?;
    let fps = decoder.fps().as_f64().max(1e-9);
    decoder
        .seek_to_time(frames.start as f64 / fps)
        .map_err(|e| e.to_string())?;
    let mut motion = Vec::new();
    let mut previous: Option<Luma> = None;
    while let Some((idx, frame)) = decoder.next_frame().map_err(|e| e.to_string())? {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if idx < frames.start {
            continue;
        }
        if idx >= frames.end {
            break;
        }
        let (w, h) = (frame.width as usize, frame.height as usize);
        let luma = Luma::downscaled(&frame.y, w, h, w);
        if let Some(prev) = &previous {
            motion.push(frame_motion(prev, &luma));
        }
        previous = Some(luma);
        done.fetch_add(1, Ordering::Relaxed);
    }
    Ok(motion)
}

impl VenturiApp {
    pub(crate) fn show_stabilize_section(&mut self, ui: &mut egui::Ui, targets: &[PanelTarget]) {
        self.poll_stabilize_job(ui.ctx());
        let Some(target) = targets.first().copied() else {
            return;
        };
        let key = (target.timeline, target.track_index, target.clip_id);
        let tl = &self.session.project.timelines[target.timeline];
        let Some(clip) = tl.clip(target.track_index, target.clip_id) else {
            return;
        };
        let ClipSource::Media(media_id) = clip.source else {
            return;
        };
        let Some(item) = self.session.project.media_pool.get(media_id) else {
            return;
        };
        if item.compound.is_some() || item.meta.is_image() || !item.meta.has_video {
            return;
        }
        let current = clip.effects.stabilize.clone();
        let (start, end) = (clip.source_in(), clip.source_out());
        let path = item.path.clone();
        let fps = item.meta.fps.as_f64();

        ui.add_space(6.0);
        let (open, _) = section_header(ui, &t!("props.stabilize"), false);
        if !open {
            return;
        }
        let running = self.stabilize.job.as_ref().filter(|j| j.clip == key);
        let mut enabled = current.is_some() || running.is_some();
        let toggled = ui
            .checkbox(&mut enabled, t!("props.stabilize_on"))
            .changed();
        if let Some(job) = running {
            let share = job.done.load(Ordering::Relaxed) as f32 / job.total.max(1) as f32;
            ui.add(egui::ProgressBar::new(share).text(t!("props.stabilize_analyzing")));
        }
        if let Some((error_key, error)) = &self.stabilize.error
            && *error_key == key
        {
            ui.colored_label(crate::theme::ERROR, error);
        }
        if toggled {
            if enabled {
                self.start_stabilize_job(key, path, start..end, fps);
            } else {
                if let Some(job) = self.stabilize.job.take() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
                self.session
                    .apply(Box::new(SetStabilization::new(key.0, key.1, key.2, None)));
            }
            return;
        }
        let Some(stab) = current else {
            return;
        };
        let strength_key = egui::Id::new(("stabilize_strength", key));
        let mut strength: f32 = ui
            .data(|d| d.get_temp(strength_key))
            .unwrap_or(stab.strength * 100.0);
        let row = param_row(ui, &t!("props.strength"), None, |ui| {
            slider_field(ui, &mut strength, 0.0..=100.0, 0.5, 0)
        });
        let dragging = ui.ctx().input(|i| i.pointer.any_down());
        if row.changed {
            ui.data_mut(|d| d.insert_temp(strength_key, strength));
        }
        let reset = row.reset;
        if reset {
            strength = DEFAULT_STRENGTH * 100.0;
        }
        if (row.changed && !dragging)
            || reset
            || (!dragging && ui.data(|d| d.get_temp::<f32>(strength_key)).is_some())
        {
            ui.data_mut(|d| d.remove::<f32>(strength_key));
            let value = Stabilization::new(stab.start, stab.motion.clone(), strength / 100.0, fps);
            if value != stab {
                self.session.apply(Box::new(SetStabilization::new(
                    key.0,
                    key.1,
                    key.2,
                    Some(value),
                )));
            }
        }
        if (start, end) != (stab.start, stab.start + stab.motion.len() as FrameIdx + 1) {
            ui.small(t!("props.stabilize_trimmed"));
            if ui.small_button(t!("props.stabilize_again")).clicked() {
                self.start_stabilize_job(key, path, start..end, fps);
            }
        }
    }

    fn start_stabilize_job(
        &mut self,
        clip: ClipKey,
        path: PathBuf,
        frames: std::ops::Range<FrameIdx>,
        fps: f64,
    ) {
        if let Some(job) = self.stabilize.job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.stabilize.error = None;
        let done = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let (job_done, job_cancel) = (done.clone(), cancel.clone());
        let range = frames.clone();
        std::thread::Builder::new()
            .name("stabilize".into())
            .spawn(move || {
                let _ = tx.send(analyze(path, range, &job_done, &job_cancel));
            })
            .expect("spawning the stabilize thread");
        self.stabilize.job = Some(Job {
            clip,
            start: frames.start,
            fps,
            done,
            total: (frames.end - frames.start).max(1) as usize,
            cancel,
            rx,
        });
    }

    pub(crate) fn poll_stabilize_job(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.stabilize.job else {
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
        let Some(job) = self.stabilize.job.take() else {
            return;
        };
        match result {
            Ok(motion) => {
                let (timeline, track, clip) = job.clip;
                let still_there = self
                    .session
                    .project
                    .timelines
                    .get(timeline)
                    .and_then(|tl| tl.clip(track, clip))
                    .is_some();
                if still_there {
                    let value = Stabilization::new(job.start, motion, DEFAULT_STRENGTH, job.fps);
                    self.session.apply(Box::new(SetStabilization::new(
                        timeline,
                        track,
                        clip,
                        Some(value),
                    )));
                }
            }
            Err(e) => self.stabilize.error = Some((job.clip, e)),
        }
    }
}

#[cfg(test)]
#[path = "tests/stabilize_ui.rs"]
mod tests;
