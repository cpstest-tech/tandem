//! «Zoom automatici» sub-tab of the Video properties: punch-ins from the
//! transcript of the clip (the Automatic cut source cache), previewed as a
//! real history step that is replaced while the parameters change.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use vv_core::auto_zoom::{self, Config, Frequency, Origin, SetAutoZoom, Speed};
use vv_core::{ClipId, ClipSource, TimelineId};

use crate::auto_cut::{Cached, SourceJob, SourceKey, clip_local, start_source};
use crate::properties_panel::{param_row, section_header, slider_field};
use crate::{PanelTarget, VenturiApp, theme};

#[derive(Default)]
pub(crate) struct State {
    job: Option<(SourceKey, SourceJob, bool)>,
    /// Last lookup outcome per source: `no_transcript`, `model_missing`...
    error: Option<(SourceKey, String)>,
    preview: Option<Preview>,
    dirty: bool,
}

struct Preview {
    clip: (TimelineId, usize, ClipId),
    generation: u64,
    count: usize,
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some((_, job, _)) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
}

fn error_text(error: &str) -> std::borrow::Cow<'static, str> {
    match error {
        "no_transcript" => t!("auto_zoom.no_transcript"),
        "model_missing" => t!("auto_zoom.model_missing"),
        "no_audio" => t!("auto_zoom.no_audio"),
        "offline" => t!("auto_zoom.offline"),
        "unsupported" => t!("auto_zoom.unsupported"),
        "no_words" => t!("auto_zoom.no_words"),
        "cancelled" => t!("auto_zoom.cancelled"),
        other => t!("auto_zoom.error", detail = other),
    }
}

impl VenturiApp {
    pub(crate) fn show_auto_zoom_tab(&mut self, ui: &mut egui::Ui, target: &PanelTarget) {
        if self
            .auto_zoom
            .preview
            .as_ref()
            .is_some_and(|p| p.generation != self.session.history.generation())
        {
            // Something else was edited: the previewed step stays as applied.
            self.auto_zoom.preview = None;
        }
        let tl = &self.session.project.timelines[target.timeline];
        let Some(clip) = tl.clip(target.track_index, target.clip_id).cloned() else {
            return;
        };
        let has_video = match clip.source {
            ClipSource::Media(id) => self
                .session
                .project
                .media_pool
                .get(id)
                .is_some_and(|m| m.meta.has_video),
            _ => false,
        };
        ui.add_space(crate::theme::SPACE_2);
        let (open, _) = section_header(ui, &t!("auto_zoom.title"), false);
        if !open {
            return;
        }
        ui.weak(t!("auto_zoom.hint"));
        if !has_video || target.is_text {
            ui.weak(t!("auto_zoom.select_video"));
            return;
        }
        let Some(spec) = self.speech_source(target.timeline, &clip) else {
            ui.weak(t!("auto_zoom.select_video"));
            return;
        };
        if let Some(error) = spec.error {
            ui.colored_label(theme::ERROR, error_text(error));
            return;
        }
        let Some(key) = spec.key.clone() else {
            return;
        };
        self.poll_auto_zoom_job(ui.ctx());
        let source = self.cached_speech(&spec);
        let error = self
            .auto_zoom
            .error
            .as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, e)| e.clone());
        let running = self.auto_zoom.job.as_ref().filter(|(k, _, _)| *k == key);

        if source.is_none() {
            if let Some((_, job, transcribing)) = running {
                let stage = job.progress.stage.load(Ordering::Relaxed);
                ui.label(match (stage, transcribing) {
                    (0, false) => t!("auto_zoom.looking_up"),
                    (0, true) => t!("auto_cut.hashing"),
                    (1, _) => t!("auto_cut.extracting"),
                    _ => t!("auto_cut.transcribing"),
                });
                let fraction = if stage == 2 {
                    job.progress.whisper.fraction()
                } else {
                    job.progress.done.load(Ordering::Relaxed) as f32 / 1000.0
                };
                ui.horizontal(|ui| {
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .show_percentage()
                            .desired_width(ui.available_width() - 80.0),
                    );
                    if ui.button(t!("auto_zoom.cancel")).clicked() {
                        job.cancel.store(true, Ordering::Relaxed);
                    }
                });
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            match error.as_deref() {
                None => {
                    self.start_auto_zoom_job(key, &spec, false, ui.ctx());
                }
                Some(e) => {
                    let missing = e == "no_transcript" || e == "cancelled";
                    ui.colored_label(
                        if missing {
                            theme::TEXT_SECONDARY
                        } else {
                            theme::ERROR
                        },
                        error_text(e),
                    );
                    if missing && ui.button(t!("auto_zoom.transcribe")).clicked() {
                        self.start_auto_zoom_job(key, &spec, true, ui.ctx());
                    }
                }
            }
            return;
        }
        let source = source.unwrap();

        let mut config = self.settings.shorts.auto_zoom.clone();
        let mut changed = false;
        let mut percent = config.intensity * 100.0;
        let row = param_row(ui, &t!("auto_zoom.intensity"), None, |ui| {
            slider_field(
                ui,
                &mut percent,
                auto_zoom::INTENSITY.start() * 100.0..=auto_zoom::INTENSITY.end() * 100.0,
                0.5,
                0,
            )
        });
        if row.changed {
            config.intensity = percent / 100.0;
            changed = true;
        }
        let speed = [
            (Speed::Slow, t!("auto_zoom.speed_slow")),
            (Speed::Normal, t!("auto_zoom.speed_normal")),
            (Speed::Fast, t!("auto_zoom.speed_fast")),
        ];
        changed |= choice_row(ui, &t!("auto_zoom.speed"), &mut config.speed, &speed);
        let frequency = [
            (Frequency::Low, t!("auto_zoom.frequency_low")),
            (Frequency::Medium, t!("auto_zoom.frequency_medium")),
            (Frequency::High, t!("auto_zoom.frequency_high")),
        ];
        if choice_row(
            ui,
            &t!("auto_zoom.frequency"),
            &mut config.frequency,
            &frequency,
        ) {
            config.min_gap = config.frequency.default_gap();
            changed = true;
        }
        let mut gap = config.min_gap as f32;
        let row = param_row(ui, &t!("auto_zoom.min_gap"), None, |ui| {
            slider_field(
                ui,
                &mut gap,
                *auto_zoom::MIN_GAP.start() as f32..=*auto_zoom::MIN_GAP.end() as f32,
                0.05,
                1,
            )
        });
        if row.changed {
            config.min_gap = f64::from(gap);
            changed = true;
        }
        changed |= ui
            .checkbox(&mut config.alternate, t!("auto_zoom.alternate"))
            .changed();
        changed |= ui
            .checkbox(&mut config.shift, t!("auto_zoom.shift"))
            .changed();
        self.settings.shorts.auto_zoom = config.clone();

        let key3 = (target.timeline, target.track_index, target.clip_id);
        let previewing = self
            .auto_zoom
            .preview
            .as_ref()
            .filter(|p| p.clip == key3)
            .map(|p| p.count);
        let pointer_down = ui.input(|i| i.pointer.any_down());
        ui.add_space(theme::SPACE_2);
        match previewing {
            Some(count) => {
                ui.label(t!("auto_zoom.preview_count", count = count));
                // A slider drag regenerates on release, not on every frame.
                self.auto_zoom.dirty |= changed;
                if self.auto_zoom.dirty && !pointer_down {
                    self.auto_zoom.dirty = false;
                    self.preview_auto_zoom(&source, &spec, key3, &config);
                }
                ui.horizontal(|ui| {
                    if ui
                        .add(crate::ui_kit::primary_button(t!("auto_zoom.apply")))
                        .clicked()
                    {
                        self.auto_zoom.preview = None;
                    }
                    if ui.button(t!("auto_zoom.discard")).clicked() {
                        self.auto_zoom.preview = None;
                        self.session.history.undo(&mut self.session.project);
                    }
                });
            }
            None => {
                if ui.button(t!("auto_zoom.preview")).clicked() {
                    self.preview_auto_zoom(&source, &spec, key3, &config);
                }
                let generated = clip
                    .auto_keyframes
                    .iter()
                    .filter(|k| k.origin == Origin::Zoom)
                    .count();
                if ui
                    .add_enabled(generated > 0, egui::Button::new(t!("auto_zoom.remove")))
                    .on_hover_text(t!("auto_zoom.remove_hint"))
                    .clicked()
                {
                    self.session.history.do_command(
                        &mut self.session.project,
                        Box::new(SetAutoZoom::remove(
                            target.timeline,
                            target.track_index,
                            &clip,
                            Origin::Zoom,
                        )),
                    );
                }
            }
        }
        if let Some((k, e)) = &self.auto_zoom.error
            && *k == key
            && e == "no_words"
        {
            ui.colored_label(theme::ERROR, error_text(e));
        }
    }

    fn start_auto_zoom_job(
        &mut self,
        key: SourceKey,
        spec: &crate::auto_cut::SpeechSource,
        transcribe: bool,
        ctx: &egui::Context,
    ) {
        if let Some((_, job, _)) = self.auto_zoom.job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.auto_zoom.error = None;
        let job = start_source(
            spec.path.clone(),
            spec.stream,
            spec.duration,
            self.subtitles.language,
            transcribe,
            ctx.clone(),
        );
        self.auto_zoom.job = Some((key, job, transcribe));
    }

    fn poll_auto_zoom_job(&mut self, ctx: &egui::Context) {
        let Some(result) = self
            .auto_zoom
            .job
            .as_ref()
            .and_then(|(_, job, _)| job.poll())
        else {
            if self.auto_zoom.job.is_some() {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            return;
        };
        let (key, _, _) = self.auto_zoom.job.take().unwrap();
        match result {
            Ok(source) => self.remember_speech(&key, &source),
            Err(error) => self.auto_zoom.error = Some((key, error)),
        }
    }

    /// Applies the zooms as one history step, replacing the previewed one.
    fn preview_auto_zoom(
        &mut self,
        source: &Arc<Cached>,
        spec: &crate::auto_cut::SpeechSource,
        (timeline, track_index, clip_id): (TimelineId, usize, ClipId),
        config: &Config,
    ) {
        if self
            .auto_zoom
            .preview
            .as_ref()
            .is_some_and(|p| p.clip == (timeline, track_index, clip_id))
        {
            self.session.history.undo(&mut self.session.project);
            self.auto_zoom.preview = None;
        }
        let tl = &self.session.project.timelines[timeline];
        let Some(clip) = tl.clip(track_index, clip_id) else {
            return;
        };
        let fps = tl.fps.as_f64();
        let duration = clip.timeline_len as f64 / fps;
        let (words, energy) = clip_local(source, spec.media_start, spec.media_speed, duration);
        let zooms = auto_zoom::plan(&words, &energy, duration, config);
        if let Some(key) = &spec.key {
            self.auto_zoom.error = zooms
                .is_empty()
                .then(|| (key.clone(), "no_words".to_owned()));
        }
        if zooms.is_empty() {
            return;
        }
        let (transform, auto) = auto_zoom::keyframes(clip, &zooms, config, fps, tl.resolution);
        let count = zooms.len();
        self.session.history.do_command(
            &mut self.session.project,
            Box::new(SetAutoZoom::new(
                timeline,
                track_index,
                clip_id,
                transform,
                auto,
            )),
        );
        self.auto_zoom.preview = Some(Preview {
            clip: (timeline, track_index, clip_id),
            generation: self.session.history.generation(),
            count,
        });
    }
}

fn choice_row<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut T,
    items: &[(T, std::borrow::Cow<'static, str>)],
) -> bool {
    param_row(ui, label, None, |ui| {
        let mut changed = false;
        for (item, text) in items {
            if ui
                .selectable_label(*value == *item, text.as_ref())
                .clicked()
                && *value != *item
            {
                *value = *item;
                changed = true;
            }
        }
        changed
    })
    .changed
}
