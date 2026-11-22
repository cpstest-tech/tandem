//! The export modal: settings, then the render's progress in the same window.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use vv_core::dub::Language;
use vv_core::{FrameIdx, Project, Timeline, TimelineId, Track, TrackKind};
use vv_media::{HwDevice, VideoBitrate, VideoCodec, VideoSettings};
use vv_session::export::{ExportError, ExportProgress, ExportSettings};
use vv_session::motion_blur::{
    Deduplicate, FpsTarget, MotionBlurSettings, Weighting, blend_summary,
};

use super::*;
use crate::cover::{CoverRender, paint_fitted, paint_skeleton};
use crate::export_plan::{
    AUDIO_KBPS, AudioFormat, BitrateChoice, CaptionFormat, CodecChoice, Container, Estimate,
    FpsChoice, ResolutionChoice,
};
use crate::properties_panel::preview_combo;

const MODAL_WIDTH: f32 = 760.0;
const LEFT_WIDTH: f32 = 250.0;
const LABEL_WIDTH: f32 = 130.0;
const BODY_HEIGHT: f32 = 430.0;
const MIN_MODAL_WIDTH: f32 = 300.0;
const MIN_BODY_HEIGHT: f32 = 160.0;
/// Title, footer and margins around the scrolled body.
const MODAL_CHROME_HEIGHT: f32 = 220.0;
const MAX_CUSTOM_MBPS: f64 = 200.0;

pub(crate) struct ExportModal {
    name: String,
    dir: String,
    video_on: bool,
    video_open: bool,
    resolution: ResolutionChoice,
    bitrate: BitrateChoice,
    custom_mbps: f64,
    custom_in_kbps: bool,
    constant: bool,
    codec: CodecChoice,
    container: Container,
    fps: FpsChoice,
    audio_on: bool,
    audio_open: bool,
    audio_format: AudioFormat,
    language: Language,
    captions_on: bool,
    captions_open: bool,
    caption_format: CaptionFormat,
    blur_on: bool,
    blur_open: bool,
    blur: MotionBlurForm,
    hw_decode: Vec<HwDevice>,
    detected_kbps: Option<u32>,
    probe: Option<mpsc::Receiver<Option<u32>>>,
    cover: CoverRender,
    browsing: Option<mpsc::Receiver<Option<PathBuf>>>,
    preview: Option<(u64, egui::TextureHandle)>,
    /// What the running export writes, for "open the folder".
    started_dir: Option<PathBuf>,
}

/// Read-only data the modal needs each frame.
struct Env<'a> {
    project: &'a Project,
    timeline: TimelineId,
    cover_frame: FrameIdx,
    range: std::ops::Range<FrameIdx>,
    waits_for_scan: bool,
    /// Background removal still analysing a clip: how far, 0..1.
    cutout_pending: Option<f32>,
    empty_material: usize,
    running: Option<&'a Arc<Mutex<ExportProgress>>>,
}

struct StartRequest {
    settings: ExportSettings,
    range: std::ops::Range<FrameIdx>,
    captions: Option<(PathBuf, String)>,
}

enum Outcome {
    None,
    Close,
    EditCover,
    Start(Box<StartRequest>),
    CancelRender,
}

struct Derived {
    size: (u32, u32),
    fps: vv_core::Rational,
    mbps: f64,
    seconds: f64,
    estimate: Estimate,
}

fn has_media(project: &Project) -> bool {
    project.media_pool.keys().next().is_some()
}

impl ExportModal {
    fn new(project: &Project, timeline: TimelineId, project_name: &str, dir: PathBuf) -> Self {
        let tl = &project.timelines[timeline];
        let short = tl.resolution.0.min(tl.resolution.1);
        let (tx, rx) = mpsc::channel();
        let paths: Vec<PathBuf> = export_plan::active_video_media(project, timeline)
            .into_iter()
            .filter_map(|id| project.media_pool.get(id).map(|item| item.path.clone()))
            .collect();
        std::thread::spawn(move || {
            let best = paths
                .iter()
                .filter_map(|path| export_plan::probe_bitrate_kbps(path))
                .max();
            let _ = tx.send(best);
        });
        let container = Container::Mp4;
        let name = export_plan::unique_name(&export_plan::sanitize_name(project_name), |n| {
            dir.join(format!("{n}.{}", container.extension())).exists()
        });
        Self {
            name,
            dir: dir.display().to_string(),
            video_on: true,
            video_open: true,
            resolution: export_plan::default_resolution(short, has_media(project)),
            bitrate: BitrateChoice::MatchBest,
            custom_mbps: 10.0,
            custom_in_kbps: false,
            constant: false,
            codec: CodecChoice::H264,
            container,
            fps: FpsChoice::MatchBest,
            audio_on: false,
            audio_open: true,
            audio_format: AudioFormat::Mp3,
            language: Language::AsIs,
            captions_on: false,
            captions_open: true,
            caption_format: CaptionFormat::Srt,
            blur_on: false,
            blur_open: true,
            blur: MotionBlurForm::default(),
            hw_decode: crate::hw_decode::export_choices(),
            detected_kbps: None,
            probe: Some(rx),
            cover: CoverRender::default(),
            browsing: None,
            preview: None,
            started_dir: None,
        }
    }

    fn derive(&self, env: &Env) -> Derived {
        let project = env.project;
        let tl = &project.timelines[env.timeline];
        let media = export_plan::active_video_media(project, env.timeline);
        let project_short = tl.resolution.0.min(tl.resolution.1);
        let short = export_plan::target_short_side(
            self.resolution,
            project_short,
            export_plan::best_short_side(project, &media),
        );
        let size = export_plan::scaled_size(tl.resolution, short);
        let fps = export_plan::target_fps(self.fps, tl.fps, export_plan::best_fps(project, &media));
        let recommended = export_plan::recommended_mbps(size, fps.as_f64(), self.codec.factor());
        let detected = self.detected_kbps.map(|kbps| f64::from(kbps) / 1000.0);
        let mbps = export_plan::choose_mbps(self.bitrate, recommended, detected, self.custom_mbps);
        let seconds = (env.range.end - env.range.start).max(0) as f64 / tl.fps.as_f64();
        let estimate = Estimate {
            video_mbps: mbps,
            has_video: self.video_on,
            video_has_audio: tl.has_audio_clips(),
            audio_file: self.audio_on.then_some(self.audio_format),
            seconds,
        };
        Derived {
            size,
            fps,
            mbps,
            seconds,
            estimate,
        }
    }

    fn poll_probe(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.probe else {
            return;
        };
        match rx.try_recv() {
            Ok(best) => {
                self.detected_kbps = best;
                self.probe = None;
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            Err(mpsc::TryRecvError::Disconnected) => self.probe = None,
        }
    }

    fn poll_browse(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.browsing else {
            return;
        };
        match rx.try_recv() {
            Ok(chosen) => {
                if let Some(path) = chosen {
                    self.dir = path.display().to_string();
                }
                self.browsing = None;
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            Err(mpsc::TryRecvError::Disconnected) => self.browsing = None,
        }
    }

    fn browse(&mut self) {
        let current = PathBuf::from(&self.dir);
        let (tx, rx) = mpsc::channel();
        // On its own thread: the dialog would block the event loop.
        std::thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new();
            if current.is_dir() {
                dialog = dialog.set_directory(&current);
            }
            let _ = tx.send(dialog.pick_folder());
        });
        self.browsing = Some(rx);
    }

    fn output_path(&self) -> PathBuf {
        Path::new(self.dir.trim()).join(format!(
            "{}.{}",
            export_plan::sanitize_name(&self.name),
            self.container.extension()
        ))
    }

    fn can_start(&self, env: &Env) -> bool {
        if self.video_on && self.blur_on && self.blur.settings().is_none() {
            return false;
        }
        let scanning = env.waits_for_scan && env.project.timelines[env.timeline].has_audio_clips();
        (self.video_on || self.audio_on)
            && !self.name.trim().is_empty()
            && !self.dir.trim().is_empty()
            && !scanning
            && env.cutout_pending.is_none()
            && env.range.end > env.range.start
    }

    fn request(&self, env: &Env, derived: &Derived) -> StartRequest {
        let tl = &env.project.timelines[env.timeline];
        let mut settings = ExportSettings::with_preferred_audio(self.output_path());
        settings.video_enabled = self.video_on;
        settings.custom_size = (derived.size != tl.resolution).then_some(derived.size);
        settings.fps = (derived.fps != tl.fps).then_some(derived.fps);
        settings.hw_decode = self.hw_decode.clone();
        settings.video = VideoSettings::for_codec(hardware_encoder());
        settings.video.bitrate = Some(VideoBitrate {
            kbps: (derived.mbps * 1000.0).round().max(1.0) as u32,
            constant: self.bitrate == BitrateChoice::Custom && self.constant,
        });
        if !tl.has_audio_clips() {
            settings.audio = None;
        }
        if self.video_on && self.blur_on {
            settings.motion_blur = self.blur.settings().map(Box::new);
        }
        if self.audio_on {
            settings.audio_file = Some(vv_session::export::AudioFileExport {
                format: self.audio_format.engine(),
                bitrate_kbps: AUDIO_KBPS,
            });
        }
        settings.language = self.language.clone();
        let captions = self
            .caption_track(tl)
            .filter(|_| self.captions_on)
            .map(|track| {
                let text = match self.caption_format {
                    CaptionFormat::Srt => vv_core::captions::track_to_srt(track, tl.fps),
                    CaptionFormat::Txt => vv_core::captions::track_to_txt(track),
                };
                (
                    settings
                        .output_path
                        .with_extension(self.caption_format.extension()),
                    text,
                )
            });
        StartRequest {
            settings,
            range: env.range.clone(),
            captions,
        }
    }

    fn show(&mut self, ctx: &egui::Context, env: &Env) -> Outcome {
        self.poll_probe(ctx);
        self.poll_browse(ctx);
        for codec in VideoCodec::choices() {
            let _ = codec.availability();
        }
        let mut outcome = Outcome::None;
        let screen = ctx.content_rect().size();
        let width = MODAL_WIDTH
            .min(screen.x - 2.0 * theme::SPACE_5)
            .max(MIN_MODAL_WIDTH);
        let body = BODY_HEIGHT
            .min(screen.y - MODAL_CHROME_HEIGHT)
            .max(MIN_BODY_HEIGHT);
        let modal = egui::Modal::new(egui::Id::new("export_modal")).show(ctx, |ui| {
            ui.set_width(width);
            ui.label(egui::RichText::new(t!("export.title")).size(theme::FONT_MODAL_TITLE));
            ui.add_space(theme::SPACE_3);
            match env.running {
                Some(progress) => outcome = self.running_view(ui, progress),
                None => outcome = self.settings_view(ui, env, width, body),
            }
        });
        if env.running.is_none() && modal.should_close() {
            return Outcome::Close;
        }
        outcome
    }

    fn settings_view(&mut self, ui: &mut egui::Ui, env: &Env, width: f32, body: f32) -> Outcome {
        let derived = self.derive(env);
        let mut outcome = Outcome::None;
        if width >= MODAL_WIDTH * 0.85 {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(LEFT_WIDTH);
                    if self.cover_view(ui, env) {
                        outcome = Outcome::EditCover;
                    }
                });
                ui.add_space(theme::SPACE_3);
                ui.vertical(|ui| {
                    egui::ScrollArea::vertical()
                        .max_height(body)
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.form(ui, env, &derived));
                });
            });
        } else {
            // Narrow window: the cover goes inside the scrolled column.
            egui::ScrollArea::vertical()
                .max_height(body)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.set_width(LEFT_WIDTH.min(width));
                        if self.cover_view(ui, env) {
                            outcome = Outcome::EditCover;
                        }
                    });
                    ui.add_space(theme::SPACE_3);
                    self.form(ui, env, &derived);
                });
        }
        ui.add_space(theme::SPACE_2);
        ui.separator();
        ui.add_space(theme::SPACE_1);
        let start = self.footer(ui, env, &derived);
        if let Some(close) = start {
            outcome = if close {
                Outcome::Close
            } else {
                Outcome::Start(Box::new(self.request(env, &derived)))
            };
        }
        outcome
    }

    /// Thumbnail with the cover button; `true` when the button was clicked.
    fn cover_view(&mut self, ui: &mut egui::Ui, env: &Env) -> bool {
        self.cover
            .request(env.project, env.timeline, env.cover_frame);
        self.cover.poll(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(LEFT_WIDTH, BODY_HEIGHT * 0.6),
            egui::Sense::hover(),
        );
        match self.cover.texture() {
            Some(texture) => paint_fitted(ui, rect, Some(texture)),
            None => paint_skeleton(ui, rect),
        }
        let button = egui::Button::new(
            egui::RichText::new(t!("export.edit_cover"))
                .size(theme::FONT_BODY)
                .color(theme::TEXT_PRIMARY),
        )
        .fill(theme::BG_MENU)
        .corner_radius(theme::RADIUS_CONTROL);
        let button_rect = egui::Rect::from_min_size(
            rect.min + egui::vec2(theme::SPACE_2, theme::SPACE_2),
            egui::vec2(140.0, theme::CONTROL_HEIGHT),
        );
        ui.put(button_rect, button).clicked()
    }

    fn form(&mut self, ui: &mut egui::Ui, env: &Env, derived: &Derived) {
        let tl = &env.project.timelines[env.timeline];
        row(ui, t!("export.timeline"), |ui| {
            ui.label(egui::RichText::new(&tl.name).color(theme::TEXT_SECONDARY));
        });
        row(ui, t!("export.name"), |ui| {
            ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(f32::INFINITY));
        });
        row(ui, t!("export.export_to"), |ui| {
            let browse = ui.add_enabled(
                self.browsing.is_none(),
                egui::Button::new(t!("export.browse")),
            );
            if browse.clicked() {
                self.browse();
            }
            ui.add(egui::TextEdit::singleline(&mut self.dir).desired_width(f32::INFINITY));
        });
        if self.output_path().exists() {
            ui.colored_label(theme::WARNING, t!("export.overwrite_warning"));
        }
        if env.empty_material > 0 {
            ui.colored_label(
                theme::WARNING,
                t!("export.empty_material", count = env.empty_material),
            );
        }
        ui.add_space(theme::SPACE_1);
        ui.separator();

        if section_header(
            ui,
            &mut self.video_on,
            &mut self.video_open,
            &t!("export.video"),
        ) {
            self.video_rows(ui, env, derived);
        }
        if self.video_on
            && section_header(
                ui,
                &mut self.blur_on,
                &mut self.blur_open,
                &t!("export.motion_blur"),
            )
        {
            let timeline_fps = env.project.timelines[env.timeline].fps;
            self.blur.rows(ui, timeline_fps, derived.fps);
        }
        if section_header(
            ui,
            &mut self.audio_on,
            &mut self.audio_open,
            &t!("export.audio"),
        ) {
            row(ui, t!("export.format"), |ui| {
                let items: Vec<_> = [
                    AudioFormat::Mp3,
                    AudioFormat::Wav,
                    AudioFormat::Aac,
                    AudioFormat::Flac,
                ]
                .into_iter()
                .map(|f| (f, f.label().to_owned(), f.engine().is_available()))
                .collect();
                preview_combo(
                    ui,
                    "export_audio_format",
                    &mut self.audio_format,
                    &items,
                    None,
                    None,
                    None,
                );
            });
            if !self.video_on {
                ui.weak(t!("export.audio_only_hint"));
            }
        }
        if !vv_core::dub::languages(tl).is_empty() {
            row(ui, t!("dub.export_language"), |ui| {
                crate::dub::ui::language_combo(
                    ui,
                    "export_dub_language",
                    tl,
                    &mut self.language,
                    true,
                );
            });
        }
        if self.caption_track(tl).is_some()
            && section_header(
                ui,
                &mut self.captions_on,
                &mut self.captions_open,
                &t!("export.captions"),
            )
        {
            row(ui, t!("export.format"), |ui| {
                let items: Vec<_> = CaptionFormat::ALL
                    .into_iter()
                    .map(|f| (f, f.label().to_owned(), true))
                    .collect();
                preview_combo(
                    ui,
                    "export_caption_format",
                    &mut self.caption_format,
                    &items,
                    None,
                    None,
                    None,
                );
            });
            ui.weak(match self.caption_format {
                CaptionFormat::Srt => t!("export.captions_srt_hint"),
                CaptionFormat::Txt => t!("export.captions_txt_hint"),
            });
        }
    }

    fn caption_track<'a>(&self, timeline: &'a Timeline) -> Option<&'a Track> {
        let dub = timeline.tracks.iter().find(|track| {
            track.kind == TrackKind::Video
                && track.dub.as_ref().is_some_and(|dub| match &self.language {
                    Language::AsIs => !track.muted,
                    Language::Original => false,
                    Language::Dub(code) => dub.language == *code,
                })
        });
        dub.or_else(|| {
            vv_core::captions::subtitle_track(timeline).map(|index| &timeline.tracks[index])
        })
        .filter(|track| !track.clips.is_empty())
    }

    fn video_rows(&mut self, ui: &mut egui::Ui, env: &Env, derived: &Derived) {
        let project = env.project;
        let tl = &project.timelines[env.timeline];
        let media = export_plan::active_video_media(project, env.timeline);
        let project_short = tl.resolution.0.min(tl.resolution.1);
        let best_short = export_plan::best_short_side(project, &media);

        row(ui, t!("export.resolution"), |ui| {
            let items: Vec<_> = ResolutionChoice::ALL
                .into_iter()
                .map(|choice| {
                    let short = export_plan::target_short_side(choice, project_short, best_short);
                    let (w, h) = export_plan::scaled_size(tl.resolution, short);
                    let label = match choice {
                        ResolutionChoice::P480 => "480p".to_owned(),
                        ResolutionChoice::P720 => "720p".to_owned(),
                        ResolutionChoice::P1080 => "1080p".to_owned(),
                        ResolutionChoice::P2K => "2K".to_owned(),
                        ResolutionChoice::P4K => "4K".to_owned(),
                        ResolutionChoice::MatchBest => t!("export.match_best").into_owned(),
                    };
                    (choice, format!("{label} ({w}×{h})"), true)
                })
                .collect();
            preview_combo(
                ui,
                "export_resolution",
                &mut self.resolution,
                &items,
                None,
                None,
                None,
            );
        });

        row(ui, t!("export.bitrate"), |ui| {
            let items: Vec<_> = BitrateChoice::ALL
                .into_iter()
                .map(|choice| {
                    let label = match choice {
                        BitrateChoice::MatchBest => match self.detected_kbps {
                            Some(kbps) => t!(
                                "export.match_best_value",
                                value = export_plan::format_mbps(f64::from(kbps) / 1000.0)
                            )
                            .into_owned(),
                            None => t!("export.match_best").into_owned(),
                        },
                        BitrateChoice::Recommended => t!("export.bitrate_recommended").into_owned(),
                        BitrateChoice::Lower => t!("export.bitrate_lower").into_owned(),
                        BitrateChoice::Higher => t!("export.bitrate_higher").into_owned(),
                        BitrateChoice::Custom => t!("export.bitrate_custom").into_owned(),
                    };
                    (choice, label, true)
                })
                .collect();
            preview_combo(
                ui,
                "export_bitrate",
                &mut self.bitrate,
                &items,
                None,
                None,
                None,
            );
        });
        if self.bitrate == BitrateChoice::Custom {
            self.custom_rows(ui);
        } else {
            ui.weak(export_plan::format_mbps(derived.mbps));
        }

        row(ui, t!("export.codec"), |ui| {
            let items: Vec<_> = [CodecChoice::H264, CodecChoice::Hevc, CodecChoice::Av1]
                .into_iter()
                .filter(|c| c.available())
                .map(|c| (c, c.label().to_owned(), true))
                .collect();
            preview_combo(
                ui,
                "export_codec",
                &mut self.codec,
                &items,
                None,
                None,
                None,
            );
        });
        row(ui, t!("export.container"), |ui| {
            let items: Vec<_> = Container::ALL
                .into_iter()
                .map(|c| {
                    (
                        c,
                        c.label().to_owned(),
                        export_plan::valid_combination(self.codec, c),
                    )
                })
                .collect();
            preview_combo(
                ui,
                "export_container",
                &mut self.container,
                &items,
                None,
                None,
                Some(&t!("export.invalid_combination")),
            );
        });
        row(ui, t!("export.frame_rate"), |ui| {
            let best = export_plan::best_fps(project, &media);
            let items: Vec<_> = FpsChoice::ALL
                .into_iter()
                .map(|choice| {
                    let label = match choice.fixed() {
                        Some(fps) => format!("{}", fps.num),
                        None => match best {
                            Some(fps) => t!(
                                "export.match_best_value",
                                value = format!("{} fps", trim_fps(fps.as_f64()))
                            )
                            .into_owned(),
                            None => t!("export.match_best").into_owned(),
                        },
                    };
                    (choice, label, true)
                })
                .collect();
            preview_combo(ui, "export_fps", &mut self.fps, &items, None, None, None);
        });
        row(ui, t!("export.color_space"), |ui| {
            ui.label(
                egui::RichText::new(t!("export.color_space_value")).color(theme::TEXT_SECONDARY),
            );
        });
    }

    fn custom_rows(&mut self, ui: &mut egui::Ui) {
        row(ui, "", |ui| {
            let unit = if self.custom_in_kbps { "Kbps" } else { "Mbps" };
            let scale = if self.custom_in_kbps { 1000.0 } else { 1.0 };
            let mut value = self.custom_mbps * scale;
            let max = MAX_CUSTOM_MBPS * scale;
            ui.spacing_mut().slider_width = 110.0;
            ui.add(egui::Slider::new(&mut value, 0.1 * scale..=max).show_value(false));
            ui.add(
                egui::DragValue::new(&mut value)
                    .range(0.1 * scale..=max)
                    .speed(0.1 * scale)
                    .max_decimals(if self.custom_in_kbps { 0 } else { 1 }),
            );
            self.custom_mbps = value / scale;
            if ui.button(unit).clicked() {
                self.custom_in_kbps = !self.custom_in_kbps;
            }
        });
        row(ui, "", |ui| {
            ui.selectable_value(&mut self.constant, true, "CBR");
            ui.selectable_value(&mut self.constant, false, "VBR");
        });
    }

    /// `Some(true)` for cancel, `Some(false)` for export.
    fn footer(&mut self, ui: &mut egui::Ui, env: &Env, derived: &Derived) -> Option<bool> {
        let mut result = None;
        if let Some(fraction) = env.cutout_pending {
            ui.label(
                egui::RichText::new(t!(
                    "export.waits_for_cutout",
                    percent = (fraction * 100.0).round() as u32
                ))
                .color(theme::TEXT_SECONDARY),
            );
        }
        ui.horizontal(|ui| {
            paint_film_icon(ui);
            ui.label(
                egui::RichText::new(t!(
                    "export.summary",
                    duration = export_plan::format_duration(derived.seconds),
                    size = export_plan::format_megabytes(derived.estimate.megabytes())
                ))
                .color(theme::TEXT_SECONDARY),
            );
            info_icon(ui, &t!("export.size_hint", audio = export_plan::AUDIO_KBPS));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let start = egui::Button::new(
                    egui::RichText::new(t!("export.start")).color(theme::ON_ACCENT),
                )
                .fill(theme::ACCENT_FILL)
                .corner_radius(theme::RADIUS_CONTROL);
                if ui.add_enabled(self.can_start(env), start).clicked() {
                    result = Some(false);
                }
                if ui.button(t!("common.cancel")).clicked() {
                    result = Some(true);
                }
            });
        });
        result
    }

    fn running_view(
        &mut self,
        ui: &mut egui::Ui,
        progress: &Arc<Mutex<ExportProgress>>,
    ) -> Outcome {
        let (current, total, done, error, fps, preview) = {
            let p = progress.lock().unwrap();
            let fresh = p
                .preview
                .as_ref()
                .filter(|_| {
                    self.preview
                        .as_ref()
                        .is_none_or(|(v, _)| *v != p.preview_version)
                })
                .map(|frame| (p.preview_version, frame.clone()));
            (
                p.current_frame,
                p.total_frames,
                p.done,
                p.error.as_ref().map(|e| match e {
                    ExportError::Cancelled => None,
                    other => Some(other.to_string()),
                }),
                p.fps(),
                fresh,
            )
        };
        if let Some((version, frame)) = preview {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [frame.width as usize, frame.height as usize],
                &frame.rgba,
            );
            let texture =
                ui.ctx()
                    .load_texture("export_preview", image, egui::TextureOptions::LINEAR);
            self.preview = Some((version, texture));
        }
        // Cancelled: the session already removed the partial files.
        if matches!(error, Some(None)) {
            return Outcome::Close;
        }

        let rect_size = egui::vec2(MODAL_WIDTH, 300.0);
        let (rect, _) = ui.allocate_exact_size(rect_size, egui::Sense::hover());
        paint_fitted(ui, rect, self.preview.as_ref().map(|(_, t)| t));
        ui.add_space(theme::SPACE_3);

        let fraction = if total > 0 {
            (current as f32 / total as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        ui.add(
            egui::ProgressBar::new(fraction)
                .text(format!("{:.0}%", fraction * 100.0))
                .animate(!done),
        );
        let mut outcome = Outcome::None;
        match &error {
            Some(Some(message)) => {
                ui.colored_label(theme::ERROR, format!("✖ {message}"));
            }
            _ if done => {
                ui.colored_label(theme::SUCCESS, format!("✔ {}", t!("export.done")));
            }
            _ => {
                if let Some(fps) = fps {
                    let remaining = ((total - current).max(0) as f64 / fps).round();
                    ui.weak(t!(
                        "export.remaining",
                        time = export_plan::format_duration(remaining)
                    ));
                } else {
                    ui.weak(t!("export.starting"));
                }
            }
        }
        ui.add_space(theme::SPACE_2);
        let finished = done || matches!(error, Some(Some(_)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if finished {
                if ui.button(t!("common.close")).clicked() {
                    outcome = Outcome::Close;
                }
                if done
                    && let Some(dir) = &self.started_dir
                    && ui.button(t!("export.open_folder")).clicked()
                {
                    open_folder(dir);
                }
            } else if ui.button(t!("common.cancel")).clicked() {
                outcome = Outcome::CancelRender;
            }
        });
        if !finished {
            ui.ctx().request_repaint();
        }
        outcome
    }
}

/// The encoder a GPU offers, else x264.
fn hardware_encoder() -> VideoCodec {
    [VideoCodec::Nvenc, VideoCodec::VideoToolbox]
        .into_iter()
        .find(|c| VideoCodec::choices().contains(c) && c.availability() == Some(true))
        .unwrap_or(VideoCodec::X264)
}

/// What the motion blur section edits: text fields stay as typed, the
/// settings are parsed from them.
struct MotionBlurForm {
    values: MotionBlurSettings,
    /// Index in `Weighting::PRESETS`, or past it for custom.
    weighting: usize,
    custom_weights: String,
    interpolate: bool,
    interpolated_fps: String,
    deduplicate: bool,
}

impl Default for MotionBlurForm {
    fn default() -> Self {
        let values = MotionBlurSettings::default();
        Self {
            interpolate: values.interpolation.is_some(),
            interpolated_fps: values
                .interpolation
                .unwrap_or(FpsTarget::Fixed(600))
                .to_string(),
            deduplicate: values.deduplicate.is_some(),
            weighting: 0,
            custom_weights: "1, 2, 3, 2, 1".to_owned(),
            values,
        }
    }
}

const CUSTOM_WEIGHTING: usize = Weighting::PRESETS.len();

fn weighting_label(weighting: &Weighting) -> std::borrow::Cow<'static, str> {
    match weighting {
        Weighting::Equal => t!("export.weighting_equal"),
        Weighting::GaussianSym => t!("export.weighting_gaussian_sym"),
        Weighting::Vegas => t!("export.weighting_vegas"),
        Weighting::Pyramid => t!("export.weighting_pyramid"),
        Weighting::Gaussian => t!("export.weighting_gaussian"),
        Weighting::Ascending => t!("export.weighting_ascending"),
        Weighting::Descending => t!("export.weighting_descending"),
        Weighting::GaussianReverse => t!("export.weighting_gaussian_reverse"),
        Weighting::Custom(_) => t!("export.weighting_custom"),
    }
}

impl MotionBlurForm {
    /// `None` while a text field does not parse.
    fn settings(&self) -> Option<MotionBlurSettings> {
        let mut settings = self.values.clone();
        settings.weighting = match Weighting::PRESETS.get(self.weighting) {
            Some(preset) => preset.clone(),
            None => Weighting::parse_custom(&self.custom_weights)?,
        };
        settings.interpolation = if self.interpolate {
            Some(FpsTarget::parse(&self.interpolated_fps)?)
        } else {
            None
        };
        settings.deduplicate = self
            .deduplicate
            .then(|| self.values.deduplicate.unwrap_or_default());
        Some(settings)
    }

    fn rows(
        &mut self,
        ui: &mut egui::Ui,
        timeline_fps: vv_core::Rational,
        output_fps: vv_core::Rational,
    ) {
        row(ui, t!("export.blur_amount"), |ui| {
            ui.spacing_mut().slider_width = 110.0;
            ui.add(egui::Slider::new(&mut self.values.amount, 0.0..=2.0).show_value(false));
            ui.add(
                egui::DragValue::new(&mut self.values.amount)
                    .range(0.0..=10.0)
                    .speed(0.01)
                    .max_decimals(2),
            );
            info_icon(ui, &t!("export.blur_amount_hint"));
        });

        row(ui, t!("export.weighting"), |ui| {
            let mut items: Vec<_> = Weighting::PRESETS
                .iter()
                .enumerate()
                .map(|(i, w)| (i, weighting_label(w).into_owned(), true))
                .collect();
            items.push((
                CUSTOM_WEIGHTING,
                t!("export.weighting_custom").into_owned(),
                true,
            ));
            preview_combo(
                ui,
                "export_blur_weighting",
                &mut self.weighting,
                &items,
                None,
                None,
                None,
            );
        });
        if self.weighting == CUSTOM_WEIGHTING {
            row(ui, t!("export.custom_weights"), |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.custom_weights).desired_width(160.0));
                info_icon(ui, &t!("export.custom_weights_hint"));
            });
            if Weighting::parse_custom(&self.custom_weights).is_none() {
                ui.colored_label(theme::WARNING, t!("export.invalid_weights"));
            }
        }
        if Weighting::PRESETS
            .get(self.weighting)
            .is_some_and(Weighting::uses_gaussian)
        {
            let gaussian = &mut self.values.gaussian;
            let reversed_or_plain = Weighting::PRESETS[self.weighting] != Weighting::GaussianSym;
            if reversed_or_plain {
                row(ui, t!("export.gaussian_mean"), |ui| {
                    ui.add(egui::DragValue::new(&mut gaussian.mean).speed(0.05));
                });
            }
            row(ui, t!("export.gaussian_std_dev"), |ui| {
                ui.add(
                    egui::DragValue::new(&mut gaussian.std_dev)
                        .range(0.01..=100.0)
                        .speed(0.05),
                );
            });
            row(ui, t!("export.gaussian_bound"), |ui| {
                ui.add(egui::DragValue::new(&mut gaussian.bound.0).speed(0.05));
                ui.add(egui::DragValue::new(&mut gaussian.bound.1).speed(0.05));
            });
        }
        row(ui, t!("export.blur_gamma"), |ui| {
            ui.add(
                egui::DragValue::new(&mut self.values.gamma)
                    .range(0.1..=10.0)
                    .speed(0.01)
                    .max_decimals(2),
            );
            info_icon(ui, &t!("export.blur_gamma_hint"));
        });

        row(ui, "", |ui| {
            ui.checkbox(&mut self.interpolate, t!("export.interpolate"));
            ui.add_enabled(
                self.interpolate,
                egui::TextEdit::singleline(&mut self.interpolated_fps).desired_width(60.0),
            );
            ui.label(egui::RichText::new("fps").color(theme::TEXT_SECONDARY));
            info_icon(ui, &t!("export.interpolated_fps_hint"));
        });
        if self.interpolate && FpsTarget::parse(&self.interpolated_fps).is_none() {
            ui.colored_label(theme::WARNING, t!("export.invalid_fps"));
        }

        row(ui, "", |ui| {
            ui.checkbox(&mut self.deduplicate, t!("export.deduplicate"));
            info_icon(ui, &t!("export.deduplicate_hint"));
        });
        if self.deduplicate {
            let dedup = self
                .values
                .deduplicate
                .get_or_insert_with(Deduplicate::default);
            row(ui, t!("export.dedup_range"), |ui| {
                ui.add(egui::DragValue::new(&mut dedup.range).range(1..=30));
            });
            row(ui, t!("export.dedup_threshold"), |ui| {
                ui.add(
                    egui::DragValue::new(&mut dedup.threshold)
                        .range(0.0..=0.5)
                        .speed(0.0001)
                        .max_decimals(4),
                );
            });
        }

        if let Some(settings) = self.settings() {
            let (fps, frames) = blend_summary(&settings, timeline_fps, output_fps);
            if frames > 1 {
                ui.weak(t!(
                    "export.blur_summary",
                    frames = frames,
                    fps = trim_fps(fps)
                ));
            } else {
                ui.colored_label(theme::WARNING, t!("export.blur_none"));
            }
        }
    }
}

fn trim_fps(fps: f64) -> String {
    if (fps - fps.round()).abs() < 0.005 {
        format!("{fps:.0}")
    } else {
        format!("{fps:.2}")
    }
}

fn row(ui: &mut egui::Ui, label: impl Into<String>, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(LABEL_WIDTH, theme::FIELD_HEIGHT),
            egui::Sense::hover(),
        );
        ui.painter().text(
            rect.left_center(),
            egui::Align2::LEFT_CENTER,
            label.into(),
            egui::FontId::proportional(theme::FONT_BODY),
            theme::TEXT_SECONDARY,
        );
        add(ui);
    });
}

/// Arrow + checkbox; `true` when expanded, even if disabled, so values
/// can be set before turning the section on.
fn section_header(ui: &mut egui::Ui, on: &mut bool, open: &mut bool, title: &str) -> bool {
    ui.add_space(theme::SPACE_1);
    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(theme::ICON_SIZE, theme::ICON_SIZE),
            egui::Sense::click(),
        );
        let c = rect.center();
        let points = if *open {
            vec![
                c + egui::vec2(-4.0, -2.0),
                c + egui::vec2(4.0, -2.0),
                c + egui::vec2(0.0, 3.0),
            ]
        } else {
            vec![
                c + egui::vec2(-2.0, -4.0),
                c + egui::vec2(3.0, 0.0),
                c + egui::vec2(-2.0, 4.0),
            ]
        };
        ui.painter().add(egui::Shape::convex_polygon(
            points,
            theme::TEXT_SECONDARY,
            egui::Stroke::NONE,
        ));
        if response.clicked() {
            *open = !*open;
        }
        ui.checkbox(
            on,
            egui::RichText::new(title)
                .size(theme::FONT_SECTION)
                .color(theme::TEXT_PRIMARY),
        );
    });
    *open
}

fn paint_film_icon(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(theme::ICON_SIZE, theme::ICON_SIZE),
        egui::Sense::hover(),
    );
    let stroke = egui::Stroke::new(theme::ICON_STROKE, theme::TEXT_SECONDARY);
    let body = rect.shrink2(egui::vec2(1.0, 2.0));
    let painter = ui.painter();
    painter.rect_stroke(body, theme::RADIUS_TAG, stroke, egui::StrokeKind::Inside);
    for i in 0..3 {
        let y = body.top() + 2.5 + i as f32 * 3.5;
        painter.circle_filled(egui::pos2(body.left() + 2.0, y), 0.8, theme::TEXT_SECONDARY);
        painter.circle_filled(
            egui::pos2(body.right() - 2.0, y),
            0.8,
            theme::TEXT_SECONDARY,
        );
    }
}

fn info_icon(ui: &mut egui::Ui, hint: &str) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(theme::ICON_SIZE, theme::ICON_SIZE),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    painter.circle_stroke(
        rect.center(),
        7.0,
        egui::Stroke::new(theme::ICON_STROKE, theme::TEXT_SECONDARY),
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "i",
        egui::FontId::proportional(theme::FONT_SMALL),
        theme::TEXT_SECONDARY,
    );
    response.on_hover_text(hint);
}

fn open_folder(dir: &Path) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(program).arg(dir).spawn();
}

impl VenturiApp {
    pub(crate) fn open_export_modal(&mut self) {
        let Some(timeline) = self.timeline_id else {
            return;
        };
        if self.export_modal.is_some() || self.export.is_some() {
            return;
        }
        let dir = self
            .settings
            .last_export_dir
            .clone()
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(export_plan::default_export_dir);
        self.export_modal = Some(ExportModal::new(
            &self.session.project,
            timeline,
            &self.project_label(),
            dir,
        ));
    }

    pub(crate) fn show_export_modal(&mut self, ctx: &egui::Context) {
        let (Some(mut modal), Some(timeline)) = (self.export_modal.take(), self.timeline_id) else {
            return;
        };
        if !self.session.project.timelines.contains_key(timeline) {
            return;
        }
        let total = self.session.project.timelines[timeline].total_frames();
        let marks = &self.timeline_state.export_marks;
        let range = if marks.is_full(total) {
            0..total
        } else {
            let (start, end) = marks.resolve(total);
            start..end
        };
        let outcome = {
            let env = Env {
                project: &self.session.project,
                timeline,
                cover_frame: self.cover_frame(timeline),
                range,
                waits_for_scan: self.session.export_waits_for_scan(timeline, true),
                cutout_pending: self.cutout_pending(timeline),
                empty_material: self.ai_empty_material(timeline),
                running: self.export.as_ref().map(|state| &state.progress),
            };
            modal.show(ctx, &env)
        };
        match outcome {
            Outcome::None => self.export_modal = Some(modal),
            Outcome::EditCover => {
                self.open_cover_picker();
                self.export_modal = Some(modal);
            }
            Outcome::CancelRender => {
                if let Some(state) = &self.export {
                    self.session.cancel_export(state.job);
                }
                self.export_modal = Some(modal);
            }
            Outcome::Close => {
                self.export = None;
            }
            Outcome::Start(request) => {
                modal.started_dir = request.settings.output_path.parent().map(Path::to_path_buf);
                self.start_modal_export(timeline, *request);
                self.export_modal = Some(modal);
            }
        }
    }

    fn start_modal_export(&mut self, timeline: TimelineId, request: StartRequest) {
        let StartRequest {
            settings,
            range,
            captions,
        } = request;
        if let Some(dir) = settings.output_path.parent() {
            let _ = std::fs::create_dir_all(dir);
            if self.settings.last_export_dir.as_deref() != Some(dir) {
                self.settings.last_export_dir = Some(dir.to_path_buf());
                self.persist_settings();
            }
        }
        if let Some((path, text)) = captions {
            let _ = std::fs::write(path, text);
        }
        self.last_export_settings = Some(settings.clone());
        self.run_export(timeline, settings, range);
    }
}
