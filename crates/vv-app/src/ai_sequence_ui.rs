use super::*;
use crate::ai_sequence::setup::{self, Setup};
use crate::ai_sequence::{Composition, Job, Placement, Provider, Queue, Request, Stage};
use crate::ai_sequence_cards::{
    Art, card_strip, choice_card, improvement_card, section_title, tab_bar,
};
use vv_core::Id;

#[path = "ai_sequence_material_ui.rs"]
mod material_ui;
use material_ui::PickTarget;

#[derive(Default)]
pub(crate) struct State {
    prompt: String,
    length: Length,
    custom_seconds: f64,
    style: String,
    placement: Placement,
    no_frames: bool,
    no_audio: bool,
    no_transcript: bool,
    transparent: bool,
    use_marks: bool,
    parent: Option<String>,
    queues: HashMap<PathBuf, Queue>,
    error: Option<String>,
    revision_prompt: String,
    fresh_session: bool,
    source_copy: Option<SourceCopy>,
    deferred_imports: Vec<(PathBuf, Job)>,
    analysis: Option<crate::ai_sequence::suggestions::Analysis>,
    analysis_project: Option<PathBuf>,
    analysis_base: Option<Request>,
    suggestions: Vec<crate::ai_sequence::suggestions::Suggestion>,
    analysis_provider: Provider,
    analysis_model: String,
    refresh_analysis: bool,
    analysis_usage: Option<serde_json::Value>,
    pending_imports: HashMap<String, (PathBuf, Job)>,
    import_jobs: HashMap<(u64, vv_session::JobId), String>,
    import_owner: Option<(u64, Option<PathBuf>)>,
    dismissed_placeholders: std::collections::HashSet<String>,
    earlier_jobs: Option<(PathBuf, std::collections::HashSet<String>)>,
    pending_applies: HashMap<String, RevisionTarget>,
    tab: PanelTab,
    shown_frame: u64,
    scope: Scope,
    analysis_prompt: String,
    setup: Option<Setup>,
    setup_error: Option<String>,
    /// Requests submitted while the runtime was still being installed.
    waiting: Vec<Request>,
    no_material_requests: bool,
    no_suggestion_material: bool,
    /// Photos/videos given before generation, with what they show.
    material: Vec<(PathBuf, String)>,
    material_pick: Option<(PickTarget, std::sync::mpsc::Receiver<Option<PathBuf>>)>,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum PanelTab {
    #[default]
    Generate,
    Suggestions,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Scope {
    #[default]
    Selection,
    Timeline,
}

/// Where a finished revision lands without a second click.
struct RevisionTarget {
    timeline: TimelineId,
    clip: vv_core::ClipId,
    expected: MediaId,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Length {
    Clip,
    #[default]
    Auto,
    Custom,
}

const AUTO_MAX_SECONDS: f64 = 30.0;
const NO_CONTEXT_SECONDS: f64 = 5.0;
const AUTO_PLACEHOLDER_SECONDS: f64 = 8.0;

struct Look {
    label: &'static str,
    hint: &'static str,
    prompt: &'static str,
    art: Art,
}

const DEFAULT_LOOK: usize = 1;

const LOOKS: [Look; 10] = [
    Look {
        label: "ai_sequence.look_simple",
        hint: "ai_sequence.look_simple_hint",
        prompt: "Simple: minimal and essential, lots of white space, few elements, a neutral palette",
        art: crate::ai_sequence_cards::art_simple,
    },
    Look {
        label: "ai_sequence.look_modern",
        hint: "ai_sequence.look_modern_hint",
        prompt: "Modern: clean, dynamic and contemporary, smooth motion, a refined teal-accented palette",
        art: crate::ai_sequence_cards::art_modern,
    },
    Look {
        label: "ai_sequence.look_vivid",
        hint: "ai_sequence.look_vivid_hint",
        prompt: "Vivid: colorful and expressive, bold saturated colors, playful energetic motion",
        art: crate::ai_sequence_cards::art_vivid,
    },
    Look {
        label: "ai_sequence.look_corporate",
        hint: "ai_sequence.look_corporate_hint",
        prompt: "Corporate: professional and trustworthy, navy and white palette, clean sans-serif type, tidy charts and icons, restrained motion",
        art: crate::ai_sequence_cards::art_corporate,
    },
    Look {
        label: "ai_sequence.look_neon",
        hint: "ai_sequence.look_neon_hint",
        prompt: "Neon: dark background with glowing cyan and magenta lines, futuristic tech feel, light trails and pulsing glows",
        art: crate::ai_sequence_cards::art_neon,
    },
    Look {
        label: "ai_sequence.look_editorial",
        hint: "ai_sequence.look_editorial_hint",
        prompt: "Editorial: magazine/newspaper layout, cream paper background, black serif headlines, thin rules and a single red accent, elegant type-driven motion",
        art: crate::ai_sequence_cards::art_editorial,
    },
    Look {
        label: "ai_sequence.look_glass",
        hint: "ai_sequence.look_glass_hint",
        prompt: "Glass: glassmorphism, frosted translucent panels with soft borders over blurred colorful gradient blobs, gentle floating motion",
        art: crate::ai_sequence_cards::art_glass,
    },
    Look {
        label: "ai_sequence.look_sketch",
        hint: "ai_sequence.look_sketch_hint",
        prompt: "Sketch: hand-drawn whiteboard style, marker strokes that draw themselves on, doodle arrows, circles and icons on a white board",
        art: crate::ai_sequence_cards::art_sketch,
    },
    Look {
        label: "ai_sequence.look_retro",
        hint: "ai_sequence.look_retro_hint",
        prompt: "Retro: 70s/80s vintage look, warm orange, mustard and brown palette, sunset stripes, rounded bold type, slight grain",
        art: crate::ai_sequence_cards::art_retro,
    },
    Look {
        label: "ai_sequence.look_elegant",
        hint: "ai_sequence.look_elegant_hint",
        prompt: "Elegant: luxury black and gold, thin serif type, fine lines and generous spacing, slow refined reveals",
        art: crate::ai_sequence_cards::art_elegant,
    },
];

const PLACEMENTS: [(Placement, &str, Art); 4] = [
    (
        Placement::Fullscreen,
        "ai_sequence.where_full_hint",
        crate::ai_sequence_cards::art_fullscreen,
    ),
    (
        Placement::Pill,
        "ai_sequence.placement_pill_hint",
        crate::ai_sequence_cards::art_pill,
    ),
    (
        Placement::AboveHead,
        "ai_sequence.placement_above_head_hint",
        crate::ai_sequence_cards::art_above_head,
    ),
    (
        Placement::Side,
        "ai_sequence.where_side_hint",
        crate::ai_sequence_cards::art_side,
    ),
];

fn placement_key(placement: Placement) -> &'static str {
    match placement {
        Placement::Fullscreen => "ai_sequence.placement_fullscreen",
        Placement::Pill => "ai_sequence.placement_pill",
        Placement::AboveHead => "ai_sequence.placement_above_head",
        Placement::Side => "ai_sequence.placement_side",
    }
}

fn accent_button(ui: &mut egui::Ui, text: String, enabled: bool) -> egui::Response {
    let button = egui::Button::new(
        egui::RichText::new(text)
            .color(theme::ON_ACCENT)
            .size(theme::FONT_SECTION),
    )
    .fill(theme::ACCENT_FILL)
    .min_size(egui::vec2(ui.available_width(), theme::FIELD_HEIGHT));
    ui.add_enabled(enabled, button)
}

struct Range {
    start: i64,
    end: i64,
    anchor: Option<u64>,
    anchor_details: Option<crate::ai_sequence::Anchor>,
    clips: usize,
    from_marks: bool,
}

impl VenturiApp {
    pub(crate) fn poll_ai_sequences(&mut self, ctx: &egui::Context) {
        self.poll_ai_source_copy();
        self.poll_ai_runtime_setup(ctx);
        self.poll_material_pick();
        if self.ai_sequences.material_pick.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        let project_path = self.session.path().map(Path::to_path_buf);
        let owner = (self.session.epoch(), project_path.clone());
        if self.ai_sequences.import_owner.as_ref() != Some(&owner) {
            self.ai_sequences.import_owner = Some(owner);
            self.ai_sequences.deferred_imports.extend(
                self.ai_sequences
                    .pending_imports
                    .drain()
                    .map(|(_, entry)| entry),
            );
            self.ai_sequences.import_jobs.clear();
        }
        if let Some(result) = self
            .ai_sequences
            .analysis
            .as_ref()
            .and_then(|analysis| analysis.poll())
        {
            if let Some(analysis) = self.ai_sequences.analysis.take() {
                self.ai_sequences.analysis_base = Some(analysis.request.clone());
            }
            match result {
                Ok(report) => {
                    self.ai_sequences.analysis_base = Some(report.request);
                    self.ai_sequences.suggestions = report.suggestions;
                    self.ai_sequences.analysis_usage = report.usage;
                    self.toast(if report.cached {
                        "Reused saved visual analysis"
                    } else {
                        "Visual suggestions are ready; select which ones to generate"
                    });
                }
                Err(error) => self.ai_sequences.error = Some(error),
            }
        }
        if self.ai_sequences.analysis.is_some() || self.ai_sequences.source_copy.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
        if let Some(path) = &project_path
            && !self.ai_sequences.queues.contains_key(path)
        {
            match Queue::open(path) {
                Ok(queue) => {
                    self.ai_sequences.deferred_imports.extend(
                        queue
                            .jobs
                            .iter()
                            .filter(|job| job.stage == Stage::Completed && !job.imported)
                            .cloned()
                            .map(|job| (path.clone(), job)),
                    );
                    self.ai_sequences.queues.insert(path.clone(), queue);
                }
                Err(error) => self.ai_sequences.error = Some(error),
            }
        }
        let mut completed = Vec::new();
        for (path, queue) in &mut self.ai_sequences.queues {
            for job in queue.poll() {
                if job.stage == Stage::Completed {
                    if Some(path) == project_path.as_ref() {
                        completed.push(job);
                    } else {
                        self.ai_sequences.deferred_imports.push((path.clone(), job));
                    }
                } else {
                    self.ai_sequences.pending_applies.remove(&job.id);
                }
            }
        }
        if self.ai_sequences.source_copy.is_none() {
            let deferred = std::mem::take(&mut self.ai_sequences.deferred_imports);
            for (path, job) in deferred {
                if Some(&path) == project_path.as_ref() {
                    completed.push(job);
                } else {
                    self.ai_sequences.deferred_imports.push((path, job));
                }
            }
        }
        for job in completed {
            if self.ai_sequences.source_copy.is_some() {
                if let Some(path) = &project_path {
                    self.ai_sequences.deferred_imports.push((path.clone(), job));
                }
                continue;
            }
            let empty = crate::ai_sequence::material::empty_count(&job);
            if empty > 0 && !job.request.render_only {
                self.toast(t!("ai_sequence.material_waiting", count = empty));
            } else {
                self.toast(format!("AI sequence {} is ready", job.id));
            }
            self.import_ai_sequence(&job);
        }
        if self.edit_drag_group.is_none() && !self.timeline_state.holds_undo_group() {
            self.finish_ai_imports();
        }
        if self
            .ai_sequences
            .queues
            .values()
            .any(|q| q.jobs.iter().any(|j| !j.stage.terminal()))
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
    }

    pub(crate) fn show_ai_sequence_tab(&mut self, ui: &mut egui::Ui) {
        // The tab keeps its scroll offset while hidden; reopening must start from the top.
        let frame = ui.ctx().cumulative_frame_nr();
        let reopened = frame > self.ai_sequences.shown_frame + 1;
        self.ai_sequences.shown_frame = frame;
        self.ensure_ai_runtime(false);
        self.open_ai_queue();

        ui.add_space(theme::SPACE_2);
        ui.heading(t!("ai_sequence.title"));
        ui.add_space(theme::SPACE_1);
        let generate_label = t!("ai_sequence.tab_generate");
        let suggestions_label = t!("ai_sequence.tab_suggestions");
        let current = self.ai_sequences.tab as usize;
        let clicked = tab_bar(ui, &[&generate_label, &suggestions_label], current)
            .filter(|index| *index != current);
        if let Some(index) = clicked {
            self.ai_sequences.tab = if index == 0 {
                PanelTab::Generate
            } else {
                PanelTab::Suggestions
            };
        }
        ui.add_space(theme::SPACE_1);
        let mut area = egui::ScrollArea::vertical()
            .id_salt(("ai_sequence_tab", self.ai_sequences.tab as usize))
            .auto_shrink([false, false]);
        if reopened || clicked.is_some() {
            area = area.vertical_scroll_offset(0.0);
        }
        area.show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            match self.ai_sequences.tab {
                PanelTab::Generate => self.show_ai_generate(ui),
                PanelTab::Suggestions => self.show_ai_suggestions(ui),
            }
        });
    }

    fn open_ai_queue(&mut self) {
        if let Some(path) = self.session.path().map(Path::to_path_buf)
            && !self.ai_sequences.queues.contains_key(&path)
        {
            match Queue::open(&path) {
                Ok(queue) => {
                    self.ai_sequences.queues.insert(path, queue);
                }
                Err(error) => self.ai_sequences.error = Some(error),
            }
        }
    }

    fn ai_style(&self) -> String {
        if self.ai_sequences.style.is_empty() {
            LOOKS[DEFAULT_LOOK].prompt.to_owned()
        } else {
            self.ai_sequences.style.clone()
        }
    }

    fn show_style_cards(&mut self, ui: &mut egui::Ui) {
        let mut chosen = None;
        card_strip(ui, "ai_styles", |ui| {
            for (index, look) in LOOKS.iter().enumerate() {
                let selected = self.ai_sequences.style == look.prompt
                    || (self.ai_sequences.style.is_empty() && index == DEFAULT_LOOK);
                if choice_card(ui, selected, &t!(look.label), look.art)
                    .on_hover_text(t!(look.hint))
                    .clicked()
                {
                    chosen = Some(look.prompt);
                }
            }
        });
        if let Some(prompt) = chosen {
            self.ai_sequences.style = prompt.into();
        }
    }

    fn show_placement_cards(&mut self, ui: &mut egui::Ui) {
        let mut chosen = None;
        card_strip(ui, "ai_placements", |ui| {
            for (placement, hint, art) in PLACEMENTS {
                if choice_card(
                    ui,
                    self.ai_sequences.placement == placement,
                    &t!(placement_key(placement)),
                    art,
                )
                .on_hover_text(t!(hint))
                .clicked()
                {
                    chosen = Some(placement);
                }
            }
        });
        if let Some(placement) = chosen {
            self.ai_sequences.placement = placement;
        }
        match self.ai_sequences.placement {
            Placement::Fullscreen => {}
            Placement::Side => {
                ui.weak(t!("ai_sequence.placement_side_hint"));
            }
            _ => {
                ui.weak(t!("ai_sequence.placement_overlay_hint"));
            }
        }
    }

    fn show_ai_generate(&mut self, ui: &mut egui::Ui) {
        let project_path = self.session.path().map(Path::to_path_buf);
        let mut import = None;
        let mut revise = None;
        let mut retry = None;
        ui.weak(t!("ai_sequence.intro"));

        if let Some(parent) = self.ai_sequences.parent.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(theme::ACCENT, t!("ai_sequence.revising", id = parent));
                if ui.small_button(t!("ai_sequence.new_composition")).clicked() {
                    self.ai_sequences.parent = None;
                }
            });
        }
        let revising = self.ai_sequences.parent.is_some();

        section_title(ui, "1", &t!("ai_sequence.section_what"));
        ui.add(
            egui::TextEdit::multiline(&mut self.ai_sequences.prompt)
                .desired_rows(3)
                .desired_width(ui.available_width())
                .hint_text(t!("ai_sequence.prompt_example")),
        );
        self.show_material_choice(ui);
        if !revising {
            self.show_transcript_choice(ui);
            section_title(ui, "2", &t!("ai_sequence.section_style"));
            self.show_style_cards(ui);
            section_title(ui, "3", &t!("ai_sequence.section_where"));
            self.show_placement_cards(ui);
        }

        ui.add_space(theme::SPACE_3);
        self.show_ai_runtime_status(ui);
        let can_generate = project_path.is_some()
            && self.timeline_id.is_some()
            && !self.ai_sequences.prompt.trim().is_empty();
        let generate = accent_button(
            ui,
            if revising {
                t!("ai_sequence.generate_revision").into_owned()
            } else {
                t!("ai_sequence.generate_infographic").into_owned()
            },
            can_generate,
        )
        .clicked();
        if project_path.is_none() {
            ui.weak(t!("ai_sequence.save_first"));
        }
        if let Some(error) = &self.ai_sequences.error {
            ui.colored_label(theme::ERROR, error);
        }

        ui.add_space(theme::SPACE_3);
        egui::CollapsingHeader::new(t!("ai_sequence.advanced").into_owned())
            .id_salt("ai_sequence_advanced")
            .show(ui, |ui| {
                self.show_ai_context_card(ui);
                if !revising {
                    ui.add_space(theme::SPACE_2);
                    ui.label(t!("ai_sequence.length"));
                    ui.horizontal_wrapped(|ui| {
                        for (length, key) in [
                            (Length::Auto, "ai_sequence.length_auto"),
                            (Length::Clip, "ai_sequence.length_clip"),
                            (Length::Custom, "ai_sequence.length_custom"),
                        ] {
                            ui.selectable_value(&mut self.ai_sequences.length, length, t!(key));
                        }
                        if self.ai_sequences.length == Length::Custom {
                            ui.add(
                                egui::DragValue::new(&mut self.ai_sequences.custom_seconds)
                                    .range(1.0..=600.0)
                                    .suffix(" s"),
                            );
                        }
                    });
                    if !self.ai_sequences.placement.is_overlay() {
                        ui.checkbox(
                            &mut self.ai_sequences.transparent,
                            t!("ai_sequence.transparent"),
                        );
                    }
                }
                ui.add_space(theme::SPACE_2);
                let model = if self.settings.ai_model.trim().is_empty() {
                    t!("ai_sequence.default_model")
                } else {
                    self.settings.ai_model.clone().into()
                };
                if ui
                    .small_button(format!("{} · {model}", self.settings.ai_provider.label()))
                    .on_hover_text(t!("ai_sequence.change_provider"))
                    .clicked()
                {
                    self.open_settings(settings_dialog::Section::AiProviders);
                }
            });

        let mut open_other_project = None;
        let history = project_path
            .as_ref()
            .and_then(|path| self.ai_sequences.queues.get(path))
            .map_or(0, |queue| queue.jobs.len());
        egui::CollapsingHeader::new(t!("ai_sequence.history", count = history).into_owned())
            .id_salt("ai_sequence_history")
            .show(ui, |ui| {
                open_other_project = self.show_other_ai_jobs(ui, project_path.as_deref());
                let Some(queue) = project_path
                    .as_ref()
                    .and_then(|path| self.ai_sequences.queues.get(path))
                else {
                    return;
                };
                for warning in &queue.warnings {
                    ui.colored_label(theme::ERROR, warning);
                }
                for job in queue.jobs.iter().rev() {
                    ui.push_id(&job.id, |ui| {
                        ui.group(|ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                if !job.stage.terminal() {
                                    ui.spinner();
                                }
                                ui.label(job.request.prompt.chars().take(60).collect::<String>());
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.weak(job.stage.label());
                                if queue.is_cancelling(&job.id) {
                                    ui.weak(t!("ai_sequence.cancelling"));
                                } else if !job.stage.terminal()
                                    && ui.small_button(t!("ai_sequence.cancel")).clicked()
                                {
                                    queue.cancel(&job.id);
                                }
                                if job.stage == Stage::Completed {
                                    if ui.small_button(t!("ai_sequence.import")).clicked() {
                                        import = Some(job.clone());
                                    }
                                    if ui.small_button(t!("ai_sequence.revise")).clicked() {
                                        revise = Some(job.id.clone());
                                    }
                                } else if job.stage.terminal()
                                    && ui.small_button(t!("ai_sequence.retry")).clicked()
                                {
                                    retry = Some(job.request.clone());
                                }
                            });
                            if let Some(error) = &job.error {
                                ui.collapsing(t!("ai_sequence.error_details"), |ui| {
                                    ui.label(error);
                                });
                            }
                            if !job.events.is_empty() {
                                ui.collapsing(t!("ai_sequence.provider_events"), |ui| {
                                    for event in &job.events {
                                        ui.label(event);
                                    }
                                });
                            }
                            if !job.usage_history.is_empty() {
                                ui.collapsing(t!("ai_sequence.usage_requests"), |ui| {
                                    for (index, usage) in job.usage_history.iter().enumerate() {
                                        ui.label(format!("{}: {}", index + 1, usage));
                                    }
                                });
                            } else if let Some(usage) = &job.usage {
                                ui.collapsing(t!("ai_sequence.usage"), |ui| {
                                    ui.label(usage.to_string());
                                });
                            }
                        });
                    });
                }
            });

        if let Some(path) = open_other_project {
            self.request_project_switch(project_io::ProjectSwitch::OpenPath(path));
            return;
        }
        if let Some(id) = revise {
            self.ai_sequences.parent = Some(id);
            self.ai_sequences.prompt.clear();
        }
        if let Some(job) = import {
            self.import_ai_sequence(&job);
        }
        if let Some(request) = retry {
            self.submit_ai_sequence(request);
        }
        if generate {
            match self.ai_sequence_request() {
                Ok(request) => {
                    self.submit_ai_sequence(request);
                    if self.ai_sequences.error.is_none() {
                        self.ai_sequences.prompt.clear();
                        self.ai_sequences.parent = None;
                        self.ai_sequences.material.clear();
                    }
                }
                Err(error) => self.ai_sequences.error = Some(error),
            }
        }
    }

    fn show_ai_runtime_status(&mut self, ui: &mut egui::Ui) {
        let mut retry = false;
        if let Some(setup) = &self.ai_sequences.setup {
            egui::Frame::new()
                .fill(theme::BG_RAISED)
                .corner_radius(theme::RADIUS_PANEL)
                .inner_margin(theme::SPACE_2)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.strong(t!("ai_sequence.setup_title"));
                    });
                    ui.weak(setup.step().label());
                    if !self.ai_sequences.waiting.is_empty() {
                        ui.label(t!(
                            "ai_sequence.setup_waiting",
                            count = self.ai_sequences.waiting.len()
                        ));
                    }
                });
            ui.add_space(theme::SPACE_2);
        } else if let Some(error) = &self.ai_sequences.setup_error {
            egui::Frame::new()
                .fill(theme::BG_RAISED)
                .stroke(egui::Stroke::new(1.0, theme::ERROR))
                .corner_radius(theme::RADIUS_PANEL)
                .inner_margin(theme::SPACE_2)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.colored_label(theme::ERROR, t!("ai_sequence.setup_failed"));
                    ui.label(error);
                    retry = ui.button(t!("ai_sequence.setup_retry")).clicked();
                });
            ui.add_space(theme::SPACE_2);
        }
        if retry {
            self.ensure_ai_runtime(true);
        }
    }

    /// Starts the automatic runtime install when the configured one is missing.
    fn ensure_ai_runtime(&mut self, retry: bool) -> bool {
        if setup::ready(&self.settings.ai_runtime) {
            return true;
        }
        if self.ai_sequences.setup.is_none() && (retry || self.ai_sequences.setup_error.is_none()) {
            self.ai_sequences.setup_error = None;
            match setup::default_directory() {
                Some(directory) => self.ai_sequences.setup = Some(Setup::start(directory)),
                None => {
                    self.ai_sequences.setup_error =
                        Some("Cannot locate the user data directory".into())
                }
            }
        }
        false
    }

    fn poll_ai_runtime_setup(&mut self, ctx: &egui::Context) {
        let Some(setup) = &self.ai_sequences.setup else {
            return;
        };
        let Some(result) = setup.poll() else {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
            return;
        };
        self.ai_sequences.setup = None;
        match result {
            Ok(runtime) => {
                self.settings.ai_runtime = runtime;
                self.persist_settings();
                self.toast(t!("ai_sequence.setup_done"));
                for request in std::mem::take(&mut self.ai_sequences.waiting) {
                    self.submit_ai_sequence(request);
                }
            }
            Err(error) => {
                self.ai_sequences.waiting.clear();
                self.ai_sequences.setup_error = Some(error);
            }
        }
    }

    fn show_transcript_choice(&mut self, ui: &mut egui::Ui) {
        let range = if self.timeline_id.is_some() {
            self.ai_range()
        } else {
            Ok(None)
        };
        match range {
            Ok(Some(range)) => {
                let mut use_transcript = !self.ai_sequences.no_transcript;
                if ui
                    .checkbox(&mut use_transcript, t!("ai_sequence.use_transcript"))
                    .changed()
                {
                    self.ai_sequences.no_transcript = !use_transcript;
                }
                let seconds = self.ai_range_seconds(&range);
                let source = if range.from_marks {
                    t!("ai_sequence.context_marks")
                } else if range.clips == 1 {
                    t!("ai_sequence.context_clip")
                } else {
                    t!("ai_sequence.context_clips", count = range.clips)
                };
                ui.weak(format!("{source} · {seconds:.1} s"));
            }
            Ok(None) => {
                ui.weak(if self.timeline_id.is_some() {
                    t!("ai_sequence.context_none")
                } else {
                    t!("ai_sequence.context_no_timeline")
                });
            }
            Err(error) => {
                ui.colored_label(theme::ERROR, error);
            }
        }
    }

    fn show_ai_context_card(&mut self, ui: &mut egui::Ui) {
        let range = if self.timeline_id.is_some() {
            self.ai_range()
        } else {
            Ok(None)
        };
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.strong(t!("ai_sequence.context_title"));
            match &range {
                Ok(Some(range)) => {
                    let seconds = self.ai_range_seconds(range);
                    let words = self.ai_range_words(range);
                    let source = if range.from_marks {
                        t!("ai_sequence.context_marks")
                    } else if range.clips == 1 {
                        t!("ai_sequence.context_clip")
                    } else {
                        t!("ai_sequence.context_clips", count = range.clips)
                    };
                    ui.label(format!("{source} · {seconds:.1} s"));
                    ui.weak(t!("ai_sequence.context_words", count = words));
                    ui.horizontal_wrapped(|ui| {
                        let mut frames = !self.ai_sequences.no_frames;
                        if ui
                            .selectable_label(frames, t!("ai_sequence.chip_frames"))
                            .clicked()
                        {
                            frames = !frames;
                            self.ai_sequences.no_frames = !frames;
                        }
                        let mut audio = !self.ai_sequences.no_audio;
                        let hint = if self.settings.ai_provider == Provider::Gemini {
                            t!("ai_sequence.chip_audio_hint")
                        } else {
                            t!("ai_sequence.chip_audio_gemini")
                        };
                        if ui
                            .selectable_label(audio, t!("ai_sequence.chip_audio"))
                            .on_hover_text(hint)
                            .clicked()
                        {
                            audio = !audio;
                            self.ai_sequences.no_audio = !audio;
                        }
                    });
                }
                Ok(None) => {
                    ui.weak(if self.timeline_id.is_some() {
                        t!("ai_sequence.context_none")
                    } else {
                        t!("ai_sequence.context_no_timeline")
                    });
                }
                Err(error) => {
                    ui.colored_label(theme::ERROR, error);
                }
            }
            if ui
                .selectable_label(self.ai_sequences.use_marks, t!("ai_sequence.marks"))
                .clicked()
            {
                self.ai_sequences.use_marks = !self.ai_sequences.use_marks;
            }
        });
    }

    fn ai_range_seconds(&self, range: &Range) -> f64 {
        let fps = self
            .timeline_id
            .map_or(30.0, |id| self.session.project.timelines[id].fps.as_f64());
        (range.end - range.start) as f64 / fps
    }

    fn ai_range_words(&self, range: &Range) -> usize {
        let Some(timeline_id) = self.timeline_id else {
            return 0;
        };
        let fps = self.session.project.timelines[timeline_id].fps.as_f64();
        let start_ms = (range.start as f64 / fps * 1000.0) as u64;
        let end_ms = (range.end as f64 / fps * 1000.0) as u64;
        self.session
            .project
            .captions
            .get(&timeline_id)
            .map_or(0, |captions| {
                captions
                    .words
                    .iter()
                    .filter(|w| u64::from(w.end_ms) > start_ms && u64::from(w.start_ms) < end_ms)
                    .count()
            })
    }

    /// The frames the request draws on: the In/Out range if enabled, otherwise the
    /// selected clips. `None` means a free animation without source context.
    fn ai_range(&self) -> Result<Option<Range>, String> {
        let timeline_id = self.timeline_id.ok_or("Open a timeline first")?;
        let timeline = &self.session.project.timelines[timeline_id];
        let anchor_for = |clip: &vv_core::Clip, frame: i64| {
            (clip.freeze.is_none()).then(|| crate::ai_sequence::Anchor {
                source_frame: frame,
                rate: clip.rate(),
                media: match clip.source {
                    vv_core::ClipSource::Media(id) => Some(id.raw()),
                    _ => None,
                },
            })
        };
        if self.ai_sequences.use_marks {
            let (start, end) = self
                .timeline_state
                .export_marks
                .resolve(timeline.total_frames());
            if start >= end {
                return Err("Set a nonempty timeline In/Out range first".into());
            }
            let clip = timeline.active_video_clip_at(start).map(|(_, clip)| clip);
            return Ok(Some(Range {
                start,
                end,
                anchor: clip.map(|clip| clip.id.0),
                anchor_details: clip.and_then(|clip| anchor_for(clip, clip.source_frame_at(start))),
                clips: 0,
                from_marks: true,
            }));
        }
        let selected: Vec<_> = self
            .timeline_state
            .selected
            .iter()
            .filter_map(|(track, id)| {
                let track = timeline.tracks.get(*track)?;
                let clip = track.clips.iter().find(|c| c.id == *id)?;
                Some((track.kind, clip))
            })
            .collect();
        let clips: Vec<_> = selected.iter().map(|(_, clip)| *clip).collect();
        let video_clips = selected
            .iter()
            .filter(|(kind, _)| *kind == TrackKind::Video)
            .count();
        let Some(first) = clips.iter().min_by_key(|c| c.timeline_start) else {
            return Ok(None);
        };
        Ok(Some(Range {
            start: first.timeline_start,
            end: clips
                .iter()
                .map(|c| c.timeline_start + c.timeline_len)
                .max()
                .unwrap_or(first.timeline_start),
            anchor: Some(first.id.0),
            anchor_details: anchor_for(first, first.source_in()),
            clips: if video_clips > 0 {
                video_clips
            } else {
                clips.len()
            },
            from_marks: false,
        }))
    }

    fn ai_analysis_range(&self) -> Result<Option<Range>, String> {
        if self.ai_sequences.scope == Scope::Selection {
            return self.ai_range();
        }
        let timeline_id = self.timeline_id.ok_or("Open a timeline first")?;
        let timeline = &self.session.project.timelines[timeline_id];
        let end = timeline.total_frames();
        if end <= 0 {
            return Ok(None);
        }
        let clips = timeline
            .tracks
            .iter()
            .filter(|track| track.kind == TrackKind::Video)
            .map(|track| track.clips.len())
            .sum();
        Ok(Some(Range {
            start: 0,
            end,
            anchor: None,
            anchor_details: None,
            clips,
            from_marks: false,
        }))
    }

    fn submit_ai_sequence(&mut self, mut request: Request) -> Option<String> {
        if self.ai_sequences.source_copy.is_some() {
            self.ai_sequences.error = Some(
                "AI sources are being copied for Save As; wait before starting another AI job"
                    .into(),
            );
            return None;
        }
        request.api_access = self.settings.sequence_api_access();
        if request.provider == Provider::Gemini {
            let allowed = crate::gemini::configured_key()
                .and_then(|key| request.api_access.check(&key, &request.model));
            if let Err(error) = allowed {
                self.ai_sequences.error = Some(error);
                return None;
            }
        }
        if !self.ensure_ai_runtime(true) {
            self.ai_sequences.waiting.push(request);
            self.ai_sequences.error = None;
            self.toast(t!("ai_sequence.setup_queued"));
            return None;
        }
        let result = self
            .session
            .path()
            .and_then(|p| self.ai_sequences.queues.get_mut(p))
            .ok_or("Save and open the AI panel first".to_owned())
            .and_then(|q| q.submit(request, self.settings.ai_runtime.clone()));
        match result {
            Ok(id) => {
                self.ai_sequences.error = None;
                Some(id)
            }
            Err(error) => {
                self.ai_sequences.error = Some(error);
                None
            }
        }
    }

    fn ai_sequence_request(&self) -> Result<Request, String> {
        if let Some(parent) = &self.ai_sequences.parent {
            let queue = self
                .session
                .path()
                .and_then(|path| self.ai_sequences.queues.get(path))
                .ok_or("Open the project sequence history first")?;
            let previous = queue
                .jobs
                .iter()
                .find(|job| &job.id == parent)
                .ok_or("Previous sequence is missing")?;
            let mut request = previous.request.clone();
            request.parent = Some(parent.clone());
            request.prompt = self.ai_sequences.prompt.clone();
            request.provider = self.settings.ai_provider;
            request.model = self.settings.ai_model.clone();
            request.insert = false;
            request.reuse_session = !self.ai_sequences.fresh_session;
            request.limits = self.settings.ai_limits.clone();
            request.render_only = false;
            request.material = self.material_request();
            request.ask_material = !self.ai_sequences.no_material_requests;
            return Ok(request);
        }
        self.request_for(
            self.ai_range()?,
            self.ai_sequences.length,
            &self.ai_sequences.prompt,
        )
    }

    fn request_for(
        &self,
        range: Option<Range>,
        length: Length,
        prompt: &str,
    ) -> Result<Request, String> {
        let timeline_id = self.timeline_id.ok_or("Open a timeline first")?;
        let timeline = &self.session.project.timelines[timeline_id];
        let selection = range.is_some();
        let (start, end, anchor, anchor_details) = match range {
            Some(range) => (range.start, range.end, range.anchor, range.anchor_details),
            None => (
                self.timeline_state.playhead,
                self.timeline_state.playhead,
                None,
                None,
            ),
        };
        let fps = timeline.fps.as_f64();
        let auto_duration = length == Length::Auto;
        let seconds = match length {
            Length::Auto => AUTO_MAX_SECONDS,
            Length::Custom => self.ai_sequences.custom_seconds.max(1.0),
            Length::Clip if end > start => (end - start) as f64 / fps,
            Length::Clip => NO_CONTEXT_SECONDS,
        };
        let context_end = if selection {
            end.min(start + (seconds * fps).round() as i64)
        } else {
            start
        };
        let end_ms = (context_end as f64 / fps * 1000.0) as u64;
        let start_ms = (start as f64 / fps * 1000.0) as u64;
        let transcript = !self.ai_sequences.no_transcript;
        let words: Vec<_> = if selection && transcript {
            self.session.project.captions.get(&timeline_id).map(|c| c.words.iter().filter(|w| u64::from(w.end_ms) > start_ms && u64::from(w.start_ms) < end_ms).map(|w| serde_json::json!({"start_ms":u64::from(w.start_ms).saturating_sub(start_ms), "end_ms":u64::from(w.end_ms).min(end_ms).saturating_sub(start_ms), "text":w.text})).collect()).unwrap_or_default()
        } else {
            Vec::new()
        };
        let media = if selection {
            let mut media = crate::ai_sequence::context::snapshot(
                &self.session.project,
                timeline_id,
                start,
                context_end,
                !self.ai_sequences.no_frames,
                !self.ai_sequences.no_audio,
            );
            if !transcript {
                media.retain(|source| {
                    !matches!(
                        source,
                        crate::ai_sequence::context::Source::Transcript { .. }
                    )
                });
            }
            media
        } else {
            Vec::new()
        };
        Ok(Request {
            prompt: prompt.to_owned(),
            context: serde_json::to_string(&words).map_err(|e| e.to_string())?,
            style: self.ai_style(),
            placement: self.ai_sequences.placement,
            media,
            material: self.material_request(),
            ask_material: !self.ai_sequences.no_material_requests,
            render_only: false,
            provider: self.settings.ai_provider,
            api_access: self.settings.sequence_api_access(),
            model: self.settings.ai_model.clone(),
            limits: self.settings.ai_limits.clone(),
            composition: Composition {
                width: timeline.resolution.0,
                height: timeline.resolution.1,
                fps,
                frames: (seconds * fps).round() as u32,
                transparent: self.ai_sequences.transparent
                    || self.ai_sequences.placement.is_overlay(),
            },
            parent: self.ai_sequences.parent.clone(),
            reuse_session: !self.ai_sequences.fresh_session,
            insert: true,
            auto_duration,
            timeline: Some(timeline_id.raw()),
            anchor_clip: anchor,
            anchor_details,
            anchor_offset: 0,
            start,
        })
    }

    fn finish_ai_imports(&mut self) {
        let Some(path) = self.session.path().map(Path::to_path_buf) else {
            return;
        };
        let ready: Vec<_> = self
            .ai_sequences
            .pending_imports
            .iter()
            .filter(|(_, (project, _))| *project == path)
            .filter_map(|(id, (_, job))| {
                let queue = self.ai_sequences.queues.get(&path)?;
                let file = queue
                    .directory(id)
                    .ok()?
                    .join(if job.request.composition.transparent {
                        "result.mov"
                    } else {
                        "result.mp4"
                    });
                self.ai_media_for_version(job)
                    .or_else(|| self.session.media_with_path(&file))
                    .map(|media| (id.clone(), job.clone(), media))
            })
            .collect();
        for (id, job, media) in ready {
            let import_group = self.session.history.begin_group();
            if self.session.project.ai_sequences.get(&media) != Some(&id) {
                self.session
                    .apply(Box::new(vv_core::SetAiSequence::new(media, id.clone())));
            }
            let already_used = self.session.project.timelines.iter().any(|(_, timeline)| {
                timeline.tracks.iter().any(|track| {
                    track.clips.iter().any(
                        |clip| matches!(clip.source, vv_core::ClipSource::Media(id) if id == media),
                    )
                })
            });
            if job.request.insert
                && job.request.parent.is_none()
                && !job.imported
                && !already_used
                && let Some(timeline_id) = job.request.timeline.map(TimelineId::from_raw)
                && let Some(timeline) = self.session.project.timelines.get(timeline_id)
            {
                let position = crate::ai_sequence::insertion::position(timeline, &job.request);
                if let Ok(start) = position {
                    let meta = self.session.project.media_pool[media].meta.clone();
                    let group = self.session.history.begin_group();
                    let track = vv_core::edit::add_track(
                        &mut self.session.project,
                        &mut self.session.history,
                        timeline_id,
                        TrackKind::Video,
                    );
                    self.insert_media_clip(
                        timeline_id,
                        timeline_ui::MediaDrag::whole(media, &meta),
                        &meta,
                        start.max(0),
                        TargetTracks {
                            video: Some(track),
                            extra_audio: None,
                        },
                    );
                    self.session.history.end_group(group);
                } else {
                    self.project_error = Some(format!(
                        "AI sequence imported. {}. Place the result from the Media Library.",
                        position.unwrap_err()
                    ));
                }
            }
            if let Some(target) = self.ai_sequences.pending_applies.remove(&id) {
                match crate::ai_sequence::revision::replacement(
                    &self.session.project,
                    target.timeline,
                    target.clip,
                    target.expected,
                    media,
                ) {
                    Ok(command) => self.session.apply(command),
                    Err(error) => self.ai_sequences.error = Some(error),
                }
            }
            self.session.history.end_group(import_group);
            self.ai_sequences.pending_imports.remove(&id);
            if let Some(queue) = self.ai_sequences.queues.get_mut(&path)
                && let Err(error) = queue.mark_imported(&id)
            {
                self.ai_sequences.error = Some(error);
            }
        }
    }

    fn import_ai_sequence(&mut self, job: &Job) {
        if self.ai_sequences.source_copy.is_some() {
            self.ai_sequences.error = Some(
                "Wait for the AI source copy to finish before importing another sequence".into(),
            );
            return;
        }
        let Some(path) = self.session.path().map(Path::to_path_buf) else {
            return;
        };
        if self.ai_media_for_version(job).is_some() {
            self.ai_sequences
                .pending_imports
                .insert(job.id.clone(), (path, job.clone()));
            return;
        }
        let Some(queue) = self.ai_sequences.queues.get_mut(&path) else {
            return;
        };
        match queue.directory(&job.id) {
            Ok(directory) => {
                let output = directory.join(if job.request.composition.transparent {
                    "result.mov"
                } else {
                    "result.mp4"
                });
                self.ai_sequences
                    .pending_imports
                    .insert(job.id.clone(), (path, job.clone()));
                if !output.is_file() {
                    self.ai_sequences.pending_imports.remove(&job.id);
                    self.ai_sequences.pending_applies.remove(&job.id);
                    self.ai_sequences.error=Some("The rendered AI media file is missing; retry generation or restore the project files".into());
                    return;
                }
                let import = self.session.import_media(vec![output]);
                self.ai_sequences
                    .import_jobs
                    .insert((self.session.epoch(), import), job.id.clone());
            }
            Err(error) => {
                self.ai_sequences.pending_applies.remove(&job.id);
                self.ai_sequences.error = Some(error);
            }
        }
    }
}

impl VenturiApp {
    pub(crate) fn show_ai_properties(
        &mut self,
        ui: &mut egui::Ui,
        target: PanelTarget,
        commands: &mut Vec<BoxedCommand>,
    ) {
        let Some(project_path) = self.session.path().map(Path::to_path_buf) else {
            return;
        };
        let Some(clip) = self
            .session
            .project
            .timelines
            .get(target.timeline)
            .and_then(|t| t.clip(target.track_index, target.clip_id))
        else {
            return;
        };
        let vv_core::ClipSource::Media(current_media) = clip.source else {
            return;
        };
        let Some(item) = self.session.project.media_pool.get(current_media) else {
            return;
        };
        let Some(queue) = self.ai_sequences.queues.get(&project_path) else {
            return;
        };
        let Some(current) = queue
            .jobs
            .iter()
            .find(|job| {
                self.session
                    .project
                    .ai_sequences
                    .get(&current_media)
                    .is_some_and(|id| id == &job.id)
                    || queue.directory(&job.id).ok().is_some_and(|dir| {
                        item.path
                            == dir.join(if job.request.composition.transparent {
                                "result.mov"
                            } else {
                                "result.mp4"
                            })
                    })
            })
            .cloned()
        else {
            if self
                .session
                .project
                .ai_sequences
                .contains_key(&current_media)
            {
                ui.collapsing("AI", |ui| {
                    ui.colored_label(theme::ERROR, t!("ai_sequence.missing_sources"));
                });
            }
            return;
        };
        let root = crate::ai_sequence::revision::family(&queue.jobs, &current.id).to_owned();
        let versions: Vec<_> = queue
            .jobs
            .iter()
            .filter(|job| crate::ai_sequence::revision::family(&queue.jobs, &job.id) == root)
            .cloned()
            .collect();
        let busy = versions.iter().any(|version| {
            !version.stage.terminal() || self.ai_sequences.pending_applies.contains_key(&version.id)
        });
        let last_failed = versions
            .last()
            .is_some_and(|version| version.stage == Stage::Failed);
        let improvements: Vec<_> = current
            .improvements
            .iter()
            .filter(|idea| {
                !versions.iter().any(|version| {
                    version.stage == Stage::Completed
                        && version
                            .request
                            .prompt
                            .trim()
                            .eq_ignore_ascii_case(idea.instruction.trim())
                })
            })
            .cloned()
            .collect();
        let mut typed = false;
        let mut chosen: Option<String> = None;
        let mut preview = None;
        let mut apply = None;
        egui::CollapsingHeader::new("AI")
            .default_open(true)
            .show(ui, |ui| {
                ui.strong(t!("ai_sequence.edit_title"));
                ui.weak(t!("ai_sequence.edit_subtitle"));
                ui.add_enabled(
                    !busy,
                    egui::TextEdit::multiline(&mut self.ai_sequences.revision_prompt)
                        .desired_rows(3)
                        .desired_width(ui.available_width())
                        .hint_text(t!("ai_sequence.revision_hint")),
                );
                typed = accent_button(
                    ui,
                    t!("ai_sequence.apply_change").into_owned(),
                    !busy && !self.ai_sequences.revision_prompt.trim().is_empty(),
                )
                .clicked();
                if busy {
                    ui.horizontal_wrapped(|ui| {
                        ui.spinner();
                        ui.label(t!("ai_sequence.editing"));
                    });
                } else if last_failed {
                    ui.colored_label(theme::ERROR, t!("ai_sequence.edit_failed"));
                }
                self.show_slots(ui, &current, target, current_media, busy);
                if !improvements.is_empty() {
                    ui.add_space(theme::SPACE_2);
                    ui.strong(t!("ai_sequence.or_try"));
                    for idea in &improvements {
                        if improvement_card(ui, idea, !busy)
                            .on_hover_text(t!("ai_sequence.suggestion_hover"))
                            .clicked()
                            && !busy
                        {
                            chosen = Some(idea.instruction.clone());
                        }
                    }
                }
                ui.add_space(theme::SPACE_2);
                ui.collapsing(t!("ai_sequence.edit_details"), |ui| {
                    ui.label(t!("ai_sequence.current_revision", id = &current.id));
                    ui.label(if current.request.model.is_empty() {
                        current.request.provider.label().to_owned()
                    } else {
                        format!(
                            "{} · {}",
                            current.request.provider.label(),
                            current.request.model
                        )
                    });
                    ui.checkbox(
                        &mut self.ai_sequences.fresh_session,
                        t!("ai_sequence.fresh_session"),
                    );
                    ui.small(t!("ai_sequence.revision_provider"));
                    for version in &versions {
                        ui.push_id(&version.id, |ui| {
                            ui.horizontal(|ui| {
                                if !version.stage.terminal() {
                                    ui.spinner();
                                }
                                ui.label(format!(
                                    "{}: {}",
                                    version.stage.label(),
                                    version.request.prompt.chars().take(55).collect::<String>()
                                ));
                            });
                            if let Some(error) = &version.error {
                                ui.colored_label(theme::ERROR, error);
                            }
                            if version.stage == Stage::Completed && version.id != current.id {
                                ui.horizontal(|ui| {
                                    if ui.small_button(t!("ai_sequence.preview")).clicked() {
                                        preview = Some(version.clone());
                                    }
                                    if ui.small_button(t!("ai_sequence.apply_version")).clicked() {
                                        apply = Some(version.clone());
                                    }
                                });
                            }
                        });
                    }
                });
                if let Some(error) = &self.ai_sequences.error {
                    ui.colored_label(theme::ERROR, error);
                }
            });
        let revision = if typed {
            Some((self.ai_sequences.revision_prompt.clone(), true))
        } else {
            chosen.map(|instruction| (instruction, false))
        };
        if let Some((prompt, from_text)) = revision
            && !busy
            && self
                .start_ai_revision(&current, target, current_media, prompt)
                .is_some()
            && from_text
        {
            self.ai_sequences.revision_prompt.clear();
        }
        for (version, should_apply) in preview
            .into_iter()
            .map(|v| (v, false))
            .chain(apply.into_iter().map(|v| (v, true)))
        {
            let Some(queue) = self.ai_sequences.queues.get(&project_path) else {
                continue;
            };
            let Ok(directory) = queue.directory(&version.id) else {
                continue;
            };
            let file = directory.join(if version.request.composition.transparent {
                "result.mov"
            } else {
                "result.mp4"
            });
            if let Some(media) = self
                .ai_media_for_version(&version)
                .or_else(|| self.session.media_with_path(&file))
            {
                if should_apply {
                    match crate::ai_sequence::revision::replacement(
                        &self.session.project,
                        target.timeline,
                        target.clip_id,
                        current_media,
                        media,
                    ) {
                        Ok(command) => commands.push(command),
                        Err(error) => self.ai_sequences.error = Some(error),
                    }
                } else {
                    self.preview_media(media);
                }
            } else {
                self.import_ai_sequence(&version);
                self.ai_sequences.error = Some("Importing the revision; select Preview or Apply again when it appears in the Media Library".into());
            }
        }
    }
}

impl VenturiApp {
    fn start_ai_revision(
        &mut self,
        current: &Job,
        target: PanelTarget,
        media: MediaId,
        prompt: String,
    ) -> Option<String> {
        let mut request = current.request.clone();
        request.parent = Some(current.id.clone());
        request.reuse_session = !self.ai_sequences.fresh_session;
        request.limits = self.settings.ai_limits.clone();
        request.prompt = prompt;
        request.provider = self.settings.ai_provider;
        request.model = self.settings.ai_model.clone();
        request.insert = false;
        request.render_only = false;
        request.material.clear();
        request.ask_material = !self.ai_sequences.no_material_requests;
        // The clip keeps its length: the new media must still cover the current trim.
        request.auto_duration = false;
        request.timeline = Some(target.timeline.raw());
        request.anchor_clip = Some(target.clip_id.0);
        let id = self.submit_ai_sequence(request)?;
        self.ai_sequences.pending_applies.insert(
            id.clone(),
            RevisionTarget {
                timeline: target.timeline,
                clip: target.clip_id,
                expected: media,
            },
        );
        Some(id)
    }

    fn show_ai_suggestions(&mut self, ui: &mut egui::Ui) {
        let mut analyze = false;
        let mut generate = false;
        let mut cancel = false;
        ui.weak(t!("ai_sequence.suggestions_intro"));

        section_title(ui, "1", &t!("ai_sequence.scope_title"));
        let mut chosen = None;
        card_strip(ui, "ai_scope", |ui| {
            for (scope, key, art) in [
                (
                    Scope::Selection,
                    "ai_sequence.scope_selection",
                    crate::ai_sequence_cards::art_scope_clip as Art,
                ),
                (
                    Scope::Timeline,
                    "ai_sequence.scope_timeline",
                    crate::ai_sequence_cards::art_scope_timeline,
                ),
            ] {
                if choice_card(ui, self.ai_sequences.scope == scope, &t!(key), art).clicked() {
                    chosen = Some(scope);
                }
            }
        });
        if let Some(scope) = chosen {
            self.ai_sequences.scope = scope;
        }
        let range = if self.timeline_id.is_some() {
            self.ai_analysis_range()
        } else {
            Ok(None)
        };
        let has_range = matches!(range, Ok(Some(_)));
        match &range {
            Ok(Some(range)) => {
                let source = if self.ai_sequences.scope == Scope::Timeline {
                    t!("ai_sequence.scope_timeline")
                } else if range.from_marks {
                    t!("ai_sequence.context_marks")
                } else if range.clips == 1 {
                    t!("ai_sequence.context_clip")
                } else {
                    t!("ai_sequence.context_clips", count = range.clips)
                };
                ui.weak(format!(
                    "{source} · {:.1} s · {}",
                    self.ai_range_seconds(range),
                    t!(
                        "ai_sequence.context_words",
                        count = self.ai_range_words(range)
                    )
                ));
            }
            Ok(None) => {
                ui.colored_label(
                    theme::WARNING,
                    if self.timeline_id.is_some() {
                        t!("ai_sequence.scope_select_clip")
                    } else {
                        t!("ai_sequence.context_no_timeline")
                    },
                );
            }
            Err(error) => {
                ui.colored_label(theme::ERROR, error);
            }
        }

        section_title(ui, "2", &t!("ai_sequence.focus_title"));
        ui.add(
            egui::TextEdit::multiline(&mut self.ai_sequences.analysis_prompt)
                .desired_rows(2)
                .desired_width(ui.available_width())
                .hint_text(t!("ai_sequence.focus_hint")),
        );
        ui.add_space(theme::SPACE_1);
        ui.horizontal_wrapped(|ui| {
            ui.label(t!("ai_sequence.analysis_with"));
            let previous_provider = self.ai_sequences.analysis_provider;
            egui::ComboBox::from_id_salt("analysis_provider")
                .selected_text(self.ai_sequences.analysis_provider.label())
                .show_ui(ui, |ui| {
                    for provider in Provider::ALL {
                        ui.selectable_value(
                            &mut self.ai_sequences.analysis_provider,
                            provider,
                            provider.label(),
                        );
                    }
                });
            if previous_provider != self.ai_sequences.analysis_provider {
                self.ai_sequences.analysis_model.clear();
            }
            if self.ai_sequences.analysis_provider == Provider::Gemini
                && self.ai_sequences.analysis_model.is_empty()
            {
                self.ai_sequences.analysis_model = "gemini-4-hydrogen".into();
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.ai_sequences.analysis_model)
                    .desired_width(150.0)
                    .hint_text(t!("ai_sequence.analysis_model")),
            );
        });
        ui.add_space(theme::SPACE_2);
        if self.ai_sequences.analysis.is_some() {
            egui::Frame::new()
                .fill(theme::BG_RAISED)
                .corner_radius(theme::RADIUS_PANEL)
                .inner_margin(theme::SPACE_2)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(t!("ai_sequence.analyzing"));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            cancel = ui.button(t!("ai_sequence.cancel_analysis")).clicked();
                        });
                    });
                });
        } else {
            analyze = accent_button(
                ui,
                t!("ai_sequence.analyze").into_owned(),
                has_range && self.session.path().is_some(),
            )
            .clicked();
            ui.checkbox(
                &mut self.ai_sequences.refresh_analysis,
                t!("ai_sequence.refresh_analysis"),
            );
            let mut ask = !self.ai_sequences.no_suggestion_material;
            if ui
                .checkbox(&mut ask, t!("ai_sequence.material_suggest"))
                .on_hover_text(t!("ai_sequence.material_ask_hint"))
                .changed()
            {
                self.ai_sequences.no_suggestion_material = !ask;
            }
        }
        if let Some(error) = &self.ai_sequences.error {
            ui.colored_label(theme::ERROR, error);
        }

        let current_project = self.ai_sequences.analysis_project.as_deref() == self.session.path();
        if current_project && !self.ai_sequences.suggestions.is_empty() {
            let selected = self
                .ai_sequences
                .suggestions
                .iter()
                .filter(|s| s.selected)
                .count();
            section_title(
                ui,
                "3",
                &t!(
                    "ai_sequence.ideas_found",
                    count = self.ai_sequences.suggestions.len()
                ),
            );
            ui.horizontal_wrapped(|ui| {
                let all = selected == self.ai_sequences.suggestions.len();
                if ui
                    .small_button(if all {
                        t!("ai_sequence.select_none")
                    } else {
                        t!("ai_sequence.select_all")
                    })
                    .clicked()
                {
                    for suggestion in &mut self.ai_sequences.suggestions {
                        suggestion.selected = !all;
                    }
                }
                if selected > 0
                    && selected < self.ai_sequences.suggestions.len()
                    && ui.small_button(t!("ai_sequence.discard")).clicked()
                {
                    self.ai_sequences.suggestions.retain(|s| s.selected);
                }
            });
            ui.add_space(theme::SPACE_1);
            let base = self.ai_sequences.analysis_base.as_ref();
            let jump_allowed =
                base.is_some_and(|base| base.timeline == self.timeline_id.map(Id::raw));
            let mut jump = None;
            let mut pick = None;
            let picking = self.ai_sequences.material_pick.is_some();
            for (index, suggestion) in self.ai_sequences.suggestions.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    // Not BG_RAISED: the unchecked checkbox would vanish into it.
                    egui::Frame::new()
                        .fill(theme::BG_PANEL)
                        .stroke(egui::Stroke::new(
                            1.0,
                            if suggestion.selected {
                                theme::ACCENT
                            } else {
                                theme::BORDER_SUBTLE
                            },
                        ))
                        .corner_radius(theme::RADIUS_PANEL)
                        .inner_margin(theme::SPACE_2)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.checkbox(
                                    &mut suggestion.selected,
                                    egui::RichText::new(t!("ai_sequence.use_idea")).strong(),
                                );
                                let time = format!(
                                    "▶ {:.1}–{:.1} s",
                                    suggestion.start_ms as f64 / 1000.0,
                                    suggestion.end_ms as f64 / 1000.0
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add_enabled(
                                                jump_allowed,
                                                egui::Button::new(time).small(),
                                            )
                                            .on_hover_text(t!("ai_sequence.jump_to"))
                                            .clicked()
                                        {
                                            jump = Some(suggestion.start_ms);
                                        }
                                    },
                                );
                            });
                            ui.add(
                                egui::TextEdit::multiline(&mut suggestion.description)
                                    .desired_rows(2)
                                    .desired_width(ui.available_width()),
                            );
                            if !suggestion.reason.trim().is_empty() {
                                ui.label(
                                    egui::RichText::new(&suggestion.reason)
                                        .small()
                                        .color(theme::TEXT_SECONDARY),
                                );
                            }
                            if let Some(target) =
                                Self::show_suggestion_material(ui, suggestion, index, picking)
                            {
                                pick = Some(target);
                            }
                        });
                });
                ui.add_space(theme::SPACE_1);
            }
            if let Some(target) = pick {
                self.pick_material(target);
            }
            if let Some(start_ms) = jump
                && let Some(base) = &self.ai_sequences.analysis_base
            {
                self.timeline_state.playhead =
                    base.start + (start_ms as f64 * base.composition.fps / 1000.0).round() as i64;
            }

            ui.add_space(theme::SPACE_2);
            let look = LOOKS
                .iter()
                .find(|look| look.prompt == self.ai_style())
                .map_or_else(|| t!("ai_sequence.look_custom"), |look| t!(look.label));
            ui.horizontal_wrapped(|ui| {
                ui.weak(t!(
                    "ai_sequence.uses_settings",
                    style = look,
                    placement = t!(placement_key(self.ai_sequences.placement))
                ));
                if ui.small_button(t!("ai_sequence.change")).clicked() {
                    self.ai_sequences.tab = PanelTab::Generate;
                }
            });
            self.show_ai_runtime_status(ui);
            generate = accent_button(
                ui,
                t!("ai_sequence.generate_count", count = selected).into_owned(),
                selected > 0,
            )
            .clicked();
            if let Some(usage) = &self.ai_sequences.analysis_usage {
                ui.collapsing(t!("ai_sequence.analysis_usage"), |ui| {
                    ui.label(usage.to_string());
                });
            }
        }

        if cancel && let Some(analysis) = &self.ai_sequences.analysis {
            analysis.cancel();
        }
        if analyze {
            self.start_ai_analysis(range);
        }
        if generate && let Some(mut base) = self.ai_sequences.analysis_base.clone() {
            base.style = self.ai_style();
            base.placement = self.ai_sequences.placement;
            base.composition.transparent =
                self.ai_sequences.transparent || self.ai_sequences.placement.is_overlay();
            base.ask_material = !self.ai_sequences.no_suggestion_material;
            let selected: Vec<_> = self
                .ai_sequences
                .suggestions
                .iter()
                .filter(|s| s.selected)
                .cloned()
                .collect();
            for suggestion in selected {
                match crate::ai_sequence::suggestions::generation_request(
                    &base,
                    &suggestion,
                    self.settings.ai_provider,
                    self.settings.ai_model.clone(),
                ) {
                    Ok(request) => {
                        self.submit_ai_sequence(request);
                    }
                    Err(error) => self.ai_sequences.error = Some(error),
                }
            }
            self.ai_sequences.suggestions.retain(|s| !s.selected);
        }
    }

    fn start_ai_analysis(&mut self, range: Result<Option<Range>, String>) {
        let analysis_prompt = self.ai_sequences.analysis_prompt.clone();
        let result =
            range.and_then(|range| self.request_for(range, Length::Clip, &analysis_prompt));
        let mut request = match result {
            Ok(request) => request,
            Err(error) => {
                self.ai_sequences.error = Some(error);
                return;
            }
        };
        // Style is applied at generation time, so it must not split the analysis cache.
        request.style.clear();
        request.material.clear();
        request.ask_material = !self.ai_sequences.no_suggestion_material;
        request.provider = self.ai_sequences.analysis_provider;
        request.model = self.ai_sequences.analysis_model.clone();
        request.api_access = self.settings.sequence_api_access();
        if request.provider == Provider::Gemini && request.model.trim().is_empty() {
            self.ai_sequences.error = Some("Select a Gemini analysis model first".into());
            return;
        }
        let Some(path) = self.session.path().map(Path::to_path_buf) else {
            return;
        };
        let directory = crate::ai_sequence::portable::directory(&path).join(".analyses");
        let result = std::fs::create_dir_all(&directory)
            .map_err(|e| e.to_string())
            .and_then(|_| {
                crate::ai_sequence::suggestions::Analysis::start(
                    request,
                    directory,
                    self.ai_sequences.refresh_analysis,
                )
            });
        match result {
            Ok(analysis) => {
                self.ai_sequences.analysis = Some(analysis);
                self.ai_sequences.analysis_project = Some(path);
                self.ai_sequences.suggestions.clear();
                self.ai_sequences.error = None;
            }
            Err(error) => self.ai_sequences.error = Some(error),
        }
    }
}

impl VenturiApp {
    pub(crate) fn ai_project_busy(&self, path: &Path) -> bool {
        self.ai_sequences
            .queues
            .get(path)
            .is_some_and(|queue| queue.jobs.iter().any(|job| !job.stage.terminal()))
            || (self.ai_sequences.analysis.is_some()
                && self.ai_sequences.analysis_project.as_deref() == Some(path))
            || self
                .ai_sequences
                .source_copy
                .as_ref()
                .is_some_and(|copy| copy.from == path)
    }

    pub(crate) fn release_ai_project(&mut self, path: &Path) {
        self.ai_sequences.queues.remove(path);
    }
}

struct SourceCopy {
    from: PathBuf,
    to: PathBuf,
    epoch: u64,
    rx: std::sync::mpsc::Receiver<Result<bool, String>>,
}

impl VenturiApp {
    pub(crate) fn begin_ai_source_copy(&mut self, destination: &Path) -> bool {
        if self.ai_sequences.source_copy.is_some() {
            self.toast("A project source copy is already running");
            return true;
        }
        let Some(from) = self
            .session
            .path()
            .filter(|path| *path != destination)
            .map(Path::to_path_buf)
        else {
            return false;
        };
        if !crate::ai_sequence::portable::directory(&from).exists() {
            return false;
        }
        let to = destination.to_path_buf();
        let (tx, rx) = std::sync::mpsc::channel();
        let source = from.clone();
        let target = to.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("ai-project-copy".into())
            .spawn(move || {
                let result =
                    crate::ai_sequence::portable::copy(&source, &target).map_err(|e| e.to_string());
                let _ = tx.send(result);
            })
        {
            self.project_error = Some(format!("Cannot start AI source copy: {error}"));
            return true;
        }
        self.ai_sequences.source_copy = Some(SourceCopy {
            from,
            to,
            epoch: self.session.epoch(),
            rx,
        });
        self.toast("Copying AI source files and rendered media in the background. Existing jobs stay with the original project.");
        true
    }

    pub(crate) fn poll_ai_source_copy(&mut self) {
        let Some(copy) = self.ai_sequences.source_copy.as_ref() else {
            return;
        };
        let result = match copy.rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("AI source copy stopped unexpectedly".into())
            }
        };
        let copy = self.ai_sequences.source_copy.take().unwrap();
        match result {
            Ok(created)
                if self.session.path() == Some(copy.from.as_path())
                    && self.session.epoch() == copy.epoch =>
            {
                self.finish_save_after_ai_copy(&copy.to, created);
            }
            Ok(created) => {
                if created && !copy.to.exists() {
                    let _ =
                        std::fs::remove_dir_all(crate::ai_sequence::portable::directory(&copy.to));
                }
                self.toast("Save As was cancelled because the current project changed during source copying");
            }
            Err(error) => {
                self.project_error = Some(format!("Cannot copy AI sequence sources: {error}"))
            }
        }
    }

    pub(crate) fn ai_placeholders(&mut self) -> Vec<timeline_ui::AiPlaceholder> {
        let (Some(timeline_id), Some(path)) = (self.timeline_id, self.session.path()) else {
            return Vec::new();
        };
        let (Some(queue), Some(timeline)) = (
            self.ai_sequences.queues.get(path),
            self.session.project.timelines.get(timeline_id),
        ) else {
            return Vec::new();
        };
        // Failures of earlier sessions get no block: remember what the queue held when first seen.
        if self
            .ai_sequences
            .earlier_jobs
            .as_ref()
            .is_none_or(|(owner, _)| owner != path)
        {
            let ids = queue.jobs.iter().map(|job| job.id.clone()).collect();
            self.ai_sequences.earlier_jobs = Some((path.to_path_buf(), ids));
        }
        let earlier = &self.ai_sequences.earlier_jobs.as_ref().unwrap().1;
        queue
            .jobs
            .iter()
            .filter(|job| {
                let request = &job.request;
                request.insert
                    && request.parent.is_none()
                    && request.timeline == Some(timeline_id.raw())
                    && (!job.stage.terminal()
                        || (job.stage == Stage::Completed
                            && self.ai_sequences.pending_imports.contains_key(&job.id))
                        || (job.stage == Stage::Failed
                            && !earlier.contains(&job.id)
                            && !self.ai_sequences.dismissed_placeholders.contains(&job.id)))
            })
            .filter_map(|job| {
                let composition = &job.request.composition;
                let start = crate::ai_sequence::insertion::position(timeline, &job.request).ok()?;
                // Until the model picks a length, don't reserve the whole maximum.
                let frames = if job.request.auto_duration && job.stage != Stage::Completed {
                    composition
                        .frames
                        .min((composition.fps * AUTO_PLACEHOLDER_SECONDS) as u32)
                } else {
                    composition.frames
                };
                let len = vv_core::convert_frames(
                    i64::from(frames),
                    vv_core::Rational::from_fps(composition.fps),
                    timeline.fps,
                )
                .max(1);
                Some(timeline_ui::AiPlaceholder {
                    job: job.id.clone(),
                    failed: job.stage == Stage::Failed,
                    done: job.stage == Stage::Completed,
                    start,
                    len,
                    label: match job.stage {
                        Stage::Failed => t!("ai_sequence.placeholder_failed").into_owned(),
                        Stage::Completed => t!("ai_sequence.placeholder_done").into_owned(),
                        _ => t!("ai_sequence.placeholder").into_owned(),
                    },
                    detail: match job.stage {
                        Stage::Failed => t!("ai_sequence.placeholder_retry").into_owned(),
                        Stage::Completed => t!("ai_sequence.placeholder_adding").into_owned(),
                        _ => job.stage.label().to_string(),
                    },
                })
            })
            .collect()
    }

    pub(crate) fn apply_ai_placeholder_action(&mut self) {
        use timeline_ui::AiPlaceholderAction as Action;
        let Some(action) = self.timeline_state.ai_placeholder_action.take() else {
            return;
        };
        let queue = self
            .session
            .path()
            .and_then(|path| self.ai_sequences.queues.get(path));
        match action {
            Action::Cancel(id) => {
                if let Some(queue) = queue {
                    queue.cancel(&id);
                }
            }
            Action::Dismiss(id) => {
                self.ai_sequences.dismissed_placeholders.insert(id);
            }
            Action::Retry(id) => {
                let request = queue
                    .and_then(|queue| queue.jobs.iter().find(|job| job.id == id))
                    .map(|job| job.request.clone());
                self.ai_sequences.dismissed_placeholders.insert(id);
                if let Some(request) = request {
                    self.submit_ai_sequence(request);
                }
            }
        }
    }

    pub(crate) fn ai_activity(&self) -> usize {
        self.ai_sequences
            .queues
            .values()
            .map(|queue| {
                queue
                    .jobs
                    .iter()
                    .filter(|job| !job.stage.terminal())
                    .count()
            })
            .sum::<usize>()
            + usize::from(self.ai_sequences.analysis.is_some())
            + usize::from(self.ai_sequences.source_copy.is_some())
    }
}

impl VenturiApp {
    fn show_other_ai_jobs(&self, ui: &mut egui::Ui, current: Option<&Path>) -> Option<PathBuf> {
        let mut open = None;
        if self
            .ai_sequences
            .queues
            .keys()
            .any(|path| Some(path.as_path()) != current)
        {
            ui.collapsing(t!("ai_sequence.other_projects"), |ui| {
                for (path, queue) in &self.ai_sequences.queues {
                    if Some(path.as_path()) == current {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.label(path.file_name().unwrap_or_default().to_string_lossy());
                        if ui.small_button(t!("ai_sequence.open_project")).clicked() {
                            open = Some(path.clone());
                        }
                    });
                    for job in queue.jobs.iter().rev() {
                        ui.horizontal(|ui| {
                            if !job.stage.terminal() {
                                ui.spinner();
                            }
                            ui.label(format!(
                                "{:?} · {}",
                                job.stage,
                                job.request.prompt.chars().take(40).collect::<String>()
                            ));
                            if !job.stage.terminal()
                                && ui.small_button(t!("ai_sequence.cancel")).clicked()
                            {
                                queue.cancel(&job.id);
                            }
                        });
                    }
                }
            });
        }
        open
    }
}

#[cfg(test)]
#[path = "tests/ai_sequence_ui.rs"]
mod tests;

impl VenturiApp {
    pub(crate) fn ai_import_finished(&mut self, job: vv_session::JobId, errors: &[String]) -> bool {
        let Some(id) = self
            .ai_sequences
            .import_jobs
            .remove(&(self.session.epoch(), job))
        else {
            return false;
        };
        if !errors.is_empty() {
            self.ai_sequences.pending_imports.remove(&id);
            self.ai_sequences.pending_applies.remove(&id);
            self.ai_sequences.error = Some(format!(
                "The AI sequence rendered successfully but media import failed: {}",
                errors.join("; ")
            ));
        }
        true
    }

    pub(crate) fn ai_import_running(&self, job: vv_session::JobId) -> bool {
        self.ai_sequences
            .import_jobs
            .contains_key(&(self.session.epoch(), job))
    }

    fn ai_media_for_version(&self, job: &Job) -> Option<MediaId> {
        self.session
            .project
            .ai_sequences
            .iter()
            .find_map(|(media, id)| {
                (id == &job.id && self.session.project.media_pool.contains_key(*media))
                    .then_some(*media)
            })
    }
}
