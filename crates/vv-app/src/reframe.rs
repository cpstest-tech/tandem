//! «Reframe» sub-tab of the Video properties: face analysis of the clip on
//! a worker, a framing previewed as a real history step (same pattern as
//! Zoom auto), and the faces drawn over the player to pick the subject.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use vv_core::auto_zoom::{Origin, SetAutoZoom};
use vv_core::reframe::{self, Analysis, Pin, Placement};
use vv_core::{ClipId, ClipSource, CommandLabel, FrameIdx, TimelineId};

use crate::properties_panel::{param_row, section_header, slider_field};
use crate::{PanelTarget, VenturiApp, theme};

/// Content hash, media frames analysed, detection step.
type Key = (u64, FrameIdx, FrameIdx, u32);
type ClipKey = (TimelineId, usize, ClipId);

/// Below this the speech cache counts a moment as silence.
const SPEECH_DB: f32 = -42.0;

#[derive(Default)]
pub(crate) struct State {
    job: Option<Job>,
    analyses: HashMap<Key, Arc<Analysis>>,
    looked_up: HashSet<Key>,
    error: Option<(Key, String)>,
    preview: Option<Preview>,
    pins: HashMap<ClipKey, Vec<Pin>>,
    dirty: bool,
    /// A face was picked with no preview running: start one.
    pending_preview: bool,
    /// Pass and clip of the last time the tab was drawn: the player shows
    /// the faces only meanwhile.
    shown: Option<(u64, ClipKey, Key)>,
}

struct Job {
    key: Key,
    progress: Arc<vv_reframe::Progress>,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<Analysis, String>>,
}

struct Preview {
    clip: ClipKey,
    generation: u64,
    missing: Vec<Range<FrameIdx>>,
    scenes: usize,
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
}

fn cache_dir() -> Option<PathBuf> {
    Some(crate::library::Library::default_root()?.join("reframe"))
}

/// What the tab needs of the selected clip, or why it cannot reframe it.
struct Subject {
    clip: vv_core::Clip,
    path: PathBuf,
    key: Key,
    media_size: (u32, u32),
    fps: f64,
}

impl VenturiApp {
    fn reframe_subject(
        &self,
        target: &PanelTarget,
    ) -> Result<Subject, std::borrow::Cow<'static, str>> {
        let tl = &self.session.project.timelines[target.timeline];
        let clip = tl
            .clip(target.track_index, target.clip_id)
            .cloned()
            .ok_or_else(|| t!("reframe.select_video"))?;
        let ClipSource::Media(media) = clip.source else {
            return Err(t!("reframe.select_video"));
        };
        let item = self
            .session
            .project
            .media_pool
            .get(media)
            .ok_or_else(|| t!("reframe.select_video"))?;
        if target.is_text || !item.meta.has_video || item.meta.is_image() || item.compound.is_some()
        {
            return Err(t!("reframe.select_video"));
        }
        if clip.freeze.is_some() || clip.disabled {
            return Err(t!("reframe.unsupported_clip"));
        }
        if reframe::unsupported_transform(&clip) {
            return Err(t!("reframe.unsupported_transform"));
        }
        if !item.path.is_file() {
            return Err(t!("reframe.offline"));
        }
        let step = self
            .settings
            .shorts
            .reframe
            .step
            .clamp(*reframe::STEP.start(), *reframe::STEP.end());
        Ok(Subject {
            key: (item.content_hash, clip.source_in(), clip.source_out(), step),
            path: item.path.clone(),
            media_size: (item.meta.width, item.meta.height),
            fps: item.meta.fps.as_f64(),
            clip,
        })
    }

    pub(crate) fn show_reframe_tab(&mut self, ui: &mut egui::Ui, target: &PanelTarget) {
        if self
            .reframe
            .preview
            .as_ref()
            .is_some_and(|p| p.generation != self.session.history.generation())
        {
            // Something else was edited: the previewed step stays as applied.
            self.reframe.preview = None;
        }
        ui.add_space(6.0);
        let (open, _) = section_header(ui, &t!("reframe.title"), false);
        if !open {
            return;
        }
        ui.weak(t!("reframe.hint"));
        let subject = match self.reframe_subject(target) {
            Ok(s) => s,
            Err(why) => {
                ui.weak(why);
                return;
            }
        };
        let clip_key = (target.timeline, target.track_index, target.clip_id);
        self.reframe.shown = Some((ui.ctx().cumulative_pass_nr(), clip_key, subject.key));
        self.poll_reframe_job(ui.ctx());
        self.show_reframe_settings(ui);

        let key = subject.key;
        if !self.reframe.analyses.contains_key(&key) && self.reframe.looked_up.insert(key) {
            let cached = cache_dir()
                .map(|dir| vv_reframe::cache_path(&dir, key.0, (key.1, key.2), key.3))
                .and_then(|path| vv_reframe::load_cached(&path));
            if let Some(analysis) = cached {
                self.reframe.analyses.insert(key, Arc::new(analysis));
            }
        }
        let Some(analysis) = self.reframe.analyses.get(&key).cloned() else {
            ui.add_space(theme::SPACE_2);
            if let Some(job) = self.reframe.job.as_ref().filter(|j| j.key == key) {
                ui.label(t!("reframe.analysing"));
                ui.horizontal(|ui| {
                    ui.add(
                        egui::ProgressBar::new(job.progress.fraction())
                            .show_percentage()
                            .desired_width(ui.available_width() - 80.0),
                    );
                    if ui.button(t!("reframe.cancel")).clicked() {
                        job.cancel.store(true, Ordering::Relaxed);
                    }
                });
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            if let Some((_, e)) = self.reframe.error.as_ref().filter(|(k, _)| *k == key) {
                ui.colored_label(
                    if e == "cancelled" {
                        theme::TEXT_SECONDARY
                    } else {
                        theme::ERROR
                    },
                    if e == "cancelled" {
                        t!("reframe.cancelled")
                    } else {
                        t!("reframe.error", detail = e)
                    },
                );
            }
            ui.weak(t!("reframe.local_note"));
            if ui.button(t!("reframe.analyse")).clicked() {
                self.start_reframe_job(&subject);
            }
            return;
        };

        let tl = &self.session.project.timelines[target.timeline];
        let resolution = tl.resolution;
        if resolution.0 >= resolution.1 && subject.media_size.0 >= subject.media_size.1 {
            ui.weak(t!("reframe.not_vertical"));
        }
        let faces = analysis
            .samples
            .iter()
            .filter(|s| !s.faces.is_empty())
            .count();
        ui.weak(t!(
            "reframe.analysed",
            shots = analysis.scenes().len(),
            found = faces,
            samples = analysis.samples.len()
        ));
        self.show_face_choice(ui, &subject, &analysis, clip_key);

        let previewing = self
            .reframe
            .preview
            .as_ref()
            .filter(|p| p.clip == clip_key)
            .map(|p| (p.scenes, p.missing.clone()));
        let pointer_down = ui.input(|i| i.pointer.any_down());
        ui.add_space(theme::SPACE_2);
        match previewing {
            Some((scenes, missing)) => {
                ui.label(t!("reframe.preview_count", count = scenes));
                self.show_missing_faces(ui, &missing, subject.fps);
                if self.reframe.dirty && !pointer_down {
                    self.reframe.dirty = false;
                    self.preview_reframe(&subject, clip_key, &analysis);
                }
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(t!("reframe.apply")).color(theme::ON_ACCENT),
                            )
                            .fill(theme::ACCENT_FILL),
                        )
                        .clicked()
                    {
                        self.reframe.preview = None;
                    }
                    if ui.button(t!("reframe.discard")).clicked() {
                        self.reframe.preview = None;
                        self.session.history.undo(&mut self.session.project);
                    }
                });
            }
            None => {
                self.reframe.dirty = false;
                if std::mem::take(&mut self.reframe.pending_preview) {
                    self.preview_reframe(&subject, clip_key, &analysis);
                    return;
                }
                if reframe::has_other_keyframes(&subject.clip) {
                    ui.colored_label(theme::WARNING, t!("reframe.replaces_keys"));
                }
                ui.horizontal(|ui| {
                    if ui.button(t!("reframe.preview")).clicked() {
                        self.preview_reframe(&subject, clip_key, &analysis);
                    }
                    if ui
                        .button(t!("reframe.reanalyse"))
                        .on_hover_text(t!("reframe.reanalyse_hint"))
                        .clicked()
                    {
                        self.reframe.analyses.remove(&key);
                        if let Some(dir) = cache_dir() {
                            let _ = std::fs::remove_file(vv_reframe::cache_path(
                                &dir,
                                key.0,
                                (key.1, key.2),
                                key.3,
                            ));
                        }
                        self.start_reframe_job(&subject);
                    }
                });
                let generated = subject
                    .clip
                    .auto_keyframes
                    .iter()
                    .any(|k| k.origin == Origin::Reframe);
                if ui
                    .add_enabled(generated, egui::Button::new(t!("reframe.remove")))
                    .on_hover_text(t!("reframe.remove_hint"))
                    .clicked()
                {
                    self.session.history.do_command(
                        &mut self.session.project,
                        Box::new(SetAutoZoom::remove(
                            target.timeline,
                            target.track_index,
                            &subject.clip,
                            Origin::Reframe,
                        )),
                    );
                }
            }
        }
    }

    fn show_reframe_settings(&mut self, ui: &mut egui::Ui) {
        let mut config = self.settings.shorts.reframe.clone();
        let mut changed = false;
        changed |= percent_row(ui, &t!("reframe.follow"), &mut config.follow, 0.0..=1.0);
        changed |= percent_row(
            ui,
            &t!("reframe.dead_zone"),
            &mut config.dead_zone,
            reframe::DEAD_ZONE,
        );
        changed |= percent_row(
            ui,
            &t!("reframe.min_zoom"),
            &mut config.min_zoom,
            reframe::MIN_ZOOM,
        );
        let row = param_row(ui, &t!("reframe.margin"), None, |ui| {
            slider_field(ui, &mut config.margin, reframe::MARGIN, 0.02, 1)
        });
        changed |= row.changed;
        changed |= param_row(ui, &t!("reframe.placement"), None, |ui| {
            let mut changed = false;
            for (value, text) in [
                (Placement::UpperThird, t!("reframe.upper_third")),
                (Placement::Center, t!("reframe.center")),
            ] {
                if ui
                    .selectable_label(config.placement == value, text.as_ref())
                    .clicked()
                    && config.placement != value
                {
                    config.placement = value;
                    changed = true;
                }
            }
            changed
        })
        .changed;
        changed |= ui
            .checkbox(&mut config.follow_speaker, t!("reframe.follow_speaker"))
            .on_hover_text(t!("reframe.follow_speaker_hint"))
            .changed();
        let mut step = config.step as f32;
        let row = param_row(ui, &t!("reframe.step"), None, |ui| {
            slider_field(
                ui,
                &mut step,
                *reframe::STEP.start() as f32..=*reframe::STEP.end() as f32,
                0.2,
                0,
            )
        });
        if row.changed {
            // A new step is a new analysis: the preview cannot follow it.
            config.step = step.round() as u32;
        }
        ui.weak(t!("reframe.step_hint"));
        self.reframe.dirty |= changed;
        self.settings.shorts.reframe = config;
    }

    fn show_face_choice(
        &mut self,
        ui: &mut egui::Ui,
        subject: &Subject,
        analysis: &Analysis,
        clip_key: ClipKey,
    ) {
        let playhead = self.timeline_state.playhead;
        if !subject.clip.contains(playhead) {
            return;
        }
        let frame = subject.clip.source_frame_at(playhead);
        let Some(sample) = analysis.sample_at(frame) else {
            return;
        };
        let mut faces: Vec<_> = sample.faces.iter().collect();
        if faces.len() < 2 {
            return;
        }
        faces.sort_by(|a, b| a.center[0].total_cmp(&b.center[0]));
        ui.add_space(theme::SPACE_1);
        ui.label(t!("reframe.choose_face"));
        let mut chosen = None;
        ui.horizontal_wrapped(|ui| {
            for (i, face) in faces.iter().enumerate() {
                if ui.button(t!("reframe.face_n", n = i + 1)).clicked() {
                    chosen = Some(face.center);
                }
            }
        });
        let pins = self.reframe.pins.get(&clip_key).map_or(0, Vec::len);
        if pins > 0
            && ui
                .small_button(t!("reframe.clear_choices", count = pins))
                .clicked()
        {
            self.reframe.pins.remove(&clip_key);
            self.reframe.dirty = true;
        }
        if let Some(at) = chosen {
            self.pin_reframe_face(clip_key, frame, at);
        }
    }

    fn pin_reframe_face(&mut self, clip_key: ClipKey, frame: FrameIdx, at: [f32; 2]) {
        let pins = self.reframe.pins.entry(clip_key).or_default();
        pins.retain(|p| p.frame != frame);
        pins.push(Pin { frame, at });
        if self
            .reframe
            .preview
            .as_ref()
            .is_some_and(|p| p.clip == clip_key)
        {
            self.reframe.dirty = true;
        } else {
            self.reframe.pending_preview = true;
        }
    }

    fn show_missing_faces(&self, ui: &mut egui::Ui, missing: &[Range<FrameIdx>], fps: f64) {
        if missing.is_empty() {
            return;
        }
        let ranges: Vec<String> = missing
            .iter()
            .take(6)
            .map(|r| {
                format!(
                    "{}–{}",
                    crate::downloader_ui::format_time(r.start as f64 / fps),
                    crate::downloader_ui::format_time(r.end as f64 / fps)
                )
            })
            .collect();
        let more = missing.len().saturating_sub(ranges.len());
        let mut text = ranges.join(", ");
        if more > 0 {
            text.push_str(&format!(" (+{more})"));
        }
        ui.colored_label(theme::WARNING, t!("reframe.no_faces", ranges = text));
    }

    fn start_reframe_job(&mut self, subject: &Subject) {
        if let Some(job) = self.reframe.job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.reframe.error = None;
        let progress = Arc::new(vv_reframe::Progress::default());
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let key = subject.key;
        let (path, size, fps) = (subject.path.clone(), subject.media_size, subject.fps);
        let hw = crate::hw_decode::devices();
        let (job_progress, job_cancel) = (progress.clone(), cancel.clone());
        std::thread::Builder::new()
            .name("reframe".into())
            .spawn(move || {
                let cache = cache_dir()
                    .map(|dir| vv_reframe::cache_path(&dir, key.0, (key.1, key.2), key.3));
                let result = vv_reframe::analyze(
                    &path,
                    size,
                    (key.1, key.2),
                    fps,
                    key.3,
                    &hw,
                    cache.as_deref(),
                    &job_progress,
                    &job_cancel,
                )
                .map_err(|e| e.to_string());
                let _ = tx.send(result);
            })
            .expect("spawning the reframe thread");
        self.reframe.job = Some(Job {
            key,
            progress,
            cancel,
            rx,
        });
    }

    fn poll_reframe_job(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.reframe.job else {
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
        let key = job.key;
        self.reframe.job = None;
        match result {
            Ok(analysis) => {
                self.reframe.analyses.insert(key, Arc::new(analysis));
            }
            Err(e) => self.reframe.error = Some((key, e)),
        }
    }

    /// Whether someone talks at a media frame of the clip, from the speech
    /// cache when the clip was transcribed; without it every moment counts.
    fn reframe_speech(
        &self,
        timeline: TimelineId,
        subject: &Subject,
    ) -> Box<dyn Fn(FrameIdx) -> bool> {
        let Some(spec) = self.speech_source(timeline, &subject.clip) else {
            return Box::new(|_| true);
        };
        let Some(source) = self.cached_speech(&spec) else {
            return Box::new(|_| true);
        };
        let fps = subject.fps;
        let picture_start = subject.clip.source_in() as f64 / fps;
        let offset = spec.media_start - picture_start;
        Box::new(move |frame| {
            let secs = frame as f64 / fps + offset;
            let step = source.energy.step.max(1e-6);
            let i = (secs / step).floor().max(0.0) as usize;
            source.energy.db.get(i).is_some_and(|db| *db > SPEECH_DB)
        })
    }

    /// Applies the framing as one history step, replacing the previewed one.
    fn preview_reframe(&mut self, subject: &Subject, clip_key: ClipKey, analysis: &Analysis) {
        if self
            .reframe
            .preview
            .as_ref()
            .is_some_and(|p| p.clip == clip_key)
        {
            self.session.history.undo(&mut self.session.project);
            self.reframe.preview = None;
        }
        let (timeline, track_index, clip_id) = clip_key;
        let tl = &self.session.project.timelines[timeline];
        let Some(clip) = tl.clip(track_index, clip_id).cloned() else {
            return;
        };
        let resolution = tl.resolution;
        let speech = self.reframe_speech(timeline, subject);
        let pins = self
            .reframe
            .pins
            .get(&clip_key)
            .cloned()
            .unwrap_or_default();
        let plan = reframe::plan(
            analysis,
            &self.settings.shorts.reframe,
            &pins,
            &*speech,
            subject.fps,
            resolution,
        );
        let (transform, auto) = reframe::keyframes(&clip, &plan);
        self.session.history.do_command(
            &mut self.session.project,
            Box::new(
                SetAutoZoom::new(timeline, track_index, clip_id, transform, auto)
                    .with_label(CommandLabel::Reframe),
            ),
        );
        self.reframe.preview = Some(Preview {
            clip: clip_key,
            generation: self.session.history.generation(),
            missing: plan.missing,
            scenes: plan.scenes.len(),
        });
    }

    /// The faces of the analysis under the playhead, over the player, while
    /// the tab is open; a click follows that face for the rest of its shot.
    pub(crate) fn show_reframe_faces(&mut self, ui: &egui::Ui, rect: egui::Rect, area: egui::Rect) {
        let Some((pass, clip_key, key)) = self.reframe.shown else {
            return;
        };
        if ui.ctx().cumulative_pass_nr() > pass + 1
            || self.browsing.is_some()
            || self.is_timeline_playing()
        {
            return;
        }
        let Some(analysis) = self.reframe.analyses.get(&key).cloned() else {
            return;
        };
        let (timeline, track_index, clip_id) = clip_key;
        let Some(tl) = self.session.project.timelines.get(timeline) else {
            return;
        };
        let Some(clip) = tl.clip(track_index, clip_id) else {
            return;
        };
        let playhead = self.timeline_state.playhead;
        if !clip.contains(playhead) {
            return;
        }
        let frame = clip.source_frame_at(playhead);
        let Some(sample) = analysis.sample_at(frame) else {
            return;
        };
        let (tw, th) = (tl.resolution.0.max(1) as f32, tl.resolution.1.max(1) as f32);
        let (sw, sh) = (analysis.size.0.max(1) as f32, analysis.size.1.max(1) as f32);
        let fit = (tw / sw).min(th / sh);
        let transform = clip.effects.transform.value_at(frame);
        let to_screen = |u: f32, v: f32| {
            let p = [(u - 0.5) * sw * fit, (0.5 - v) * sh * fit];
            let f = crate::viewer_overlay::clip_to_frame(&transform, p);
            egui::pos2(
                rect.center().x + f[0] * rect.width() / tw,
                rect.center().y - f[1] * rect.height() / th,
            )
        };
        let pinned = self
            .reframe
            .pins
            .get(&clip_key)
            .and_then(|pins| pins.iter().find(|p| p.frame == frame))
            .map(|p| p.at);
        let painter = ui.painter_at(rect.intersect(area));
        let mut chosen = None;
        for (i, face) in sample.faces.iter().enumerate() {
            let a = to_screen(
                face.center[0] - face.size[0] / 2.0,
                face.center[1] - face.size[1] / 2.0,
            );
            let b = to_screen(
                face.center[0] + face.size[0] / 2.0,
                face.center[1] + face.size[1] / 2.0,
            );
            let face_rect = egui::Rect::from_two_pos(a, b);
            let is_pinned = pinned.is_some_and(|p| p == face.center);
            let response = ui
                .interact(
                    face_rect.intersect(area),
                    ui.id().with(("reframe_face", i)),
                    egui::Sense::click(),
                )
                .on_hover_text(t!("reframe.click_face"));
            let color = if is_pinned || response.hovered() {
                theme::ACCENT
            } else {
                egui::Color32::from_white_alpha(170)
            };
            painter.rect_stroke(
                face_rect,
                theme::RADIUS_CONTROL,
                egui::Stroke::new(if is_pinned { 2.5 } else { 1.5 }, color),
                egui::StrokeKind::Middle,
            );
            if response.clicked() {
                chosen = Some(face.center);
            }
        }
        if let Some(at) = chosen {
            self.pin_reframe_face(clip_key, frame, at);
        }
    }
}

pub(crate) fn percent_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    let mut p = *value * 100.0;
    let row = param_row(ui, label, None, |ui| {
        slider_field(
            ui,
            &mut p,
            range.start() * 100.0..=range.end() * 100.0,
            0.5,
            0,
        )
    });
    if row.changed {
        *value = p / 100.0;
    }
    row.changed
}
