//! «Stabilize» section of the Video tab: the camera motion measured on a
//! worker, its correction applied as one history step that a new setting
//! replaces while it is still the last step.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use vv_core::auto_zoom::{Origin, SetAutoZoom};
use vv_core::stabilize::{self, Blocker, Config, Motion};
use vv_core::{ClipId, ClipSource, CommandLabel, FrameIdx, TimelineId};

use crate::properties_panel::section_header;
use crate::reframe::percent_row;
use crate::{PanelTarget, VenturiApp, theme};

/// Content hash and media frames measured.
type Key = (u64, FrameIdx, FrameIdx);
type ClipKey = (TimelineId, usize, ClipId);

#[derive(Default)]
pub(crate) struct State {
    job: Option<Job>,
    motions: HashMap<Key, Arc<Motion>>,
    looked_up: HashSet<Key>,
    error: Option<(Key, String)>,
    config: Config,
    applied: Option<Applied>,
    dirty: bool,
}

struct Job {
    key: Key,
    /// Stabilized as soon as the measure is done.
    clip: ClipKey,
    media_size: (u32, u32),
    fps: f64,
    progress: Arc<vv_reframe::Progress>,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<Motion, String>>,
}

struct Applied {
    clip: ClipKey,
    generation: u64,
    zoom: f32,
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
}

fn cache_dir() -> Option<PathBuf> {
    Some(crate::library::Library::default_root()?.join("stabilize"))
}

fn cache_path(key: Key) -> Option<PathBuf> {
    cache_dir().map(|dir| vv_reframe::stabilize::motion_cache_path(&dir, key.0, (key.1, key.2)))
}

struct Subject {
    clip: vv_core::Clip,
    path: PathBuf,
    key: Key,
    media_size: (u32, u32),
    fps: f64,
}

impl VenturiApp {
    fn stabilize_subject(
        &self,
        target: &PanelTarget,
    ) -> Result<Subject, std::borrow::Cow<'static, str>> {
        let tl = &self.session.project.timelines[target.timeline];
        let clip = tl
            .clip(target.track_index, target.clip_id)
            .cloned()
            .ok_or_else(|| t!("stabilize.select_video"))?;
        let ClipSource::Media(media) = clip.source else {
            return Err(t!("stabilize.select_video"));
        };
        let item = self
            .session
            .project
            .media_pool
            .get(media)
            .ok_or_else(|| t!("stabilize.select_video"))?;
        if target.is_text || !item.meta.has_video || item.meta.is_image() || item.compound.is_some()
        {
            return Err(t!("stabilize.select_video"));
        }
        if clip.freeze.is_some() || clip.disabled {
            return Err(t!("stabilize.unsupported_clip"));
        }
        match stabilize::blocker(&clip) {
            Some(Blocker::Keyframes) => return Err(t!("stabilize.has_keyframes")),
            Some(Blocker::Transform) => return Err(t!("stabilize.unsupported_transform")),
            None => {}
        }
        if !item.path.is_file() {
            return Err(t!("stabilize.offline"));
        }
        Ok(Subject {
            key: (item.content_hash, clip.source_in(), clip.source_out()),
            path: item.path.clone(),
            media_size: (item.meta.width, item.meta.height),
            fps: item.meta.fps.as_f64(),
            clip,
        })
    }

    pub(crate) fn show_stabilize_section(&mut self, ui: &mut egui::Ui, targets: &[PanelTarget]) {
        if self
            .stabilize
            .applied
            .as_ref()
            .is_some_and(|a| a.generation != self.session.history.generation())
        {
            self.stabilize.applied = None;
        }
        self.poll_stabilize_job(ui.ctx());
        ui.add_space(crate::theme::SPACE_2);
        let (open, _) = section_header(ui, &t!("props.stabilize"), false);
        if !open {
            return;
        }
        let [target] = targets else {
            ui.weak(t!("stabilize.single_clip"));
            return;
        };
        let subject = match self.stabilize_subject(target) {
            Ok(s) => s,
            Err(why) => {
                ui.weak(why);
                return;
            }
        };
        let clip_key = (target.timeline, target.track_index, target.clip_id);
        let key = subject.key;

        let mut config = self.stabilize.config.clone();
        let mut changed = percent_row(
            ui,
            &t!("stabilize.strength"),
            &mut config.strength,
            stabilize::STRENGTH,
        );
        changed |= percent_row(
            ui,
            &t!("stabilize.max_zoom"),
            &mut config.max_zoom,
            stabilize::MAX_ZOOM,
        );
        ui.weak(t!("stabilize.max_zoom_hint"));
        self.stabilize.config = config;
        self.stabilize.dirty |= changed;

        if !self.stabilize.motions.contains_key(&key)
            && self.stabilize.looked_up.insert(key)
            && let Some(motion) =
                cache_path(key).and_then(|p| vv_reframe::stabilize::load_cached_motion(&p))
        {
            self.stabilize.motions.insert(key, Arc::new(motion));
        }

        ui.add_space(theme::SPACE_2);
        if let Some(job) = self.stabilize.job.as_ref().filter(|j| j.key == key) {
            ui.label(t!("stabilize.analysing"));
            ui.horizontal(|ui| {
                ui.add(
                    egui::ProgressBar::new(job.progress.fraction())
                        .show_percentage()
                        .desired_width(ui.available_width() - 80.0),
                );
                if ui.button(t!("stabilize.cancel")).clicked() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
            });
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }
        if let Some((_, e)) = self.stabilize.error.as_ref().filter(|(k, _)| *k == key) {
            if e == "cancelled" {
                ui.colored_label(theme::TEXT_SECONDARY, t!("stabilize.cancelled"));
            } else {
                ui.colored_label(theme::ERROR, t!("stabilize.error", detail = e));
            }
        }

        let generated = subject
            .clip
            .auto_keyframes
            .iter()
            .any(|k| k.origin == Origin::Stabilize);
        let motion = self.stabilize.motions.get(&key).cloned();
        if let Some(motion) = &motion
            && generated
            && self.stabilize.dirty
            && !ui.input(|i| i.pointer.any_down())
        {
            self.stabilize.dirty = false;
            self.apply_stabilize(clip_key, subject.media_size, subject.fps, motion);
        }
        if !generated {
            self.stabilize.dirty = false;
        }

        if let Some(applied) = self
            .stabilize
            .applied
            .as_ref()
            .filter(|a| a.clip == clip_key)
        {
            ui.weak(t!(
                "stabilize.applied",
                zoom = format!("{:.0}", applied.zoom * 100.0)
            ));
        }
        ui.horizontal(|ui| {
            if !generated {
                let run = ui.add(crate::ui_kit::primary_button(t!("stabilize.run")));
                if run.clicked() {
                    match &motion {
                        Some(motion) => {
                            self.apply_stabilize(clip_key, subject.media_size, subject.fps, motion)
                        }
                        None => self.start_stabilize_job(&subject, clip_key),
                    }
                }
            } else if ui.button(t!("stabilize.remove")).clicked() {
                self.stabilize.applied = None;
                self.session.history.do_command(
                    &mut self.session.project,
                    Box::new(SetAutoZoom::remove(
                        target.timeline,
                        target.track_index,
                        &subject.clip,
                        Origin::Stabilize,
                    )),
                );
            }
        });
        if motion.is_none() {
            ui.weak(t!("stabilize.local_note"));
        }
    }

    fn start_stabilize_job(&mut self, subject: &Subject, clip: ClipKey) {
        if let Some(job) = self.stabilize.job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.stabilize.error = None;
        let progress = Arc::new(vv_reframe::Progress::default());
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let key = subject.key;
        let (path, size, fps) = (subject.path.clone(), subject.media_size, subject.fps);
        let hw = crate::hw_decode::devices();
        let (job_progress, job_cancel) = (progress.clone(), cancel.clone());
        std::thread::Builder::new()
            .name("stabilize".into())
            .spawn(move || {
                let cache = cache_path(key);
                let result = vv_reframe::stabilize::analyze_motion(
                    &path,
                    size,
                    (key.1, key.2),
                    fps,
                    &hw,
                    cache.as_deref(),
                    &job_progress,
                    &job_cancel,
                )
                .map_err(|e| e.to_string());
                let _ = tx.send(result);
            })
            .expect("spawning the stabilize thread");
        self.stabilize.job = Some(Job {
            key,
            clip,
            media_size: subject.media_size,
            fps: subject.fps,
            progress,
            cancel,
            rx,
        });
    }

    fn poll_stabilize_job(&mut self, ctx: &egui::Context) {
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
                let motion = Arc::new(motion);
                self.stabilize.motions.insert(job.key, motion.clone());
                self.apply_stabilize(job.clip, job.media_size, job.fps, &motion);
            }
            Err(e) => self.stabilize.error = Some((job.key, e)),
        }
    }

    /// One history step; the previous one is replaced when it is still the last.
    fn apply_stabilize(
        &mut self,
        clip_key: ClipKey,
        media_size: (u32, u32),
        fps: f64,
        motion: &Motion,
    ) {
        if self.stabilize.applied.as_ref().is_some_and(|a| {
            a.clip == clip_key && a.generation == self.session.history.generation()
        }) {
            self.session.history.undo(&mut self.session.project);
        }
        self.stabilize.applied = None;
        let (timeline, track_index, clip_id) = clip_key;
        let Some(tl) = self.session.project.timelines.get(timeline) else {
            return;
        };
        let Some(clip) = tl.clip(track_index, clip_id).cloned() else {
            return;
        };
        if stabilize::blocker(&clip).is_some()
            || (clip.source_in(), clip.source_out()) != motion.range
        {
            return;
        }
        let plan = stabilize::plan(motion, &self.stabilize.config, fps);
        let zoom = plan.shots.iter().map(|s| s.zoom).fold(1.0, f32::max);
        let (transform, auto) = stabilize::keyframes(&clip, &plan, media_size, tl.resolution);
        self.session.history.do_command(
            &mut self.session.project,
            Box::new(
                SetAutoZoom::new(timeline, track_index, clip_id, transform, auto)
                    .with_label(CommandLabel::Stabilize),
            ),
        );
        self.stabilize.applied = Some(Applied {
            clip: clip_key,
            generation: self.session.history.generation(),
            zoom,
        });
    }
}

#[cfg(test)]
#[path = "tests/stabilize.rs"]
mod tests;
