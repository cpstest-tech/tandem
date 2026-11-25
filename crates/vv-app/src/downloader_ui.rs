//! The downloader in the editor: a link bar on top of Music, SFX and the
//! project media, the options dialog (quality, destination, section) and
//! the running jobs. Finished files are imported where they were asked for.

use super::*;
use crate::downloader::source::{self, Link, Site};
use crate::downloader::{
    self, AudioFormat, Destination, Downloader, HEIGHTS, MediaKind, Phase, Probed, Request, ytdlp,
};
use crate::library_ribbon::{Category, RibbonTab};
use std::borrow::Cow;

const DIALOG_WIDTH: f32 = 560.0;
const THUMB_SIZE: egui::Vec2 = egui::vec2(192.0, 108.0);
const RANGE_HEIGHT: f32 = 64.0;
const LABEL_WIDTH: f32 = 110.0;
const BROWSERS: [&str; 6] = ["", "firefox", "chrome", "chromium", "brave", "edge"];

#[derive(Default)]
pub(crate) struct DownloadState {
    downloader: Option<Downloader>,
    bar_text: String,
    dialog: Option<Dialog>,
}

struct Dialog {
    link: Link,
    destination: Destination,
    kind: MediaKind,
    prefs: downloader::Prefs,
    probe: Option<downloader::Probe>,
    probed: Option<Result<Probed, String>>,
    texture: Option<egui::TextureHandle>,
    use_range: bool,
    range: (f64, f64),
    range_text: (String, String),
}

impl Dialog {
    fn duration(&self) -> Option<f64> {
        match &self.probed {
            Some(Ok(Probed::Media { info, .. })) => info.duration,
            Some(Ok(Probed::Spotify(c))) if c.tracks.len() == 1 => Some(c.tracks[0].duration),
            _ => None,
        }
    }

    /// Music and SFX hold audio only.
    fn kind(&self) -> MediaKind {
        if self.destination == Destination::Project && self.link.site != Site::Spotify {
            self.kind
        } else {
            MediaKind::Audio
        }
    }

    fn set_range(&mut self, start: f64, end: f64) {
        self.range = (start, end);
        self.range_text = (format_time(start), format_time(end));
    }
}

fn destination_label(destination: Destination) -> Cow<'static, str> {
    match destination {
        Destination::Music => t!("ribbon.audio_music"),
        Destination::Sfx => t!("ribbon.audio_sfx"),
        Destination::Project => t!("downloader.to_project"),
    }
}

fn audio_format_label(format: AudioFormat) -> Cow<'static, str> {
    match format {
        AudioFormat::Original => t!("downloader.format_original"),
        AudioFormat::Wav => "WAV".into(),
        AudioFormat::Flac => "FLAC".into(),
        AudioFormat::Mp3 => "MP3 (V0)".into(),
    }
}

/// `m:ss.s`, or `h:mm:ss.s` past the hour.
pub(crate) fn format_time(seconds: f64) -> String {
    let tenths = (seconds.max(0.0) * 10.0).round() as u64;
    let (h, m, s, d) = (
        tenths / 36000,
        tenths / 600 % 60,
        tenths / 10 % 60,
        tenths % 10,
    );
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{d}")
    } else {
        format!("{m}:{s:02}.{d}")
    }
}

/// Seconds, `m:ss(.s)` or `h:mm:ss(.s)`.
pub(crate) fn parse_time(text: &str) -> Option<f64> {
    let mut total = 0.0;
    let parts: Vec<&str> = text.trim().split(':').collect();
    if parts.len() > 3 {
        return None;
    }
    for (i, part) in parts.iter().enumerate() {
        let value: f64 = part.trim().parse().ok()?;
        let last = i + 1 == parts.len();
        if value < 0.0 || (!last && value.fract() != 0.0) || (i > 0 && value >= 60.0) {
            return None;
        }
        total = total * 60.0 + value;
    }
    Some(total)
}

fn format_speed(bytes_per_second: f64) -> String {
    if bytes_per_second >= 1e6 {
        format!("{:.1} MB/s", bytes_per_second / 1e6)
    } else {
        format!("{:.0} kB/s", bytes_per_second / 1e3)
    }
}

fn primary_button(ui: &mut egui::Ui, text: impl Into<String>, enabled: bool) -> egui::Response {
    let button =
        crate::ui_kit::primary_button(text.into()).min_size(egui::vec2(0.0, theme::FIELD_HEIGHT));
    ui.add_enabled(enabled, button)
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

impl RibbonTab {
    pub(crate) fn download_destination(self, category: Category) -> Option<Destination> {
        match (self, category) {
            (RibbonTab::Media, Category::Project) => Some(Destination::Project),
            (RibbonTab::Audio, Category::Music) => Some(Destination::Music),
            (RibbonTab::Audio, Category::Sfx) => Some(Destination::Sfx),
            _ => None,
        }
    }
}

impl VenturiApp {
    fn downloader(&mut self, ctx: &egui::Context) -> &mut Downloader {
        self.downloads
            .downloader
            .get_or_insert_with(|| Downloader::new(ctx.clone(), downloader::Dirs::default_dirs()))
    }

    fn download_env(&self, cookies_browser: &str) -> ytdlp::Env {
        let node = Some(self.settings.ai_runtime.node.clone())
            .filter(|node| node.is_file())
            .or_else(|| ytdlp::on_path(if cfg!(windows) { "node.exe" } else { "node" }));
        ytdlp::Env {
            ffmpeg: ytdlp::bundled_ffmpeg_dir(),
            node,
            cookies_browser: Some(cookies_browser.to_owned()).filter(|b| !b.is_empty()),
        }
    }

    /// Where a link pasted anywhere goes: the audio category on screen, else
    /// Music for Spotify and the project for the rest.
    fn default_download_destination(&self, site: Site) -> Destination {
        if self.block_shown(Block::Library) && self.library_ribbon.tab == RibbonTab::Audio {
            if self.library_ribbon.category() == Category::Sfx {
                return Destination::Sfx;
            }
            return Destination::Music;
        }
        if site == Site::Spotify {
            Destination::Music
        } else {
            Destination::Project
        }
    }

    /// A pasted link to a known site opens the dialog instead of pasting.
    pub(crate) fn paste_download_link(&mut self, ctx: &egui::Context, text: &str) -> bool {
        let Some(link) = source::parse_known(text) else {
            return false;
        };
        let destination = self.default_download_destination(link.site);
        self.open_download_dialog(ctx, link, destination);
        true
    }

    fn open_download_dialog(&mut self, ctx: &egui::Context, link: Link, destination: Destination) {
        let prefs = self.settings.download.clone();
        let kind = if destination == Destination::Project && link.site != Site::Spotify {
            MediaKind::Video
        } else {
            MediaKind::Audio
        };
        let env = self.download_env(&prefs.cookies_browser);
        let probe = self.downloader(ctx).probe(link.clone(), env);
        self.downloads.dialog = Some(Dialog {
            link,
            destination,
            kind,
            prefs,
            probe: Some(probe),
            probed: None,
            texture: None,
            use_range: false,
            range: (0.0, 0.0),
            range_text: Default::default(),
        });
    }

    /// The link field on top of Music, SFX and the project media, with the
    /// jobs headed there below it.
    pub(crate) fn show_download_bar(&mut self, ui: &mut egui::Ui, destination: Destination) {
        let mut submit = false;
        egui::Frame::new()
            .fill(theme::BG_INSET)
            .stroke(egui::Stroke::new(1.0, theme::BORDER_SUBTLE))
            .corner_radius(theme::RADIUS_CONTROL)
            .inner_margin(egui::Margin::symmetric(6, 3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("⬇").color(theme::ACCENT));
                    let button_width = 96.0;
                    let edit = egui::TextEdit::singleline(&mut self.downloads.bar_text)
                        .hint_text(t!("downloader.bar_hint"))
                        .frame(egui::Frame::NONE)
                        .desired_width(ui.available_width() - button_width);
                    let response = ui.add(edit);
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        submit = true;
                    }
                    let ready = source::parse(&self.downloads.bar_text).is_some();
                    if primary_button(ui, t!("downloader.get"), ready)
                        .on_hover_text(t!("downloader.get_hint"))
                        .clicked()
                    {
                        submit = true;
                    }
                });
            });
        if submit {
            match source::parse(&self.downloads.bar_text) {
                Some(link) => {
                    self.downloads.bar_text.clear();
                    self.open_download_dialog(&ui.ctx().clone(), link, destination);
                }
                None => self.toast(t!("downloader.not_a_link")),
            }
        }
        let group = |d: Destination| match destination {
            Destination::Project => d == Destination::Project,
            _ => d != Destination::Project,
        };
        self.show_download_jobs(ui, group);
        ui.add_space(theme::SPACE_1);
    }

    fn show_download_jobs(&mut self, ui: &mut egui::Ui, show: impl Fn(Destination) -> bool) {
        let Some(downloader) = &self.downloads.downloader else {
            return;
        };
        let jobs: Vec<_> = downloader
            .jobs()
            .into_iter()
            .filter(|j| show(j.request.destination))
            .collect();
        let mut cancel = None;
        let mut dismiss = None;
        let mut retry = None;
        for job in &jobs {
            ui.add_space(theme::SPACE_1);
            ui.horizontal(|ui| {
                let close = ui
                    .add(egui::Button::new("✕").small().frame(false))
                    .on_hover_text(if job.is_active() {
                        t!("downloader.cancel")
                    } else {
                        t!("downloader.dismiss")
                    });
                if close.clicked() {
                    if job.is_active() {
                        cancel = Some(job.id);
                    } else {
                        dismiss = Some(job.id);
                    }
                }
                let where_to = destination_label(job.request.destination);
                let title = format!("{} → {where_to}", job.request.title);
                match &job.phase {
                    Phase::Failed(error) => {
                        if ui.small_button(t!("downloader.retry")).clicked() {
                            retry = Some(job.id);
                        }
                        ui.colored_label(theme::ERROR, format!("{title}: {error}"))
                            .on_hover_text(error);
                    }
                    phase => {
                        let (fraction, detail) = match phase {
                            Phase::Waiting => (None, t!("downloader.waiting").into_owned()),
                            Phase::Preparing => (None, t!("downloader.preparing").into_owned()),
                            Phase::Processing => (None, t!("downloader.processing").into_owned()),
                            Phase::Downloading {
                                fraction,
                                speed,
                                eta,
                            } => {
                                let mut parts = Vec::new();
                                if let Some(f) = fraction {
                                    parts.push(format!("{:.0}%", f * 100.0));
                                }
                                if let Some(speed) = speed {
                                    parts.push(format_speed(*speed));
                                }
                                if let Some(eta) = eta {
                                    parts.push(format_time(*eta));
                                }
                                (*fraction, parts.join(" · "))
                            }
                            Phase::Failed(_) => unreachable!(),
                        };
                        let mut bar = egui::ProgressBar::new(fraction.unwrap_or(0.0))
                            .desired_width(ui.available_width())
                            .desired_height(18.0)
                            .fill(theme::ACCENT_FILL)
                            .text(format!("{title}  {detail}"));
                        if fraction.is_none() {
                            bar = bar.animate(true);
                        }
                        ui.add(bar);
                    }
                }
            });
        }
        if let Some(id) = cancel {
            downloader.cancel(id);
        }
        if let Some(id) = dismiss {
            downloader.dismiss(id);
        }
        if let Some(id) = retry {
            let cookies = self.settings.download.cookies_browser.clone();
            let env = self.download_env(&cookies);
            if let Some(downloader) = &mut self.downloads.downloader {
                downloader.retry(id, env);
            }
        }
    }

    /// Next to the other background activity of the toolbar; a click shows
    /// the jobs in the Audio tab.
    pub(crate) fn show_download_indicator(&mut self, ui: &mut egui::Ui) {
        let Some(downloader) = &self.downloads.downloader else {
            return;
        };
        let jobs = downloader.jobs();
        let active: Vec<_> = jobs.iter().filter(|j| j.is_active()).collect();
        if active.is_empty() {
            return;
        }
        let fractions: Vec<f32> = active
            .iter()
            .filter_map(|j| match j.phase {
                Phase::Downloading { fraction, .. } => fraction,
                _ => None,
            })
            .collect();
        ui.separator();
        ui.spinner();
        let mut text = t!("downloader.active", count = active.len()).into_owned();
        if !fractions.is_empty() {
            let mean = fractions.iter().sum::<f32>() / fractions.len() as f32;
            text.push_str(&format!(" · {:.0}%", mean * 100.0));
        }
        let first = active[0].request.destination;
        if ui.link(text).clicked() {
            let (tab, category) = match first {
                Destination::Project => (RibbonTab::Media, Category::Project),
                Destination::Music => (RibbonTab::Audio, Category::Music),
                Destination::Sfx => (RibbonTab::Audio, Category::Sfx),
            };
            self.library_ribbon.show(tab, category);
            self.reveal_block(Block::Library);
        }
    }

    /// Finished jobs: imported, announced with a toast and, with the window
    /// in the background, a system notification.
    pub(crate) fn poll_downloads(&mut self, ctx: &egui::Context) {
        let Some(downloader) = &self.downloads.downloader else {
            return;
        };
        let finished = downloader.poll();
        let focused = ctx.input(|i| i.focused);
        for done in finished {
            let title = done.request.title.clone();
            let paths = match done.result {
                Ok(paths) => paths,
                Err(error) if error == downloader::CANCELLED => continue,
                Err(error) => {
                    let message = t!("downloader.failed", title = title, error = error);
                    if !focused {
                        downloader::notify_desktop("Tandem", &message);
                    }
                    self.toast(message);
                    continue;
                }
            };
            let destination = done.request.destination;
            let message = match destination {
                Destination::Music | Destination::Sfx => {
                    let category = if destination == Destination::Music {
                        Category::Music
                    } else {
                        Category::Sfx
                    };
                    self.import_into_category(paths, category);
                    t!(
                        "downloader.done",
                        title = title,
                        place = destination_label(destination)
                    )
                }
                Destination::Project if done.request.epoch == self.session.epoch() => {
                    self.import_media_files(paths);
                    t!(
                        "downloader.done",
                        title = title,
                        place = destination_label(destination)
                    )
                }
                Destination::Project => {
                    let folder = paths
                        .first()
                        .and_then(|p| p.parent())
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    t!("downloader.done_elsewhere", title = title, folder = folder)
                }
            };
            if !focused {
                downloader::notify_desktop(&t!("downloader.done_title"), &message);
            }
            self.toast(message);
        }
    }

    pub(crate) fn show_download_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.downloads.dialog else {
            return;
        };
        if let Some(result) = dialog.probe.as_ref().and_then(|p| p.poll()) {
            dialog.probe = None;
            if let Ok(Probed::Media {
                thumbnail: Some(thumb),
                ..
            }) = &result
            {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [thumb.width as usize, thumb.height as usize],
                    &thumb.rgba,
                );
                dialog.texture =
                    Some(ctx.load_texture("download_thumb", image, egui::TextureOptions::LINEAR));
            }
            dialog.probed = Some(result);
            if let Some(duration) = dialog.duration() {
                dialog.set_range(0.0, duration);
            }
        }
        let mut close = false;
        let mut start = false;
        let modal = egui::Modal::new(egui::Id::new("download_dialog")).show(ctx, |ui| {
            ui.set_width(DIALOG_WIDTH);
            crate::ui_kit::heading(ui, t!("downloader.title"));
            ui.add_space(theme::SPACE_3);
            dialog_header(ui, dialog);
            ui.add_space(theme::SPACE_3);
            let ready = matches!(dialog.probed, Some(Ok(_)));
            if ready {
                dialog_form(ui, dialog);
            }
            ui.add_space(theme::SPACE_2);
            ui.separator();
            ui.add_space(theme::SPACE_1);
            ui.horizontal(|ui| {
                ui.weak(t!("downloader.rights_hint"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = match &dialog.probed {
                        Some(Ok(Probed::Spotify(c))) if c.tracks.len() > 1 => {
                            t!("downloader.download_n", count = c.tracks.len())
                        }
                        _ => t!("downloader.download"),
                    };
                    let valid_range = !dialog.use_range || dialog.range.1 > dialog.range.0;
                    if primary_button(ui, label, ready && valid_range).clicked() {
                        start = true;
                    }
                    if ui.button(t!("downloader.close")).clicked() {
                        close = true;
                    }
                });
            });
        });
        if start {
            self.start_downloads();
        } else if close || modal.should_close() {
            self.downloads.dialog = None;
        }
    }

    fn start_downloads(&mut self) {
        let Some(dialog) = self.downloads.dialog.take() else {
            return;
        };
        let epoch = self.session.epoch();
        let range = dialog
            .use_range
            .then_some(dialog.range)
            .filter(|(start, end)| end > start);
        let base = Request {
            url: dialog.link.url.clone(),
            title: String::new(),
            kind: dialog.kind(),
            audio_format: dialog.prefs.audio_format,
            max_height: dialog.prefs.max_height,
            prefer_h264: dialog.prefs.prefer_h264,
            range,
            destination: dialog.destination,
            spotify: None,
            epoch,
        };
        let requests: Vec<Request> = match dialog.probed {
            Some(Ok(Probed::Spotify(collection))) => {
                let single = collection.tracks.len() == 1;
                collection
                    .tracks
                    .into_iter()
                    .map(|track| Request {
                        title: track.label(),
                        range: if single { base.range } else { None },
                        kind: MediaKind::Audio,
                        spotify: Some(track),
                        ..base.clone()
                    })
                    .collect()
            }
            Some(Ok(Probed::Media { info, .. })) => vec![Request {
                title: if info.title.is_empty() {
                    dialog.link.url.clone()
                } else {
                    info.title
                },
                ..base
            }],
            _ => return,
        };
        let env = self.download_env(&dialog.prefs.cookies_browser);
        self.settings.download = dialog.prefs;
        self.persist_settings();
        let count = requests.len();
        let ctx = self.downloads_ctx();
        let downloader = self.downloader(&ctx);
        for request in requests {
            downloader.enqueue(request, env.clone());
        }
        self.toast(t!("downloader.started", count = count));
    }

    fn downloads_ctx(&self) -> egui::Context {
        self.downloads
            .downloader
            .as_ref()
            .map(|d| d.ctx())
            .unwrap_or_default()
    }
}

fn dialog_header(ui: &mut egui::Ui, dialog: &Dialog) {
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(THUMB_SIZE, egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, theme::RADIUS_CONTROL, theme::BG_INSET);
        match &dialog.texture {
            Some(texture) => {
                let size = texture.size_vec2();
                let scale = (rect.width() / size.x).min(rect.height() / size.y);
                let fitted = egui::Rect::from_center_size(rect.center(), size * scale);
                painter.image(
                    texture.id(),
                    fitted,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None => {
                let glyph = match dialog.link.site {
                    Site::Spotify => "♫",
                    _ => "▶",
                };
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    glyph,
                    egui::FontId::proportional(32.0),
                    theme::TEXT_DISABLED,
                );
            }
        }
        ui.add_space(theme::SPACE_3);
        ui.vertical(|ui| {
            ui.set_width(DIALOG_WIDTH - THUMB_SIZE.x - theme::SPACE_3);
            let site = egui::RichText::new(dialog.link.site.label())
                .size(theme::FONT_SMALL)
                .color(theme::ACCENT);
            ui.label(site);
            match &dialog.probed {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(t!("downloader.probing"));
                    });
                    ui.weak(&dialog.link.url);
                }
                Some(Err(error)) => {
                    ui.colored_label(theme::ERROR, error);
                    ui.weak(&dialog.link.url);
                }
                Some(Ok(Probed::Media { info, .. })) => {
                    ui.label(crate::ui_kit::section_title(&info.title));
                    let mut meta = vec![info.uploader.clone()];
                    if let Some(duration) = info.duration {
                        meta.push(format_time(duration));
                    }
                    if info.items > 1 {
                        meta.push(t!("downloader.items", count = info.items).into_owned());
                    }
                    ui.weak(
                        meta.into_iter()
                            .filter(|m| !m.is_empty())
                            .collect::<Vec<_>>()
                            .join(" · "),
                    );
                }
                Some(Ok(Probed::Spotify(collection))) => {
                    let single = collection.tracks.len() == 1;
                    let heading = if single {
                        collection.tracks[0].label()
                    } else {
                        collection.name.clone()
                    };
                    ui.label(crate::ui_kit::section_title(heading));
                    if !single {
                        ui.weak(t!("downloader.tracks", count = collection.tracks.len()));
                    }
                    ui.weak(t!("downloader.spotify_hint"));
                }
            }
        });
    });
}

fn dialog_form(ui: &mut egui::Ui, dialog: &mut Dialog) {
    let spotify = dialog.link.site == Site::Spotify;
    row(ui, t!("downloader.save_to"), |ui| {
        for destination in [Destination::Music, Destination::Sfx, Destination::Project] {
            if ui
                .selectable_label(
                    dialog.destination == destination,
                    destination_label(destination),
                )
                .clicked()
            {
                dialog.destination = destination;
            }
        }
    });
    if !spotify && dialog.destination == Destination::Project {
        row(ui, t!("downloader.kind"), |ui| {
            ui.selectable_value(&mut dialog.kind, MediaKind::Audio, t!("downloader.audio"));
            ui.selectable_value(&mut dialog.kind, MediaKind::Video, t!("downloader.video"));
        });
    }
    let kind = dialog.kind();
    let prefs = &mut dialog.prefs;
    if kind == MediaKind::Audio {
        row(ui, t!("downloader.audio_format"), |ui| {
            egui::ComboBox::from_id_salt("download_audio_format")
                .selected_text(audio_format_label(prefs.audio_format))
                .show_ui(ui, |ui| {
                    for format in AudioFormat::ALL {
                        ui.selectable_value(
                            &mut prefs.audio_format,
                            format,
                            audio_format_label(format),
                        );
                    }
                });
        });
    } else {
        row(ui, t!("downloader.resolution"), |ui| {
            egui::ComboBox::from_id_salt("download_height")
                .selected_text(format!("{}p", prefs.max_height))
                .show_ui(ui, |ui| {
                    for height in HEIGHTS {
                        ui.selectable_value(&mut prefs.max_height, height, format!("{height}p"));
                    }
                });
            ui.checkbox(&mut prefs.prefer_h264, "H.264")
                .on_hover_text(t!("downloader.h264_hint"));
        });
    }
    if dialog.link.site == Site::Instagram || !prefs.cookies_browser.is_empty() {
        row(ui, t!("downloader.cookies"), |ui| {
            let label = |b: &str| {
                if b.is_empty() {
                    t!("downloader.cookies_none").into_owned()
                } else {
                    b.to_owned()
                }
            };
            egui::ComboBox::from_id_salt("download_cookies")
                .selected_text(label(&prefs.cookies_browser))
                .show_ui(ui, |ui| {
                    for browser in BROWSERS {
                        ui.selectable_value(
                            &mut prefs.cookies_browser,
                            browser.to_owned(),
                            label(browser),
                        );
                    }
                })
                .response
                .on_hover_text(t!("downloader.cookies_hint"));
        });
    }
    let Some(duration) = dialog.duration() else {
        return;
    };
    ui.add_space(theme::SPACE_2);
    ui.checkbox(&mut dialog.use_range, t!("downloader.only_section"));
    if !dialog.use_range {
        return;
    }
    let (heatmap, chapters) = match &dialog.probed {
        Some(Ok(Probed::Media { info, .. })) => (info.heatmap.clone(), info.chapters.clone()),
        _ => Default::default(),
    };
    let mut range = dialog.range;
    if range_bar(ui, duration, &mut range, &heatmap, &chapters) {
        dialog.set_range(range.0, range.1);
    }
    ui.horizontal(|ui| {
        let mut edited = false;
        ui.label(t!("downloader.from"));
        let start =
            ui.add(egui::TextEdit::singleline(&mut dialog.range_text.0).desired_width(70.0));
        ui.label(t!("downloader.to"));
        let end = ui.add(egui::TextEdit::singleline(&mut dialog.range_text.1).desired_width(70.0));
        if start.lost_focus() || end.lost_focus() {
            edited = true;
        }
        let length = (dialog.range.1 - dialog.range.0).max(0.0);
        ui.weak(t!("downloader.length", length = format_time(length)));
        if !chapters.is_empty() {
            egui::ComboBox::from_id_salt("download_chapter")
                .selected_text(t!("downloader.chapter"))
                .show_ui(ui, |ui| {
                    for chapter in &chapters {
                        let text = format!("{}  {}", format_time(chapter.start), chapter.title);
                        if ui.selectable_label(false, text).clicked() {
                            dialog.set_range(chapter.start, chapter.end.min(duration));
                        }
                    }
                });
        }
        if edited {
            let start = parse_time(&dialog.range_text.0).map(|s| s.clamp(0.0, duration));
            let end = parse_time(&dialog.range_text.1).map(|e| e.clamp(0.0, duration));
            match (start, end) {
                (Some(start), Some(end)) if end > start => dialog.set_range(start, end),
                _ => {
                    let (start, end) = dialog.range;
                    dialog.set_range(start, end);
                }
            }
        }
    });
    if !heatmap.is_empty() {
        ui.weak(t!("downloader.heatmap_hint"));
    }
}

/// The section picker: the "most replayed" curve and the chapter marks
/// under two draggable edges. `true` when the range changed.
fn range_bar(
    ui: &mut egui::Ui,
    duration: f64,
    range: &mut (f64, f64),
    heatmap: &[(f64, f64, f32)],
    chapters: &[ytdlp::Chapter],
) -> bool {
    const HANDLE: f32 = 6.0;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, RANGE_HEIGHT),
        egui::Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect);
    let to_x = |t: f64| rect.left() + (t / duration.max(1e-6)) as f32 * rect.width();
    let to_t = |x: f32| (((x - rect.left()) / rect.width()) as f64 * duration).clamp(0.0, duration);
    painter.rect_filled(rect, theme::RADIUS_CONTROL, theme::BG_INSET);
    if !heatmap.is_empty() {
        let points = heatmap
            .iter()
            .map(|&(start, end, value)| {
                let y = rect.bottom() - value.clamp(0.0, 1.0) * (rect.height() - 8.0);
                egui::pos2(to_x((start + end) / 2.0), y)
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(1.5, theme::TEXT_DISABLED),
        ));
    }
    for chapter in chapters.iter().skip(1) {
        let x = to_x(chapter.start);
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            egui::Stroke::new(1.0, theme::BORDER_SUBTLE),
        );
    }
    let (x0, x1) = (to_x(range.0), to_x(range.1));
    let selected = egui::Rect::from_x_y_ranges(x0..=x1, rect.y_range());
    painter.rect_filled(selected, 0.0, theme::ACCENT_TRANSLUCENT);
    painter.rect_stroke(
        selected,
        0.0,
        egui::Stroke::new(1.0, theme::ACCENT),
        egui::StrokeKind::Inside,
    );
    for x in [x0, x1] {
        let handle = egui::Rect::from_center_size(
            egui::pos2(x, rect.center().y),
            egui::vec2(HANDLE, rect.height() * 0.6),
        );
        painter.rect_filled(handle, 2.0, theme::ACCENT);
    }
    let drag_id = response.id.with("edge");
    let mut changed = false;
    if let Some(pos) = response.interact_pointer_pos() {
        if response.drag_started() || response.clicked() {
            let near_end = (pos.x - x1).abs() < (pos.x - x0).abs();
            ui.memory_mut(|m| m.data.insert_temp(drag_id, near_end));
        }
        let moving_end = ui
            .memory(|m| m.data.get_temp::<bool>(drag_id))
            .unwrap_or(false);
        let t = to_t(pos.x);
        let min_len = (duration / 1000.0).max(0.1);
        if moving_end {
            range.1 = t.max(range.0 + min_len).min(duration);
        } else {
            range.0 = t.min(range.1 - min_len).max(0.0);
        }
        changed = true;
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if let Some(pos) = response.hover_pos() {
        response.on_hover_text_at_pointer(format_time(to_t(pos.x)));
    }
    changed
}

#[cfg(test)]
#[path = "tests/downloader_ui.rs"]
mod tests;
