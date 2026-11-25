//! The Subtitles tab of the Library block: Generate, Templates, Edit.

use super::*;
use crate::subtitles::{Message, Section, models_dir};
use vv_core::captions::{SubtitleTemplate, TEMPLATES, TemplateCategory};
use vv_transcribe::Stage;

const CARD_SIDE: f32 = 160.0;
/// Length of a subtitle added by hand.
const NEW_LINE_MS: u32 = 2000;

enum EditAction {
    Seek(u32),
    Text(std::ops::Range<usize>, String),
    Retime(std::ops::Range<usize>, u32, u32),
    Insert(u32),
}
const CARD_LABEL: f32 = 24.0;
const PREVIEW_TEXTURE: u32 = 320;
/// Square frame the sample titles are built and drawn for.
const PREVIEW_RES: (u32, u32) = (1080, 1080);

/// Seconds a hover preview plays its animation, and the pause before it loops.
const PREVIEW_CLIP_SECS: f32 = 1.6;
const PREVIEW_PAUSE_SECS: f32 = 0.5;

/// The sample `secs` into a looping hover preview.
fn hover_frame(title: &vv_core::TitleParams, secs: f32) -> vv_core::TitleParams {
    let t = secs
        .rem_euclid(PREVIEW_CLIP_SECS + PREVIEW_PAUSE_SECS)
        .min(PREVIEW_CLIP_SECS);
    let mut frame = title.clone();
    frame.anim_state = frame
        .anim
        .state_at(&frame.display_text(), t, PREVIEW_CLIP_SECS);
    frame
}

/// `template` speaking `sample` word by word as its clips would, `secs`
/// into a looping hover preview.
fn template_hover_frame(
    template: &SubtitleTemplate,
    sample: &str,
    secs: f32,
) -> vv_core::TitleParams {
    let length = SubtitleTemplate::preview_secs(sample);
    let t = secs.rem_euclid(length + PREVIEW_PAUSE_SECS).min(length);
    template.preview_frame(sample, PREVIEW_RES, t)
}

/// The most characters any frame of a template preview shows, so every
/// word is drawn at the same size and none overflows the card.
fn widest_hover_text(template: &SubtitleTemplate, sample: &str) -> usize {
    let words = sample.split_whitespace().count().max(1);
    (0..words)
        .map(|i| {
            let secs = (i as f32 + 0.5) * vv_core::captions::PREVIEW_WORD_SECS;
            template
                .preview_frame(sample, PREVIEW_RES, secs)
                .content
                .default
                .chars()
                .count()
        })
        .max()
        .unwrap_or(1)
}

fn text_chars(title: &vv_core::TitleParams) -> usize {
    title.content.default.chars().count()
}

/// The sample centred and enlarged so `chars` characters fill most of a
/// square card.
fn square_preview_image(title: &vv_core::TitleParams, chars: usize) -> egui::ColorImage {
    let chars = chars.max(1) as f32;
    let boost = (PREVIEW_RES.0 as f32 * 0.62 / (title.size.max(1.0) * 0.6 * chars)).clamp(1.0, 3.5);
    let mut title = title.clone();
    title.size *= boost;
    title.position = [0.0, 0.0];
    title.anchor = (vv_core::HAnchor::Center, vv_core::VAnchor::Middle);
    library_ribbon::preview_image_in(
        std::slice::from_ref(&title),
        PREVIEW_RES,
        (PREVIEW_TEXTURE, PREVIEW_TEXTURE),
    )
}

/// How a card plays while the pointer is on it.
enum Hover<'a> {
    /// The template's clips for this sample, word after word.
    Template(&'static SubtitleTemplate, &'a str),
    /// The still title's own animation.
    Title,
}

const SIDEBAR: f32 = 112.0;

fn section_label(section: Section) -> std::borrow::Cow<'static, str> {
    match section {
        Section::Generate => t!("subtitles.section_generate"),
        Section::Templates => t!("subtitles.section_templates"),
        Section::Edit => t!("subtitles.section_edit"),
    }
}

fn language_label(code: Option<&str>) -> std::borrow::Cow<'static, str> {
    match code {
        None => t!("subtitles.language_auto"),
        Some("it") => t!("subtitles.lang_it"),
        Some("en") => t!("subtitles.lang_en"),
        Some("es") => t!("subtitles.lang_es"),
        Some("fr") => t!("subtitles.lang_fr"),
        Some("de") => t!("subtitles.lang_de"),
        Some("pt") => t!("subtitles.lang_pt"),
        Some("nl") => t!("subtitles.lang_nl"),
        Some("ru") => t!("subtitles.lang_ru"),
        Some("pl") => t!("subtitles.lang_pl"),
        Some("tr") => t!("subtitles.lang_tr"),
        Some("ja") => t!("subtitles.lang_ja"),
        Some("zh") => t!("subtitles.lang_zh"),
        Some(other) => other.to_owned().into(),
    }
}

impl VenturiApp {
    /// A still of subtitle style `id` (a template or `text:<id>`) showing
    /// `sample`, as the Subtitles tab cards draw it.
    pub(crate) fn subtitle_style_image(&self, id: &str, sample: &str) -> Option<egui::ColorImage> {
        let title = match SubtitleTemplate::find(id) {
            Some(template) => template.title(sample, PREVIEW_RES),
            None => {
                let text_id = id.strip_prefix(crate::text_templates::SUBTITLE_PREFIX)?;
                match crate::text_templates::find(text_id) {
                    Some(def) => def.subtitle_title(sample, PREVIEW_RES),
                    None => {
                        let user = self
                            .settings
                            .text_templates
                            .iter()
                            .find(|t| t.id == text_id)?;
                        vv_core::text_template::subtitle_from_title(
                            &user.title,
                            sample,
                            PREVIEW_RES,
                            0.06,
                        )
                    }
                }
            }
        };
        Some(square_preview_image(&title, text_chars(&title)))
    }
}

pub(crate) fn template_name(id: &str) -> std::borrow::Cow<'static, str> {
    match id {
        "word_pop" => t!("subtitles.tpl_word_pop"),
        "word_yellow" => t!("subtitles.tpl_word_yellow"),
        "word_build" => t!("subtitles.tpl_word_build"),
        "word_slam" => t!("subtitles.tpl_word_slam"),
        "word_drop" => t!("subtitles.tpl_word_drop"),
        "line_typewriter" => t!("subtitles.tpl_line_typewriter"),
        "line_classic" => t!("subtitles.tpl_line_classic"),
        "line_outline" => t!("subtitles.tpl_line_outline"),
        "line_yellow" => t!("subtitles.tpl_line_yellow"),
        other => match other.strip_prefix(crate::text_templates::SUBTITLE_PREFIX) {
            Some(id) => crate::text_templates::find(id)
                .map_or_else(|| t!("subtitles.tpl_custom"), |d| d.name.clone().into()),
            None => other.to_owned().into(),
        },
    }
}

fn primary_button(ui: &mut egui::Ui, text: String, enabled: bool) -> egui::Response {
    let button = crate::ui_kit::primary_button(text).min_size(egui::vec2(0.0, theme::FIELD_HEIGHT));
    ui.add_enabled(enabled, button)
}

impl VenturiApp {
    pub(crate) fn show_subtitles_tab(&mut self, ui: &mut egui::Ui) {
        let area = ui.available_rect_before_wrap();
        let side_rect = area.with_max_x(area.left() + SIDEBAR.min(area.width() * 0.4));
        let content_rect = area.with_min_x(side_rect.right() + theme::SPACE_2);
        let mut side_ui = ui.new_child(egui::UiBuilder::new().max_rect(side_rect));
        for section in [Section::Generate, Section::Templates, Section::Edit] {
            let selected = self.subtitles.section == section;
            let text = egui::RichText::new(section_label(section)).color(if selected {
                theme::ACCENT
            } else {
                theme::TEXT_PRIMARY
            });
            let button = egui::Button::selectable(selected, text);
            if side_ui
                .add_sized([side_ui.available_width(), theme::CONTROL_HEIGHT], button)
                .clicked()
            {
                self.subtitles.section = section;
            }
        }
        let mut content = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
        match self.subtitles.section {
            Section::Generate => self.show_generate_section(&mut content),
            Section::Templates => self.show_templates_section(&mut content),
            Section::Edit => self.show_subtitle_lines(&mut content, None, true),
        }
        ui.allocate_rect(area, egui::Sense::hover());
    }

    fn show_generate_section(&mut self, ui: &mut egui::Ui) {
        ui.add_space(theme::SPACE_1);
        ui.weak(t!("subtitles.generate_hint"));
        ui.add_space(theme::SPACE_2);
        let running = self.subtitles.job.is_some();
        ui.add_enabled_ui(!running, |ui| {
            ui.horizontal(|ui| {
                ui.label(t!("subtitles.language"));
                egui::ComboBox::from_id_salt("subtitle_language")
                    .selected_text(language_label(self.subtitles.language))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.subtitles.language,
                            None,
                            language_label(None),
                        );
                        for code in vv_transcribe::LANGUAGES {
                            ui.selectable_value(
                                &mut self.subtitles.language,
                                Some(code),
                                language_label(Some(code)),
                            );
                        }
                    });
            });
        });
        ui.add_space(theme::SPACE_2);
        if let Some((stage, fraction)) = self.transcription_stage() {
            let label = match stage {
                Stage::Download => t!("subtitles.stage_download"),
                Stage::Idle => t!("subtitles.stage_audio"),
                Stage::Transcribe => t!("subtitles.stage_transcribe"),
            };
            ui.label(label);
            let bar = egui::ProgressBar::new(if stage == Stage::Idle { 0.0 } else { fraction })
                .show_percentage()
                .animate(stage == Stage::Idle);
            ui.add(bar);
            ui.add_space(theme::SPACE_1);
            if ui.button(t!("common.cancel")).clicked() {
                self.cancel_transcription();
            }
        } else {
            let ready = self.timeline_id.is_some();
            if primary_button(ui, t!("subtitles.generate").into_owned(), ready).clicked() {
                self.retranscribe_subtitles();
            }
            let model = vv_transcribe::Model::preferred();
            let model_ready =
                models_dir().is_some_and(|dir| vv_transcribe::model_present(&dir, model));
            if !model_ready {
                ui.add_space(theme::SPACE_1);
                ui.weak(t!("subtitles.model_download_hint", mb = model.size_mb()));
            }
            if let Some(gpu) = vv_transcribe::gpu_name() {
                ui.add_space(theme::SPACE_1);
                ui.weak(t!("subtitles.gpu", name = gpu));
            }
        }
        if let Some(captions) = self
            .timeline_id
            .and_then(|id| self.session.project.captions.get(&id))
        {
            ui.add_space(theme::SPACE_2);
            ui.weak(t!(
                "subtitles.existing",
                words = captions.words.len(),
                language = captions.language,
                template = template_name(&captions.template)
            ));
        }
        match &self.subtitles.message {
            Some(Message::Error(text)) => {
                ui.add_space(theme::SPACE_2);
                ui.colored_label(theme::ERROR, text);
            }
            Some(Message::Info(text)) => {
                ui.add_space(theme::SPACE_2);
                ui.weak(text);
            }
            None => {}
        }
    }

    fn show_templates_section(&mut self, ui: &mut egui::Ui) {
        let resolution = PREVIEW_RES;
        let applied = self
            .timeline_id
            .and_then(|id| self.session.project.captions.get(&id))
            .map(|c| c.template.clone());
        let mut chosen = None;
        let footer = theme::FIELD_HEIGHT + theme::SPACE_2;
        let hover_sample = t!("subtitles.sample_line").into_owned();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height((ui.available_height() - footer).max(0.0))
            .show(ui, |ui| {
                for (category, title) in [
                    (TemplateCategory::WordByWord, t!("subtitles.category_word")),
                    (TemplateCategory::Line, t!("subtitles.category_line")),
                ] {
                    ui.label(crate::ui_kit::section_title(title));
                    ui.add_space(theme::SPACE_1);
                    ui.horizontal_wrapped(|ui| {
                        for template in TEMPLATES.iter().filter(|t| t.category == category) {
                            let selected = self.subtitles.selected.as_deref() == Some(template.id)
                                || (self.subtitles.selected.is_none()
                                    && applied.as_deref() == Some(template.id));
                            let sample = match template.category {
                                TemplateCategory::WordByWord => t!("subtitles.sample_word"),
                                TemplateCategory::Line => t!("subtitles.sample_line"),
                            };
                            let title = template.title(&sample, resolution);
                            let name = template_name(template.id).into_owned();
                            let hover = Hover::Template(template, &hover_sample);
                            if self.template_card(ui, template.id, name, &title, hover, selected) {
                                chosen = Some(template.id.to_owned());
                            }
                        }
                    });
                    ui.add_space(theme::SPACE_2);
                }
                ui.label(crate::ui_kit::section_title(t!(
                    "subtitles.category_text_styles"
                )));
                ui.add_space(theme::SPACE_1);
                ui.horizontal_wrapped(|ui| {
                    let styles = self.text_style_choices(resolution);
                    for (id, name, title) in styles {
                        let selected = self.subtitles.selected.as_deref() == Some(id.as_str())
                            || (self.subtitles.selected.is_none()
                                && applied.as_deref() == Some(id.as_str()));
                        if self.template_card(ui, &id, name, &title, Hover::Title, selected) {
                            chosen = Some(id);
                        }
                    }
                });
            });
        ui.add_space(theme::SPACE_2);
        let can_generate = self.subtitles.selected.is_some()
            && self.timeline_id.is_some()
            && self.subtitles.job.is_none();
        if primary_button(ui, t!("subtitles.generate").into_owned(), can_generate)
            .on_disabled_hover_text(t!("subtitles.pick_template"))
            .clicked()
        {
            self.generate_subtitles();
        }
        if let Some(id) = chosen {
            self.choose_subtitle_template(&id);
        }
    }

    /// `(id, name, sample title)` of the text templates usable as a
    /// subtitle style: the bundled ones, then the user's.
    fn text_style_choices(
        &self,
        resolution: (u32, u32),
    ) -> Vec<(String, String, vv_core::TitleParams)> {
        let sample = t!("subtitles.sample_line");
        let prefix = crate::text_templates::SUBTITLE_PREFIX;
        let bundled = crate::text_templates::all().iter().map(|d| {
            (
                format!("{prefix}{}", d.id),
                d.name.clone(),
                d.subtitle_title(&sample, resolution),
            )
        });
        let user = self.settings.text_templates.iter().map(|u| {
            (
                format!("{prefix}{}", u.id),
                u.name.clone(),
                vv_core::text_template::subtitle_from_title(&u.title, &sample, resolution, 0.06),
            )
        });
        bundled.chain(user).collect()
    }

    fn template_card(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        name: String,
        title: &vv_core::TitleParams,
        hover: Hover<'_>,
        selected: bool,
    ) -> bool {
        let key = id.to_owned();
        let animated = match hover {
            Hover::Template(template, _) => !template.title("", PREVIEW_RES).anim.is_empty(),
            Hover::Title => !title.anim.is_empty(),
        };
        let now = ui.input(|i| i.time);
        let hover_secs = match &self.subtitles.hover {
            Some((hovered, since)) if animated && *hovered == key => Some((now - since) as f32),
            _ => None,
        };
        if let Some(secs) = hover_secs {
            let image = match hover {
                Hover::Template(template, sample) => square_preview_image(
                    &template_hover_frame(template, sample, secs),
                    widest_hover_text(template, sample),
                ),
                Hover::Title => square_preview_image(&hover_frame(title, secs), text_chars(title)),
            };
            let texture =
                ui.ctx()
                    .load_texture("subtitle-hover", image, egui::TextureOptions::LINEAR);
            self.subtitles.hover_texture = Some(texture);
        }
        if !self.subtitles.previews.contains_key(&key) {
            let image = square_preview_image(title, text_chars(title));
            let texture = ui.ctx().load_texture(
                format!("subtitle-{key}"),
                image,
                egui::TextureOptions::LINEAR,
            );
            self.subtitles.previews.insert(key.clone(), texture);
        }
        let side = CARD_SIDE.min(ui.available_width());
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(side, side + CARD_LABEL), egui::Sense::click());
        let fill = if response.hovered() {
            theme::BG_RAISED_HOVER
        } else {
            theme::BG_RAISED
        };
        let painter = ui.painter();
        painter.rect_filled(rect, theme::RADIUS_CONTROL, fill);
        let texture = match hover_secs {
            Some(_) => self.subtitles.hover_texture.as_ref(),
            None => self.subtitles.previews.get(&key),
        };
        if let Some(texture) = texture {
            painter.image(
                texture.id(),
                egui::Rect::from_min_size(rect.min, egui::vec2(side, side)),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        painter.text(
            egui::pos2(rect.center().x, rect.bottom() - theme::SPACE_1),
            egui::Align2::CENTER_BOTTOM,
            name,
            egui::FontId::proportional(theme::FONT_SMALL),
            theme::TEXT_SECONDARY,
        );
        if selected {
            painter.rect_stroke(
                rect,
                theme::RADIUS_CONTROL,
                egui::Stroke::new(2.0, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        if animated && response.hovered() {
            if self
                .subtitles
                .hover
                .as_ref()
                .is_none_or(|(old, _)| *old != key)
            {
                self.subtitles.hover = Some((key, now));
            }
            ui.ctx().request_repaint();
        } else if self
            .subtitles
            .hover
            .as_ref()
            .is_some_and(|(old, _)| *old == key)
        {
            self.subtitles.hover = None;
        }
        response.clicked()
    }
}

#[cfg(test)]
#[path = "tests/subtitles_ui.rs"]
mod tests;

impl VenturiApp {
    /// The subtitles as a list, like CapCut's editor: every line with its
    /// times and text, editable; a click jumps there. `focus_ms` (the
    /// selected subtitle clip) is highlighted and scrolled to once.
    pub(crate) fn show_subtitle_lines(
        &mut self,
        ui: &mut egui::Ui,
        focus_ms: Option<u32>,
        own_scroll: bool,
    ) {
        let Some(timeline_id) = self.timeline_id else {
            ui.weak(t!("subtitles.edit_no_timeline"));
            return;
        };
        let fps = self.session.project.timelines[timeline_id]
            .fps
            .as_f64()
            .max(1e-9);
        let playhead_ms = (self.timeline_state.playhead as f64 / fps * 1000.0).round() as u32;
        let words = self
            .session
            .project
            .captions
            .get(&timeline_id)
            .map(|c| c.words.clone())
            .unwrap_or_default();
        let lines = vv_core::captions::caption_lines(&words);
        let playing = self.is_timeline_playing();
        let mut action = None;

        ui.add_space(theme::SPACE_1);
        ui.horizontal(|ui| {
            if ui.button(t!("subtitles.edit_add")).clicked() {
                // After the line under the playhead, not inside it.
                let at = lines
                    .iter()
                    .find(|l| (l.start_ms..l.end_ms).contains(&playhead_ms))
                    .map_or(playhead_ms, |l| l.end_ms);
                action = Some(EditAction::Insert(at));
            }
            ui.weak(t!("subtitles.edit_hint"));
        });
        ui.add_space(theme::SPACE_1);
        if lines.is_empty() {
            ui.weak(t!("subtitles.edit_empty"));
        }
        let mut drag_started = false;
        let mut drag_stopped = false;
        let scroll_to_focus = focus_ms.is_some() && focus_ms != self.subtitles.scrolled_to;
        self.subtitles.scrolled_to = focus_ms;
        let mut rows = |ui: &mut egui::Ui| {
            for line in &lines {
                let span = line.start_ms..line.end_ms.max(line.start_ms + 1);
                let active = span.contains(&playhead_ms);
                let focused = focus_ms.is_some_and(|ms| span.contains(&ms));
                let fill = if focused {
                    theme::BG_SELECTED
                } else if active {
                    theme::ACCENT_TRANSLUCENT
                } else {
                    egui::Color32::TRANSPARENT
                };
                // The row's click sense sits under its fields, which keep theirs.
                let row = ui.scope_builder(
                    egui::UiBuilder::new()
                        .id_salt(("subtitle_line", line.words.start))
                        .sense(egui::Sense::click()),
                    |ui| {
                        egui::Frame::new()
                            .fill(fill)
                            .corner_radius(4.0)
                            .inner_margin(egui::Margin::same(4))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let mut start = f64::from(line.start_ms) / 1000.0;
                                    let mut end = f64::from(line.end_ms) / 1000.0;
                                    fn time(value: &mut f64) -> egui::DragValue<'_> {
                                        egui::DragValue::new(value)
                                            .speed(0.01)
                                            .range(0.0..=f64::from(u32::MAX) / 1000.0)
                                            .fixed_decimals(2)
                                            .suffix(" s")
                                    }
                                    let start_resp = ui.add(time(&mut start));
                                    ui.label("–");
                                    let end_resp = ui.add(time(&mut end));
                                    for resp in [&start_resp, &end_resp] {
                                        drag_started |= resp.drag_started();
                                        drag_stopped |= resp.drag_stopped();
                                    }
                                    if start_resp.changed() || end_resp.changed() {
                                        let start_ms = (start * 1000.0).round() as u32;
                                        let end_ms =
                                            ((end * 1000.0).round() as u32).max(start_ms + 1);
                                        action = Some(EditAction::Retime(
                                            line.words.clone(),
                                            start_ms,
                                            end_ms,
                                        ));
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .small_button("✕")
                                                .on_hover_text(t!("subtitles.edit_delete"))
                                                .clicked()
                                            {
                                                action = Some(EditAction::Text(
                                                    line.words.clone(),
                                                    String::new(),
                                                ));
                                            }
                                        },
                                    );
                                });
                                let key = line.words.start;
                                let mut text = match &self.subtitles.editing {
                                    Some((k, t)) if *k == key => t.clone(),
                                    _ => line.text.clone(),
                                };
                                let field = ui.add(
                                    egui::TextEdit::singleline(&mut text)
                                        .desired_width(f32::INFINITY),
                                );
                                if field.changed() || field.gained_focus() {
                                    self.subtitles.editing = Some((key, text.clone()));
                                }
                                if field.lost_focus() {
                                    self.subtitles.editing = None;
                                    if text != line.text {
                                        action = Some(EditAction::Text(line.words.clone(), text));
                                    }
                                }
                            })
                    },
                );
                if row.response.clicked() {
                    action = Some(EditAction::Seek(line.start_ms));
                }
                if (active && playing) || (focused && scroll_to_focus) {
                    row.response.scroll_to_me(Some(egui::Align::Center));
                }
            }
        };
        if own_scroll {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, rows);
        } else {
            rows(ui);
        }

        if drag_started && self.subtitles.drag_group.is_none() {
            self.subtitles.drag_group = Some(self.session.history.begin_group());
        }
        match action {
            Some(EditAction::Seek(ms)) => {
                self.jump_playhead_to((f64::from(ms) / 1000.0 * fps).round() as FrameIdx);
            }
            Some(EditAction::Text(range, text)) => {
                self.edit_caption_words(|words| {
                    vv_core::captions::set_line_text(words, range, &text)
                });
            }
            Some(EditAction::Retime(range, start, end)) => {
                self.edit_caption_words(|words| {
                    vv_core::captions::retime_line(words, range, start, end)
                });
            }
            Some(EditAction::Insert(at)) => {
                let text = t!("subtitles.edit_new_text").into_owned();
                self.edit_caption_words(|words| {
                    vv_core::captions::insert_line(words, at, at + NEW_LINE_MS, &text)
                });
            }
            None => {}
        }
        if drag_stopped && let Some(mark) = self.subtitles.drag_group.take() {
            self.session.history.end_group(mark);
        }
    }
}
