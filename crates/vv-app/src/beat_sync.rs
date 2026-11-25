//! «Beat» panel: tempo analysis of the selected music clip on a worker,
//! beat markers, snap toggle and «Tagli sui beat».

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, mpsc};

use vv_audio::beat::{BeatError, Beats, OnsetDetector, SAMPLE_RATE};
use vv_core::beat_sync::{self, CutsError, SetMarkers};
use vv_core::{ClipId, TimelineId, TrackKind};

use crate::auto_cut::SourceKey;
use crate::{VenturiApp, theme};

#[derive(Default)]
pub(crate) struct State {
    pub open: bool,
    job: Option<Job>,
    results: HashMap<SourceKey, Result<Arc<Beats>, String>>,
    preview: Option<u64>,
    message: Option<String>,
}

struct Job {
    key: SourceKey,
    cancel: Arc<AtomicBool>,
    /// 0..=1000.
    progress: Arc<AtomicU32>,
    rx: mpsc::Receiver<Result<Beats, String>>,
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
}

fn analyse(
    path: PathBuf,
    stream: usize,
    duration: f64,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<Beats, String> {
    let mut detector = OnsetDetector::new();
    let mut mono = Vec::new();
    let formats = vv_media::decode_audio_streams_streaming(
        &path,
        &[stream],
        Some(SAMPLE_RATE),
        |_, channels, chunk| {
            if cancel.load(Ordering::Relaxed) {
                return std::ops::ControlFlow::Break(());
            }
            let channels = usize::from(channels.max(1));
            mono.clear();
            mono.extend(
                chunk
                    .chunks_exact(channels)
                    .map(|f| f.iter().sum::<f32>() / channels as f32),
            );
            detector.push(&mono);
            progress.store(
                (detector.seconds() / duration.max(0.01) * 1000.0).min(999.0) as u32,
                Ordering::Relaxed,
            );
            std::ops::ControlFlow::Continue(())
        },
    )
    .map_err(|e| e.to_string())?;
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled".into());
    }
    if formats.first().copied().flatten().is_none() {
        return Err("no_audio".into());
    }
    detector.finish().map_err(|e| {
        match e {
            BeatError::TooShort => "too_short",
            BeatError::Silent => "silent",
            BeatError::NoRhythm => "no_rhythm",
        }
        .into()
    })
}

fn error_text(error: &str) -> std::borrow::Cow<'static, str> {
    match error {
        "too_short" => t!("beat.too_short"),
        "silent" => t!("beat.silent"),
        "no_rhythm" => t!("beat.no_rhythm"),
        "no_audio" => t!("beat.no_audio"),
        "offline" => t!("beat.offline"),
        "unsupported" => t!("beat.unsupported"),
        "cancelled" => t!("beat.cancelled"),
        other => t!("beat.error", detail = other),
    }
}

fn cuts_error(error: CutsError) -> std::borrow::Cow<'static, str> {
    match error {
        CutsError::NoClips => t!("beat.cuts_no_clips"),
        CutsError::NotEnoughBeats => t!("beat.cuts_no_beats"),
        CutsError::NoBeatAfterStart => t!("beat.cuts_no_beat_after"),
        CutsError::Locked => t!("beat.cuts_locked"),
    }
}

impl VenturiApp {
    /// The selected clip to analyse: an audio-track clip first.
    fn beat_target(&self, timeline: TimelineId) -> Option<(usize, ClipId)> {
        let tl = &self.session.project.timelines[timeline];
        self.timeline_state
            .selected
            .iter()
            .copied()
            .filter(|&(t, id)| {
                tl.clip(t, id)
                    .is_some_and(|c| matches!(c.source, vv_core::ClipSource::Media(_)))
            })
            .min_by_key(|&(t, _)| usize::from(tl.tracks[t].kind != TrackKind::Audio))
    }

    fn poll_beat_job(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.beat.job else {
            return;
        };
        let result = match job.rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Disconnected) => Err("worker_stopped".into()),
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
        };
        let job = self.beat.job.take().unwrap();
        if self.beat.results.len() >= 8 {
            self.beat.results.clear();
        }
        let placed = result.is_ok();
        self.beat
            .results
            .insert(job.key.clone(), result.map(Arc::new));
        if placed {
            self.place_beat_markers(&job.key);
        }
    }

    /// Beat markers over the selected clip from a finished analysis.
    fn place_beat_markers(&mut self, key: &SourceKey) {
        let Some(timeline) = self.timeline_id else {
            return;
        };
        let Some(Ok(beats)) = self.beat.results.get(key).cloned() else {
            return;
        };
        let Some(target) = self.beat_target(timeline) else {
            return;
        };
        let tl = &self.session.project.timelines[timeline];
        let Some(clip) = tl.clip(target.0, target.1) else {
            return;
        };
        let Some(spec) = self.speech_source(timeline, clip) else {
            return;
        };
        if spec.key.as_ref() != Some(key) {
            return;
        }
        let markers =
            beat_sync::with_clip_beats(tl, clip, &beats.times, spec.media_start, spec.media_speed);
        self.session.history.do_command(
            &mut self.session.project,
            Box::new(SetMarkers::new(timeline, markers)),
        );
    }

    pub(crate) fn show_beat_window(&mut self, ctx: &egui::Context) {
        self.timeline_state.beat_snap = self.settings.shorts.beat_snap;
        self.poll_beat_job(ctx);
        if self
            .beat
            .preview
            .is_some_and(|g| g != self.session.history.generation())
        {
            self.beat.preview = None;
        }
        if !self.beat.open {
            return;
        }
        let mut open = true;
        egui::Window::new(t!("beat.title"))
            .open(&mut open)
            .resizable(false)
            .default_width(300.0)
            .show(ctx, |ui| self.beat_window_contents(ui));
        if !open {
            self.beat.open = false;
            if let Some(job) = self.beat.job.take() {
                job.cancel.store(true, Ordering::Relaxed);
            }
        }
    }

    fn beat_window_contents(&mut self, ui: &mut egui::Ui) {
        let Some(timeline) = self.timeline_id else {
            ui.weak(t!("beat.no_timeline"));
            return;
        };
        ui.label(egui::RichText::new(t!("beat.analysis")).strong());
        let target = self.beat_target(timeline);
        let tl = &self.session.project.timelines[timeline];
        let spec = target
            .and_then(|(t, id)| tl.clip(t, id))
            .and_then(|clip| self.speech_source(timeline, clip));
        match &spec {
            None => {
                ui.weak(t!("beat.select_music"));
            }
            Some(spec) => {
                if let Some(error) = spec.error {
                    ui.colored_label(theme::ERROR, error_text(error));
                }
            }
        }
        let key = spec
            .as_ref()
            .filter(|s| s.error.is_none())
            .and_then(|s| s.key.clone());
        if let Some(job) = &self.beat.job {
            ui.label(t!("beat.analysing"));
            ui.horizontal(|ui| {
                ui.add(
                    egui::ProgressBar::new(job.progress.load(Ordering::Relaxed) as f32 / 1000.0)
                        .show_percentage()
                        .desired_width(ui.available_width() - 80.0),
                );
                if ui.button(t!("beat.cancel")).clicked() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
            });
        } else if let (Some(key), Some(spec)) = (key.clone(), &spec)
            && ui.button(t!("beat.analyse")).clicked()
        {
            let cancel = Arc::new(AtomicBool::new(false));
            let progress = Arc::new(AtomicU32::new(0));
            let (tx, rx) = mpsc::channel();
            let ctx = ui.ctx().clone();
            let (path, stream, duration) = (spec.path.clone(), spec.stream, spec.duration);
            std::thread::spawn({
                let cancel = cancel.clone();
                let progress = progress.clone();
                move || {
                    let _ = tx.send(analyse(path, stream, duration, &cancel, &progress));
                    ctx.request_repaint();
                }
            });
            self.beat.job = Some(Job {
                key,
                cancel,
                progress,
                rx,
            });
        }
        if let Some(result) = key.as_ref().and_then(|k| self.beat.results.get(k)) {
            match result {
                Ok(beats) => {
                    ui.label(t!(
                        "beat.result",
                        bpm = format!("{:.0}", beats.bpm),
                        count = beats.times.len()
                    ));
                    if !beats.stable {
                        ui.colored_label(theme::WARNING, t!("beat.unstable"));
                    } else if beats.clarity < 0.25 {
                        ui.colored_label(theme::WARNING, t!("beat.weak"));
                    }
                }
                Err(e) => {
                    ui.colored_label(theme::ERROR, error_text(e));
                }
            }
        }

        ui.separator();
        ui.label(egui::RichText::new(t!("beat.markers")).strong());
        let count = self.session.project.timelines[timeline]
            .markers
            .iter()
            .filter(|m| m.beat)
            .count();
        ui.weak(t!("beat.marker_count", count = count));
        ui.checkbox(&mut self.settings.shorts.beat_snap, t!("beat.snap"))
            .on_hover_text(t!("beat.snap_hint"));
        if ui
            .add_enabled(count > 0, egui::Button::new(t!("beat.remove_markers")))
            .clicked()
        {
            let markers = beat_sync::without_beats(&self.session.project.timelines[timeline]);
            self.session.history.do_command(
                &mut self.session.project,
                Box::new(SetMarkers::new(timeline, markers)),
            );
        }

        ui.separator();
        ui.label(egui::RichText::new(t!("beat.cuts_title")).strong());
        ui.weak(t!("beat.cuts_hint"));
        ui.horizontal(|ui| {
            ui.label(t!("beat.every"));
            for n in [1_u8, 2, 4] {
                ui.selectable_value(&mut self.settings.shorts.beat_every, n, n.to_string());
            }
        });
        match self.beat.preview {
            Some(_) => {
                ui.horizontal(|ui| {
                    if ui
                        .add(crate::ui_kit::primary_button(t!("beat.apply")))
                        .clicked()
                    {
                        self.beat.preview = None;
                    }
                    if ui.button(t!("beat.discard")).clicked() {
                        self.beat.preview = None;
                        self.session.history.undo(&mut self.session.project);
                    }
                });
            }
            None => {
                if ui
                    .add_enabled(
                        count > 0 && !self.timeline_state.selected.is_empty(),
                        egui::Button::new(t!("beat.preview")),
                    )
                    .clicked()
                {
                    let selected: Vec<_> = self.timeline_state.selected.iter().copied().collect();
                    match beat_sync::cuts_on_beats(
                        &mut self.session.project,
                        timeline,
                        &selected,
                        usize::from(self.settings.shorts.beat_every),
                    ) {
                        Ok(command) => {
                            self.session
                                .history
                                .do_command(&mut self.session.project, Box::new(command));
                            self.beat.preview = Some(self.session.history.generation());
                            let tl = &self.session.project.timelines[timeline];
                            self.timeline_state.selected = selected
                                .iter()
                                .filter_map(|&(_, id)| {
                                    tl.tracks
                                        .iter()
                                        .position(|t| t.clip(id).is_some())
                                        .map(|t| (t, id))
                                })
                                .collect();
                            self.beat.message = None;
                        }
                        Err(e) => self.beat.message = Some(cuts_error(e).into_owned()),
                    }
                }
            }
        }
        if let Some(message) = &self.beat.message {
            ui.colored_label(theme::ERROR, message);
        }
    }
}
