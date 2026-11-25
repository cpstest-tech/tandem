//! File → Settings window.

use std::borrow::Cow;
use std::path::PathBuf;

use vv_media::audio_file::AudioFileFormat;
use vv_media::proxy::ProxyQuality;

use crate::i18n::Language;
use crate::properties_panel::preview_combo;
use crate::settings::{Action, Keymap, Settings, Shortcut};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    General,
    Playback,
    Recording,
    Integrations,
    AiProviders,
    Shortcuts,
}

impl Section {
    const ALL: [Section; 6] = [
        Section::General,
        Section::Playback,
        Section::Recording,
        Section::Integrations,
        Section::AiProviders,
        Section::Shortcuts,
    ];

    fn title(self) -> Cow<'static, str> {
        match self {
            Section::General => t!("settings.section_general"),
            Section::Playback => t!("settings.section_playback"),
            Section::Recording => t!("settings.section_recording"),
            Section::Integrations => t!("settings.section_integrations"),
            Section::AiProviders => t!("settings.section_ai"),
            Section::Shortcuts => t!("settings.section_shortcuts"),
        }
    }
}

/// Shortcut waiting for the next key: replaces the `slot`-th one, or is
/// appended if `None`.
#[derive(Debug, Clone, Copy)]
struct Capture {
    action: Action,
    slot: Option<usize>,
}

pub struct SettingsDialog {
    /// MCP is on for this run whatever the setting (`--mcp`).
    pub mcp_forced: bool,
    section: Section,
    capture: Option<Capture>,
    notice: Option<String>,
    /// Asked once per opening: listing the devices is slow on some hosts.
    input_devices: Vec<String>,
    /// The formats this FFmpeg can write, asked once like the devices.
    recording_formats: Vec<AudioFileFormat>,
    /// Lives in its own owner-only file, never in the settings.
    gemini_key: String,
    gemini_notice: Option<String>,
    ai_models: Vec<String>,
    ai_model_request: Option<std::sync::mpsc::Receiver<Result<Vec<String>, String>>>,
    ai_model_error: Option<String>,
    ai_probe: Option<crate::ai_sequence::availability::Probe>,
    ai_status: Vec<(crate::ai_sequence::Provider, String)>,
}

pub struct SettingsDialogResponse {
    pub open: bool,
    pub changed: bool,
    /// The folder of the takes must be chosen with the system dialog.
    pub pick_recording_dir: bool,
    pub pick_models_dir: bool,
    pub gemini_key_changed: bool,
}

impl SettingsDialog {
    pub fn new(section: Section) -> Self {
        Self {
            mcp_forced: false,
            section,
            capture: None,
            notice: None,
            input_devices: vv_audio::recorder::input_device_names(),
            recording_formats: AudioFileFormat::available(),
            gemini_key: crate::ai_clips::load_key().unwrap_or_default(),
            gemini_notice: None,
            ai_models: Vec::new(),
            ai_model_request: None,
            ai_model_error: None,
            ai_probe: None,
            ai_status: Vec::new(),
        }
    }

    /// While waiting for a key, global shortcuts must be suspended.
    pub fn is_capturing(&self) -> bool {
        self.capture.is_some()
    }

    pub fn show(&mut self, ctx: &egui::Context, settings: &mut Settings) -> SettingsDialogResponse {
        let mut response = SettingsDialogResponse {
            open: true,
            changed: false,
            pick_recording_dir: false,
            pick_models_dir: false,
            gemini_key_changed: false,
        };
        response.changed |= self.capture_key(ctx, &mut settings.keymap);
        egui::Window::new(t!("settings.title"))
            .id(egui::Id::new("settings_window"))
            .vscroll(true)
            .open(&mut response.open)
            .collapsible(false)
            .default_size([640.0, 480.0])
            .show(ctx, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        for section in Section::ALL {
                            ui.selectable_value(&mut self.section, section, section.title());
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| match self.section {
                        Section::General => {
                            response.changed |= general_section(ui, settings);
                        }
                        Section::Recording => {
                            let (changed, pick) = recording_section(
                                ui,
                                settings,
                                &self.input_devices,
                                &self.recording_formats,
                            );
                            response.changed |= changed;
                            response.pick_recording_dir |= pick;
                        }
                        Section::Playback => {
                            response.changed |= playback_section(ui, settings);
                        }
                        Section::Integrations => {
                            response.changed |= integrations_section(ui, settings, self.mcp_forced);
                            ui.add_space(crate::theme::SPACE_3);
                            ui.separator();
                            response.gemini_key_changed |= gemini_key_section(
                                ui,
                                &mut self.gemini_key,
                                &mut self.gemini_notice,
                            );
                        }
                        Section::AiProviders => {
                            let (changed, pick) = models_and_dub_section(ui, settings);
                            response.changed |= changed;
                            response.pick_models_dir |= pick;
                            ui.add_space(crate::theme::SPACE_3);
                            ui.separator();
                            response.changed |= self.ai_section(ui, settings);
                            response.gemini_key_changed |= gemini_key_section(
                                ui,
                                &mut self.gemini_key,
                                &mut self.gemini_notice,
                            );
                        }
                        Section::Shortcuts => {
                            response.changed |= self.shortcuts_section(ui, &mut settings.keymap);
                        }
                    });
                });
            });
        if !response.open {
            self.capture = None;
        }
        response
    }

    fn ai_section(&mut self, ui: &mut egui::Ui, settings: &mut Settings) -> bool {
        use crate::ai_sequence::{Provider, availability::Probe};
        if self.ai_status.is_empty() && self.ai_probe.is_none() {
            self.ai_probe = Some(Probe::start());
        }
        if let Some(statuses) = self.ai_probe.as_ref().and_then(|probe| probe.poll()) {
            self.ai_status = statuses;
            self.ai_probe = None;
        }
        if self.ai_probe.is_some() {
            ui.spinner();
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(150));
        }
        for (provider, status) in &self.ai_status {
            ui.label(format!("{}: {status}", provider.label()));
        }
        if ui.button(t!("ai_sequence.detect")).clicked() {
            self.ai_probe = Some(Probe::start());
        }
        let mut changed = false;
        ui.separator();
        ui.label(t!("ai_sequence.default_provider"));
        let previous_provider = settings.ai_provider;
        egui::ComboBox::from_id_salt("ai_default_provider")
            .selected_text(settings.ai_provider.label())
            .show_ui(ui, |ui| {
                for provider in Provider::ALL {
                    changed |= ui
                        .selectable_value(&mut settings.ai_provider, provider, provider.label())
                        .changed();
                }
            });
        settings.remember_ai_model_on_provider_change(previous_provider);
        ui.label(t!("ai_sequence.settings_model"));
        changed |= ui.text_edit_singleline(&mut settings.ai_model).changed();
        if let Some(result) = self
            .ai_model_request
            .as_ref()
            .and_then(|rx| rx.try_recv().ok())
        {
            self.ai_model_request = None;
            match result {
                Ok(models) => {
                    self.ai_models = models;
                    self.ai_model_error = None;
                }
                Err(error) => self.ai_model_error = Some(error),
            }
        }
        if self.ai_model_request.is_some() {
            ui.spinner();
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(150));
        } else if ui.button(t!("ai_sequence.list_models")).clicked() {
            let (tx, rx) = std::sync::mpsc::channel();
            self.ai_model_request = Some(rx);
            std::thread::spawn(move || {
                let _ = tx.send(crate::gemini::list_models());
            });
        }
        if !self.ai_models.is_empty()
            && settings.ai_provider == crate::ai_sequence::Provider::Gemini
        {
            egui::ComboBox::from_id_salt("gemini_accessible_models")
                .selected_text(&settings.ai_model)
                .show_ui(ui, |ui| {
                    for model in &self.ai_models {
                        changed |= ui
                            .selectable_value(&mut settings.ai_model, model.clone(), model)
                            .changed();
                    }
                });
            ui.small(t!("ai_sequence.listing_hint"));
        }
        if let Some(error) = &self.ai_model_error {
            ui.colored_label(crate::theme::ERROR, error);
        }
        ui.label(t!("ai_sequence.hydrogen_hint"));
        if settings.ai_provider == crate::ai_sequence::Provider::Gemini {
            ui.label(crate::gemini::free_tier_label(&settings.ai_model));
        }
        changed |= sequence_api_section(ui, settings);
        ui.hyperlink_to(
            t!("ai_sequence.pricing_tiers"),
            "https://aistudio.google.com/docs/pricing",
        );
        ui.hyperlink_to(
            t!("ai_sequence.quotas"),
            "https://aistudio.google.com/rate-limit",
        );
        ui.separator();
        ui.label(t!("ai_sequence.attempts"));
        changed |= ui
            .add(egui::Slider::new(
                &mut settings.ai_limits.max_attempts,
                1..=5,
            ))
            .changed();
        ui.label(t!("ai_sequence.provider_timeout"));
        changed |= ui
            .add(
                egui::DragValue::new(&mut settings.ai_limits.provider_timeout_secs)
                    .range(10..=3600),
            )
            .changed();
        ui.label(t!("ai_sequence.render_timeout"));
        changed |= ui
            .add(egui::DragValue::new(&mut settings.ai_limits.render_timeout_secs).range(10..=3600))
            .changed();
        ui.label(t!("ai_sequence.runtime"));
        for (label, path) in [
            (
                t!("ai_sequence.runtime_directory"),
                &mut settings.ai_runtime.directory,
            ),
            (t!("ai_sequence.node"), &mut settings.ai_runtime.node),
            (t!("ai_sequence.browser"), &mut settings.ai_runtime.browser),
        ] {
            ui.label(label);
            let mut text = path.to_string_lossy().into_owned();
            if ui.text_edit_singleline(&mut text).changed() {
                *path = text.into();
                changed = true;
            }
        }
        changed
    }

    fn capture_key(&mut self, ctx: &egui::Context, keymap: &mut Keymap) -> bool {
        let Some(capture) = self.capture else {
            return false;
        };
        let pressed = ctx.input(|i| i.events.iter().find_map(Shortcut::captured));
        let Some(shortcut) = pressed else {
            return false;
        };
        self.capture = None;
        if shortcut == Shortcut::plain(egui::Key::Escape) {
            return false;
        }
        let stolen = keymap.assign(capture.action, capture.slot, shortcut);
        self.notice = (!stolen.is_empty()).then(|| {
            let names: Vec<Cow<str>> = stolen.iter().map(|a| a.label()).collect();
            t!(
                "settings.shortcut_taken_from",
                shortcut = shortcut,
                actions = names.join(", ")
            )
            .into_owned()
        });
        true
    }

    fn shortcuts_section(&mut self, ui: &mut egui::Ui, keymap: &mut Keymap) -> bool {
        let mut changed = false;
        ui.label(t!("settings.shortcuts_hint"));
        if let Some(notice) = &self.notice {
            crate::ui_kit::status(ui, crate::ui_kit::Tone::Warning, notice);
        }
        ui.add_space(crate::theme::SPACE_1);
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 36.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let mut category = Cow::Borrowed("");
                for action in Action::ALL {
                    if action.category() != category {
                        category = action.category();
                        ui.add_space(crate::theme::SPACE_2);
                        ui.strong(category.as_ref());
                    }
                    ui.horizontal(|ui| {
                        ui.add_sized([220.0, 20.0], egui::Label::new(action.label()).truncate());
                        changed |= self.bindings_row(ui, keymap, action);
                    });
                }
            });
        ui.separator();
        if ui.button(t!("settings.restore_defaults")).clicked() {
            *keymap = Keymap::default();
            self.capture = None;
            self.notice = None;
            changed = true;
        }
        changed
    }

    fn bindings_row(&mut self, ui: &mut egui::Ui, keymap: &mut Keymap, action: Action) -> bool {
        let capture = self.capture;
        let waiting = |slot| capture.is_some_and(|c| c.action == action && c.slot == slot);
        let mut remove = None;
        for (slot, shortcut) in keymap.shortcuts(action).iter().enumerate() {
            let text = if waiting(Some(slot)) {
                t!("settings.press_a_key").into_owned()
            } else {
                shortcut.to_string()
            };
            if ui.button(text).clicked() {
                self.capture = Some(Capture {
                    action,
                    slot: Some(slot),
                });
            }
            if ui
                .small_button("x")
                .on_hover_text(t!("settings.remove"))
                .clicked()
            {
                remove = Some(slot);
            }
        }
        let add = if waiting(None) {
            t!("settings.press_a_key")
        } else {
            "+".into()
        };
        if ui
            .small_button(add)
            .on_hover_text(t!("settings.add_shortcut"))
            .clicked()
        {
            self.capture = Some(Capture { action, slot: None });
        }
        if let Some(slot) = remove {
            keymap.remove(action, slot);
            self.capture = None;
            return true;
        }
        false
    }
}

fn general_section(ui: &mut egui::Ui, settings: &mut Settings) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(t!("settings.language"));
        let items: Vec<_> = Language::ALL
            .iter()
            .map(|language| (*language, language.label().to_string(), true))
            .collect();
        changed |= preview_combo(
            ui,
            "settings_language",
            &mut settings.language,
            &items,
            None,
            None,
            None,
        );
    });
    if changed {
        settings.language.apply();
    }
    changed |= ui
        .checkbox(&mut settings.kinetic_scroll, t!("settings.kinetic_scroll"))
        .changed();
    changed |= ui
        .checkbox(
            &mut settings.kinetic_scroll_media_pool,
            t!("settings.kinetic_scroll_media_pool"),
        )
        .changed();
    changed |= ui
        .checkbox(&mut settings.check_updates, t!("update.check_at_startup"))
        .changed();
    changed
}

pub(crate) fn input_device_picker(
    ui: &mut egui::Ui,
    input_device: &mut Option<String>,
    input_devices: &[String],
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(t!("settings.input_device"));
        let default = t!("settings.input_device_default").into_owned();
        let missing = input_device
            .clone()
            .filter(|name| !input_devices.contains(name));
        egui::ComboBox::from_id_salt("input_device")
            .width(180.0)
            .selected_text(input_device.as_deref().unwrap_or(&default))
            .show_ui(ui, |ui| {
                changed |= ui.selectable_value(input_device, None, &default).changed();
                // A device unplugged since keeps its place: the recording
                // falls back to the default until it is back.
                for name in input_devices.iter().chain(missing.as_ref()) {
                    changed |= ui
                        .selectable_value(input_device, Some(name.clone()), name)
                        .changed();
                }
            });
    });
    changed
}

/// `(changed, pick the folder)`.
fn recording_section(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    input_devices: &[String],
    formats: &[AudioFileFormat],
) -> (bool, bool) {
    let mut changed = input_device_picker(ui, &mut settings.input_device, input_devices);
    ui.add_space(crate::theme::SPACE_2);
    ui.horizontal(|ui| {
        ui.label(t!("settings.recording_format"));
        let current = AudioFileFormat::resolve(settings.recording_format);
        egui::ComboBox::from_id_salt("settings_recording_format")
            .width(160.0)
            .selected_text(current.label())
            .show_ui(ui, |ui| {
                for &format in formats {
                    if ui
                        .selectable_label(format == current, format.label())
                        .clicked()
                    {
                        settings.recording_format = Some(format);
                        changed = true;
                    }
                }
            });
    });
    if settings
        .recording_format
        .is_some_and(|f| !formats.contains(&f))
    {
        ui.weak(t!("settings.recording_format_missing"));
    }

    ui.add_space(crate::theme::SPACE_2);
    ui.label(t!("settings.recording_dir"));
    let mut pick = false;
    if ui
        .radio(
            settings.recording_dir.is_none(),
            t!("settings.recording_dir_project"),
        )
        .clicked()
        && settings.recording_dir.is_some()
    {
        settings.recording_dir = None;
        changed = true;
    }
    ui.horizontal(|ui| {
        // Choosing "another folder" is choosing which one.
        pick |= ui
            .radio(
                settings.recording_dir.is_some(),
                t!("settings.recording_dir_other"),
            )
            .clicked();
        if let Some(dir) = &settings.recording_dir {
            ui.monospace(dir.display().to_string());
        }
        pick |= ui.button(t!("settings.recording_dir_choose")).clicked();
    });
    (changed, pick)
}

/// Where models go, and what dubbing may send out.
fn models_and_dub_section(ui: &mut egui::Ui, settings: &mut Settings) -> (bool, bool) {
    let mut changed = false;
    let mut pick = false;
    ui.label(egui::RichText::new(t!("settings.models_dir")).strong());
    ui.horizontal(|ui| {
        let shown = crate::subtitles::models_dir()
            .map(|d| d.display().to_string())
            .unwrap_or_default();
        ui.monospace(shown);
    });
    ui.horizontal(|ui| {
        pick |= ui.button(t!("settings.models_dir_choose")).clicked();
        if ui
            .add_enabled(
                settings.models_dir.is_some(),
                egui::Button::new(t!("settings.models_dir_default")),
            )
            .clicked()
        {
            settings.models_dir = None;
            changed = true;
        }
    });
    ui.weak(t!("settings.models_dir_hint"));
    changed |= ui
        .checkbox(
            &mut settings.shorts.dub.allow_cloud_translation,
            t!("dub.cloud_consent"),
        )
        .changed();
    ui.weak(t!("dub.cloud_consent_hint"));
    (changed, pick)
}

fn integrations_section(ui: &mut egui::Ui, settings: &mut Settings, forced: bool) -> bool {
    let changed = ui
        .checkbox(&mut settings.mcp_enabled, t!("settings.mcp_enabled"))
        .changed();
    if forced && !settings.mcp_enabled {
        ui.label(egui::RichText::new(t!("settings.mcp_forced")).color(crate::theme::ACCENT));
    }
    ui.add_space(crate::theme::SPACE_1);
    ui.label(egui::RichText::new(t!("settings.mcp_enabled_hint")).weak());
    // Inside an AppImage, `current_exe` is the temporary mount.
    let exe = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
        .map(|exe| {
            // Linux marks a binary replaced while running.
            let path = exe.display().to_string();
            PathBuf::from(path.strip_suffix(" (deleted)").unwrap_or(&path))
        });
    let command = format!(
        "{} mcp --attach",
        exe.map_or_else(|| "vv-app".into(), |exe| exe.display().to_string())
    );
    ui.add(
        egui::TextEdit::singleline(&mut command.as_str())
            .code_editor()
            .desired_width(f32::INFINITY),
    );
    changed
}

fn gemini_key_section(ui: &mut egui::Ui, key: &mut String, notice: &mut Option<String>) -> bool {
    let mut changed = false;
    ui.strong(t!("settings.gemini_key"));
    ui.label(egui::RichText::new(t!("settings.gemini_key_hint")).weak());
    ui.add(
        egui::TextEdit::singleline(key)
            .password(true)
            .desired_width(f32::INFINITY)
            .hint_text(t!("settings.gemini_key_placeholder")),
    );
    ui.horizontal(|ui| {
        if ui.button(t!("settings.gemini_key_save")).clicked() {
            *notice = Some(if key.trim().is_empty() {
                t!("settings.gemini_key_empty").into_owned()
            } else {
                match crate::ai_clips::save_key(key) {
                    Ok(()) => {
                        changed = true;
                        t!("settings.gemini_key_saved").into_owned()
                    }
                    Err(error) => error,
                }
            });
        }
        if ui.button(t!("settings.gemini_key_delete")).clicked() {
            *notice = Some(match crate::ai_clips::delete_key() {
                Ok(()) => {
                    key.clear();
                    changed = true;
                    t!("settings.gemini_key_deleted").into_owned()
                }
                Err(error) => error,
            });
        }
        ui.hyperlink_to(
            t!("settings.gemini_key_get"),
            "https://aistudio.google.com/apikey",
        );
    });
    if let Some(notice) = notice {
        ui.label(notice.as_str());
    }
    if let Ok(path) = crate::ai_clips::key_path() {
        ui.label(
            egui::RichText::new(t!("settings.gemini_key_location", path = path.display()))
                .small()
                .weak(),
        );
    }
    changed
}

/// Under `Auto` if it is what failed, else under `Off`, where the app
/// moved the setting.
fn warns_under(
    fallback: &crate::hw_decode::Fallback,
    mode: crate::hw_decode::HwDecodeMode,
) -> bool {
    use crate::hw_decode::HwDecodeMode;
    match fallback.mode {
        HwDecodeMode::Auto => mode == HwDecodeMode::Auto,
        _ => mode == HwDecodeMode::Off,
    }
}

fn hw_decode_settings(ui: &mut egui::Ui, settings: &mut Settings) -> bool {
    use crate::hw_decode::HwDecodeMode;
    let mut changed = false;
    ui.strong(t!("settings.hw_decode"));
    for &mode in HwDecodeMode::available() {
        let (label, hint) = match mode {
            HwDecodeMode::Auto => {
                let label = match crate::hw_decode::auto_device() {
                    None => t!("settings.hw_decode_auto").into_owned(),
                    Some(device) => format!(
                        "{} ({})",
                        t!("settings.hw_decode_auto"),
                        device.map_or_else(
                            || t!("settings.hw_decode_auto_cpu").into_owned(),
                            |d| crate::hw_decode::device_label(&d)
                        )
                    ),
                };
                (label.into(), Some(t!("settings.hw_decode_auto_hint")))
            }
            HwDecodeMode::Off => (t!("settings.hw_decode_off"), None),
            HwDecodeMode::Nvdec => (t!("settings.hw_decode_nvdec"), None),
            HwDecodeMode::Vaapi => (
                t!("settings.hw_decode_vaapi"),
                Some(t!("settings.hw_decode_vaapi_hint")),
            ),
            HwDecodeMode::Vulkan => (
                t!("settings.hw_decode_vulkan"),
                Some(t!("settings.hw_decode_vulkan_hint")),
            ),
        };
        let response = ui.radio_value(&mut settings.hw_decode, mode, label);
        let response = match hint {
            Some(hint) => response.on_hover_text(hint),
            None => response,
        };
        changed |= response.changed();
        if let Some(fallback) = crate::hw_decode::fallback()
            && warns_under(&fallback, mode)
        {
            let devices: Vec<String> = fallback
                .devices
                .iter()
                .map(crate::hw_decode::device_label)
                .collect();
            ui.colored_label(
                crate::theme::WARNING,
                t!("settings.hw_decode_failed", devices = devices.join(", ")),
            );
        }
    }
    if crate::hw_decode::has_intel_gpu() == Some(true) {
        changed |= ui
            .checkbox(
                &mut settings.intel_experimental_decode,
                t!("settings.hw_decode_intel_experimental"),
            )
            .on_hover_text(t!("settings.hw_decode_intel_experimental_hint"))
            .changed();
    }
    ui.add_enabled_ui(settings.hw_decode != HwDecodeMode::Off, |ui| {
        ui.horizontal(|ui| {
            ui.label(t!("settings.hw_decode_memory"))
                .on_hover_text(t!("settings.hw_decode_memory_hint"));
            let mut budget_mb = (settings.hw_decode_budget_bytes() / 1_000_000) as u32;
            if ui
                .add(
                    egui::DragValue::new(&mut budget_mb)
                        .range(100..=64000)
                        .suffix(" MB"),
                )
                .on_hover_text(t!("settings.hw_decode_memory_hint"))
                .changed()
            {
                settings.hw_decode_budget_bytes = Some(budget_mb as usize * 1_000_000);
                changed = true;
            }
            if ui
                .add_enabled(
                    settings.hw_decode_budget_bytes.is_some(),
                    egui::Button::new(t!("settings.hw_decode_memory_reset")),
                )
                .clicked()
            {
                settings.hw_decode_budget_bytes = None;
                changed = true;
            }
        });
    });
    changed
}

fn playback_section(ui: &mut egui::Ui, settings: &mut Settings) -> bool {
    let mut changed = false;
    ui.strong(t!("settings.read_ahead"));
    egui::Grid::new("settings_read_ahead")
        .num_columns(2)
        .show(ui, |ui| {
            ui.label(t!("settings.read_ahead_forward"))
                .on_hover_text(t!("settings.read_ahead_forward_hint"));
            changed |= ui
                .add(
                    egui::DragValue::new(&mut settings.lookahead_secs)
                        .range(0.0..=30.0)
                        .speed(0.1)
                        .suffix(" s"),
                )
                .on_hover_text(t!("settings.read_ahead_forward_hint"))
                .changed();
            ui.end_row();

            ui.label(t!("settings.read_ahead_behind"))
                .on_hover_text(t!("settings.read_ahead_behind_hint"));
            changed |= ui
                .add(
                    egui::DragValue::new(&mut settings.behind_secs)
                        .range(0.0..=30.0)
                        .speed(0.1)
                        .suffix(" s"),
                )
                .on_hover_text(t!("settings.read_ahead_behind_hint"))
                .changed();
            ui.end_row();

            ui.label(t!("settings.video_cache"))
                .on_hover_text(t!("settings.video_cache_hint"));
            let mut budget_mb = (settings.cache_budget_bytes / 1_000_000) as u32;
            if ui
                .add(
                    egui::DragValue::new(&mut budget_mb)
                        .range(100..=8000)
                        .suffix(" MB"),
                )
                .on_hover_text(t!("settings.video_cache_hint"))
                .changed()
            {
                settings.cache_budget_bytes = budget_mb as usize * 1_000_000;
                changed = true;
            }
            ui.end_row();

            ui.label(t!("settings.audio_cache"))
                .on_hover_text(t!("settings.audio_cache_hint"));
            let mut budget_mb = (settings.audio_budget_bytes / 1_000_000) as u32;
            if ui
                .add(
                    egui::DragValue::new(&mut budget_mb)
                        .range(crate::settings::AUDIO_BUDGET_MB)
                        .suffix(" MB"),
                )
                .on_hover_text(t!("settings.audio_cache_hint"))
                .changed()
            {
                settings.audio_budget_bytes = budget_mb as usize * 1_000_000;
                changed = true;
            }
            ui.end_row();
        });
    changed |= ui
        .checkbox(
            &mut settings.fast_playback_shown_frames_only,
            t!("settings.fast_playback_shown_frames_only"),
        )
        .on_hover_text(t!("settings.fast_playback_shown_frames_only_hint"))
        .changed();
    changed |= ui
        .checkbox(
            &mut settings.keyframes_when_behind,
            t!("settings.keyframes_when_behind"),
        )
        .on_hover_text(t!("settings.keyframes_when_behind_hint"))
        .changed();

    ui.add_space(crate::theme::SPACE_3);
    changed |= hw_decode_settings(ui, settings);

    ui.add_space(crate::theme::SPACE_3);
    ui.strong(t!("settings.proxy"));
    changed |= ui
        .checkbox(&mut settings.proxy_enabled, t!("menu.use_proxy"))
        .on_hover_text(t!("menu.use_proxy_hint"))
        .changed();
    ui.add_enabled_ui(!settings.proxy_enabled, |ui| {
        changed |= ui
            .checkbox(&mut settings.proxy_auto, t!("settings.proxy_auto"))
            .on_hover_text(t!("settings.proxy_auto_hint"))
            .changed();
    });
    ui.add_enabled_ui(settings.proxy().is_some(), |ui| {
        ui.label(t!("settings.proxy_quality"));
        for quality in ProxyQuality::ALL {
            let (label, hint) = match quality {
                ProxyQuality::Low => (
                    t!("settings.proxy_quality_low"),
                    t!("settings.proxy_quality_low_hint"),
                ),
                ProxyQuality::Medium => (
                    t!("settings.proxy_quality_medium"),
                    t!("settings.proxy_quality_medium_hint"),
                ),
                ProxyQuality::High => (
                    t!("settings.proxy_quality_high"),
                    t!("settings.proxy_quality_high_hint"),
                ),
            };
            let text = format!("{label} ({} px)", quality.max_width());
            changed |= ui
                .radio_value(&mut settings.proxy_quality, quality, text)
                .on_hover_text(hint)
                .changed();
        }
        ui.label(egui::RichText::new(t!("settings.proxy_quality_note")).weak());
    });
    changed
}

pub(crate) fn sequence_api_section(ui: &mut egui::Ui, settings: &mut Settings) -> bool {
    let mut changed = ui
        .checkbox(&mut settings.ai_free_only, t!("ai_sequence.free_only"))
        .changed();
    if settings.ai_free_only {
        let identity = crate::gemini::configured_key()
            .ok()
            .map(|key| crate::gemini::key_identity(&key));
        let mut confirmed = identity.is_some() && identity == settings.ai_free_key_identity;
        if ui
            .add_enabled(
                identity.is_some(),
                egui::Checkbox::new(&mut confirmed, t!("ai_sequence.confirm_unbilled")),
            )
            .changed()
        {
            settings.ai_free_key_identity = if confirmed { identity } else { None };
            changed = true;
        }
        ui.small(t!("ai_sequence.billing_limit"));
    } else {
        changed |= ui
            .checkbox(&mut settings.ai_allow_api, t!("ai_sequence.allow_billing"))
            .changed();
    }
    changed
}
