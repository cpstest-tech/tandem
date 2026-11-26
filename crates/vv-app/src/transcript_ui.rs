//! «Trascrizione» tab: the transcript laid out as running text. Only the
//! lines in view are painted, so long recordings stay cheap during playback.

use std::collections::BTreeSet;

use vv_core::auto_cut::SegmentType;

use crate::auto_cut_ui::{timecode, type_color};
use crate::transcript::{Item, JobStatus, Mark, Model};
use crate::{VenturiApp, theme};

const FONT_SIZE: f32 = 15.0;
const LINE_HEIGHT: f32 = 26.0;
const WORD_PAD: f32 = 3.0;
const WORD_GAP: f32 = 2.0;
const HEADER_HEIGHT: f32 = 22.0;
const PARAGRAPH_GAP: f32 = 10.0;

#[derive(Clone, Copy)]
enum Kind {
    Item(usize),
    Header(usize),
}

struct Placed {
    kind: Kind,
    /// Relative to the top left of the text.
    rect: egui::Rect,
}

pub(crate) struct Layout {
    key: ((u64, u64, u64), u32, bool),
    height: f32,
    placed: Vec<Placed>,
}

impl Layout {
    fn build(ui: &egui::Ui, model: &Model, width: f32, pauses: bool) -> Self {
        let font = egui::FontId::proportional(FONT_SIZE);
        let measure = |text: String| {
            ui.ctx().fonts_mut(|f| {
                f.layout_no_wrap(text, font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            })
        };
        let mut placed = Vec::with_capacity(model.items.len());
        let (mut x, mut y) = (0.0_f32, 0.0_f32);
        let mut clip = None;
        for (index, item) in model.items.iter().enumerate() {
            if matches!(item, Item::Pause(_)) && !pauses {
                continue;
            }
            let item_clip = model.item_clip(*item);
            if clip != Some(item_clip) {
                if clip.is_some() {
                    y += LINE_HEIGHT + PARAGRAPH_GAP;
                }
                placed.push(Placed {
                    kind: Kind::Header(item_clip),
                    rect: egui::Rect::from_min_size(
                        egui::pos2(0.0, y),
                        egui::vec2(width, HEADER_HEIGHT),
                    ),
                });
                y += HEADER_HEIGHT;
                x = 0.0;
                clip = Some(item_clip);
            }
            let w = measure(item_text(model, *item)) + 2.0 * WORD_PAD;
            if x > 0.0 && x + w > width {
                x = 0.0;
                y += LINE_HEIGHT;
            }
            placed.push(Placed {
                kind: Kind::Item(index),
                rect: egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, LINE_HEIGHT - 4.0)),
            });
            x += w + WORD_GAP;
        }
        Self {
            key: (model.stamp, width.to_bits(), pauses),
            height: y + LINE_HEIGHT,
            placed,
        }
    }

    /// Placed entries whose line meets `top..bottom`.
    fn visible(&self, top: f32, bottom: f32) -> std::ops::Range<usize> {
        let from = self.placed.partition_point(|p| p.rect.bottom() < top);
        let to = self.placed.partition_point(|p| p.rect.top() <= bottom);
        from..to.max(from)
    }

    fn hit(&self, pos: egui::Pos2) -> Option<usize> {
        let line = self.visible(pos.y, pos.y);
        self.placed[line].iter().find_map(|p| match p.kind {
            Kind::Item(i) if p.rect.contains(pos) => Some(i),
            _ => None,
        })
    }

    fn rect_of(&self, item: usize) -> Option<egui::Rect> {
        self.placed.iter().find_map(|p| match p.kind {
            Kind::Item(i) if i == item => Some(p.rect),
            _ => None,
        })
    }
}

fn item_text(model: &Model, item: Item) -> String {
    match item {
        Item::Word(w) => model.words[w].text.clone(),
        Item::Pause(p) => {
            let p = &model.pauses[p];
            t!("transcript.pause", secs = format!("{:.1}", p.end - p.start)).into_owned()
        }
    }
}

fn error_text(error: &str) -> String {
    match error {
        "no_transcript" => t!("transcript.no_transcript").into_owned(),
        "model_missing" => t!("auto_zoom.model_missing").into_owned(),
        "no_audio" => t!("auto_cut.error_no_audio").into_owned(),
        "offline" => t!("auto_cut.error_offline").into_owned(),
        "locked" => t!("auto_cut.error_locked").into_owned(),
        "source_changed" => t!("auto_cut.error_source_changed").into_owned(),
        "timeline_changed" => t!("auto_cut.error_timeline_changed").into_owned(),
        "nothing_to_cut" => t!("transcript.nothing_to_cut").into_owned(),
        "cancelled" => t!("auto_cut.cancelled").into_owned(),
        other => t!("auto_cut.error", detail = other).into_owned(),
    }
}

#[derive(Default)]
struct Actions {
    seek: Option<f64>,
    cut: bool,
    auto_cut: Option<Option<usize>>,
    correct: Option<(usize, String)>,
    edit: Option<usize>,
    broll: bool,
}

impl VenturiApp {
    pub(crate) fn show_transcript_tab(&mut self, ui: &mut egui::Ui) {
        let delete = std::mem::take(&mut self.transcript.delete_requested);
        let select_all = std::mem::take(&mut self.transcript.select_all_requested);
        if self.timeline_id.is_none() {
            ui.weak(t!("transcript.no_timeline"));
            return;
        }
        self.refresh_transcript();
        self.poll_transcript_jobs(ui.ctx());
        let mut actions = Actions {
            cut: delete && !self.transcript.selection.is_empty(),
            ..Actions::default()
        };
        if select_all {
            let count = self.transcript.model.as_ref().map_or(0, |m| m.items.len());
            self.transcript.selection = (0..count).collect();
        }
        self.transcript_toolbar(ui, &mut actions);
        self.transcript_status(ui);
        if self
            .transcript
            .model
            .as_ref()
            .is_some_and(|m| !m.words.is_empty())
        {
            self.transcript_text(ui, &mut actions);
        }
        self.run_transcript_actions(ui.ctx(), actions);
    }

    fn transcript_toolbar(&mut self, ui: &mut egui::Ui, actions: &mut Actions) {
        let Some(model) = &self.transcript.model else {
            return;
        };
        let matches = crate::transcript::search(&model.words, &self.transcript.search);
        let mut jump = None;
        ui.horizontal(|ui| {
            let search = ui.add(
                egui::TextEdit::singleline(&mut self.transcript.search)
                    .hint_text(t!("transcript.search"))
                    .desired_width((ui.available_width() - 120.0).max(80.0)),
            );
            if search.changed() {
                self.transcript.match_index = 0;
                jump = Some(0);
            }
            let enter = search.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if !matches.is_empty() {
                let count = matches.len();
                let current = self.transcript.match_index.min(count - 1);
                if ui.small_button("‹").clicked() {
                    jump = Some((current + count - 1) % count);
                }
                if ui.small_button("›").clicked() || enter {
                    jump = Some((current + 1) % count);
                    if enter {
                        search.request_focus();
                    }
                }
                ui.weak(format!("{}/{}", current + 1, count));
            } else if !self.transcript.search.trim().is_empty() {
                ui.weak(t!("transcript.no_match"));
            }
        });
        if let Some(index) = jump
            && let Some(range) = matches.get(index)
        {
            self.transcript.match_index = index;
            let items: BTreeSet<usize> = range.clone().filter_map(|w| model.word_item(w)).collect();
            self.transcript.anchor = items.first().copied();
            self.transcript.selection = items;
            actions.seek = Some(model.words[range.start].start);
            self.transcript.followed = None;
        }
        let selected = self.transcript.selection.len();
        ui.horizontal_wrapped(|ui| {
            let cut = egui::Button::new(
                egui::RichText::new(t!("transcript.cut", count = selected)).color(theme::ON_ACCENT),
            )
            .fill(theme::ACCENT_FILL);
            if ui
                .add_enabled(selected > 0, cut)
                .on_hover_text(t!("transcript.cut_hint"))
                .clicked()
            {
                actions.cut = true;
            }
            if ui
                .button(t!("broll.button"))
                .on_hover_text(t!("broll.hint"))
                .clicked()
            {
                actions.broll = true;
            }
            ui.menu_button(t!("transcript.select"), |ui| {
                let model = self.transcript.model.as_ref().unwrap();
                let mut pick = None;
                if self.settings.shorts.transcript.show_pauses
                    && ui.button(t!("transcript.select_pauses")).clicked()
                {
                    pick = Some(model.marked_items(None, true));
                }
                if ui.button(t!("transcript.select_false_starts")).clicked() {
                    pick = Some(model.marked_items(Some(Mark::FalseStart), false));
                }
                if ui.button(t!("transcript.select_fillers")).clicked() {
                    pick = Some(model.marked_items(Some(Mark::Filler), false));
                }
                if ui.button(t!("transcript.select_none")).clicked() {
                    pick = Some(BTreeSet::new());
                }
                if let Some(items) = pick {
                    self.transcript.anchor = items.first().copied();
                    self.transcript.selection = items;
                    ui.close();
                }
            });
            if ui
                .button(t!("auto_cut.title"))
                .on_hover_text(t!("transcript.auto_cut_hint"))
                .clicked()
            {
                let model = self.transcript.model.as_ref().unwrap();
                let at = self.timeline_state.playhead as f64
                    / self.session.project.timelines[model.timeline].fps.as_f64();
                actions.auto_cut = Some(model.word_at(at).or_else(|| {
                    self.transcript
                        .selection
                        .first()
                        .and_then(|&i| match model.items[i] {
                            Item::Word(w) => Some(w),
                            Item::Pause(_) => None,
                        })
                }));
            }
            ui.menu_button(t!("transcript.options"), |ui| {
                let prefs = &mut self.settings.shorts.transcript;
                let mut margin = prefs.margin * 1000.0;
                if ui
                    .add(
                        egui::Slider::new(&mut margin, 0.0..=400.0)
                            .step_by(10.0)
                            .text(t!("transcript.margin"))
                            .suffix(" ms"),
                    )
                    .on_hover_text(t!("transcript.margin_hint"))
                    .changed()
                {
                    prefs.margin = margin / 1000.0;
                }
                let mut fade = prefs.crossfade * 1000.0;
                if ui
                    .add(
                        egui::Slider::new(&mut fade, 0.0..=20.0)
                            .step_by(1.0)
                            .text(t!("transcript.crossfade"))
                            .suffix(" ms"),
                    )
                    .on_hover_text(t!("transcript.crossfade_hint"))
                    .changed()
                {
                    prefs.crossfade = fade / 1000.0;
                }
                ui.checkbox(&mut prefs.ripple_all, t!("auto_cut.ripple"))
                    .on_hover_text(t!("auto_cut.ripple_hint"));
                ui.checkbox(&mut prefs.show_pauses, t!("transcript.show_pauses"));
            });
        });
        ui.horizontal_wrapped(|ui| {
            for (kind, label) in [
                (SegmentType::Silence, t!("auto_cut.silence")),
                (SegmentType::FalseStart, t!("auto_cut.false_start")),
                (SegmentType::Filler, t!("auto_cut.filler")),
            ] {
                ui.colored_label(type_color(kind), label);
            }
            ui.colored_label(theme::WARNING, t!("transcript.low_confidence"));
        });
    }

    pub(crate) fn transcript_status(&mut self, ui: &mut egui::Ui) {
        let Some(model) = &self.transcript.model else {
            return;
        };
        if let Some(error) = &self.transcript.error {
            ui.colored_label(theme::ERROR, error_text(error));
        }
        if model.candidates == 0 {
            ui.weak(t!("transcript.no_clips"));
            return;
        }
        if model.unreadable > 0 {
            ui.colored_label(
                theme::WARNING,
                t!("transcript.unreadable", count = model.unreadable),
            );
        }
        match self.transcript_job_status() {
            JobStatus::LookingUp => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(t!("auto_zoom.looking_up"));
                });
            }
            JobStatus::Transcribing {
                stage,
                fraction,
                done,
                total,
            } => {
                ui.label(match stage {
                    0 => t!("auto_cut.hashing"),
                    1 => t!("auto_cut.extracting"),
                    _ => t!("auto_cut.transcribing"),
                });
                ui.horizontal(|ui| {
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .show_percentage()
                            .desired_width((ui.available_width() - 90.0).max(60.0))
                            .text(format!("{}/{}", (done + 1).min(total), total)),
                    );
                    if ui.button(t!("common.cancel")).clicked() {
                        self.cancel_transcript_jobs();
                    }
                });
            }
            JobStatus::Idle => {
                let (to_transcribe, failure) = self.transcript_gaps();
                if let Some(failure) = failure {
                    ui.colored_label(theme::ERROR, error_text(&failure));
                }
                if to_transcribe > 0 {
                    ui.label(if model.words.is_empty() {
                        t!("transcript.no_transcript")
                    } else {
                        t!("transcript.some_missing", count = to_transcribe)
                    });
                    if ui
                        .button(t!("transcript.generate", count = to_transcribe))
                        .clicked()
                    {
                        self.transcribe_missing();
                    }
                } else if model.words.is_empty() && model.missing.is_empty() {
                    ui.weak(t!("transcript.no_words"));
                }
            }
        }
    }

    fn transcript_text(&mut self, ui: &mut egui::Ui, actions: &mut Actions) {
        let pauses = self.settings.shorts.transcript.show_pauses;
        let touch = self.vertical_layout();
        let playing = self.is_timeline_playing();
        let low_confidence = vv_core::auto_cut::Config::default().low_confidence;
        let Some(model_owned) = self.transcript.model.take() else {
            return;
        };
        let model = &model_owned;
        let fps = self.session.project.timelines[model.timeline].fps.as_f64();
        let now = self.timeline_state.playhead as f64 / fps;
        let current = model.word_at(now);
        let matches: BTreeSet<usize> =
            crate::transcript::search(&model.words, &self.transcript.search)
                .into_iter()
                .flatten()
                .collect();
        let width = (ui.available_width() - theme::SPACE_3).max(80.0);
        let layout_owned = match self.transcript.layout.take() {
            Some(l) if l.key == (model.stamp, width.to_bits(), pauses) => l,
            _ => Layout::build(ui, model, width, pauses),
        };
        let layout = &layout_owned;
        let follow = (playing && current.is_some() && current != self.transcript.followed)
            .then_some(current)
            .flatten();
        if follow.is_some() {
            self.transcript.followed = follow;
        }
        let jump_to = (!playing)
            .then(|| self.transcript.selection.first().copied())
            .flatten()
            .filter(|_| actions.seek.is_some());
        let state = &mut self.transcript;
        let mut focused = state.focused;
        let area = egui::ScrollArea::vertical()
            .id_salt("transcript_text")
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let sense = if touch {
                    egui::Sense::click()
                } else {
                    egui::Sense::click_and_drag()
                };
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(width, layout.height), sense);
                let origin = rect.min;
                let to_screen = |r: egui::Rect| r.translate(origin.to_vec2());
                let follow_item = follow.and_then(|w| model.word_item(w)).or(jump_to);
                if let Some(r) = follow_item.and_then(|i| layout.rect_of(i)) {
                    let screen = to_screen(r);
                    if !ui.clip_rect().contains_rect(screen) {
                        ui.scroll_to_rect(screen.expand(LINE_HEIGHT), Some(egui::Align::Center));
                    }
                }
                let hovered = response
                    .hover_pos()
                    .and_then(|p| layout.hit(p - origin.to_vec2()));
                let painter = ui.painter();
                let font = egui::FontId::proportional(FONT_SIZE);
                for placed in &layout.placed[layout.visible(viewport.top(), viewport.bottom())] {
                    let r = to_screen(placed.rect);
                    match placed.kind {
                        Kind::Header(c) => {
                            let clip = &model.clips[c];
                            painter.text(
                                r.left_center(),
                                egui::Align2::LEFT_CENTER,
                                t!(
                                    "transcript.clip_header",
                                    number = c + 1,
                                    at = timecode(clip.start)
                                ),
                                egui::FontId::proportional(theme::FONT_SMALL),
                                theme::TEXT_SECONDARY,
                            );
                        }
                        Kind::Item(index) => {
                            let item = model.items[index];
                            let selected = state.selection.contains(&index);
                            let (fill, text_color) = match item {
                                Item::Word(w) => {
                                    let word = &model.words[w];
                                    let fill = if selected {
                                        Some(theme::ACCENT_FILL)
                                    } else if current == Some(w) {
                                        Some(theme::BG_SELECTED)
                                    } else if matches.contains(&w) {
                                        Some(theme::WARNING.gamma_multiply(0.35))
                                    } else if hovered == Some(index) {
                                        Some(theme::BG_RAISED_HOVER)
                                    } else {
                                        match word.mark {
                                            Mark::FalseStart => Some(
                                                type_color(SegmentType::FalseStart)
                                                    .gamma_multiply(0.55),
                                            ),
                                            Mark::Filler => Some(
                                                type_color(SegmentType::Filler)
                                                    .gamma_multiply(0.55),
                                            ),
                                            Mark::Speech => None,
                                        }
                                    };
                                    let color = if current == Some(w) || selected {
                                        egui::Color32::WHITE
                                    } else if word.text != word.original {
                                        theme::ACCENT
                                    } else {
                                        theme::TEXT_PRIMARY
                                    };
                                    (fill, color)
                                }
                                Item::Pause(_) => (
                                    Some(if selected {
                                        theme::ACCENT_FILL
                                    } else if hovered == Some(index) {
                                        theme::BG_RAISED_HOVER
                                    } else {
                                        type_color(SegmentType::Silence).gamma_multiply(0.45)
                                    }),
                                    theme::TEXT_SECONDARY,
                                ),
                            };
                            if let Some(fill) = fill {
                                painter.rect_filled(r, theme::RADIUS_CONTROL, fill);
                            }
                            let editing = matches!(
                                (item, &state.editing),
                                (Item::Word(w), Some((e, _))) if w == *e
                            );
                            if !editing {
                                painter.text(
                                    r.left_center() + egui::vec2(WORD_PAD, 0.0),
                                    egui::Align2::LEFT_CENTER,
                                    item_text(model, item),
                                    if matches!(item, Item::Pause(_)) {
                                        egui::FontId::proportional(theme::FONT_SMALL)
                                    } else {
                                        font.clone()
                                    },
                                    text_color,
                                );
                            }
                            if let Item::Word(w) = item
                                && model.words[w]
                                    .confidence
                                    .is_some_and(|c| c < low_confidence)
                            {
                                let y = r.bottom() - 1.0;
                                painter.extend(egui::Shape::dashed_line(
                                    &[
                                        egui::pos2(r.left() + WORD_PAD, y),
                                        egui::pos2(r.right() - WORD_PAD, y),
                                    ],
                                    egui::Stroke::new(1.5, theme::WARNING),
                                    2.0,
                                    2.0,
                                ));
                            }
                        }
                    }
                }
                // The inline editor of a correction.
                if let Some((w, text)) = &mut state.editing
                    && let Some(r) = model.word_item(*w).and_then(|i| layout.rect_of(i))
                {
                    let r = to_screen(r);
                    let editor_rect = egui::Rect::from_min_size(
                        r.min,
                        egui::vec2(r.width().max(120.0), r.height()),
                    );
                    let id = egui::Id::new("transcript_correction");
                    let edit = ui.put(
                        editor_rect,
                        egui::TextEdit::singleline(text)
                            .id(id)
                            .font(egui::FontId::proportional(FONT_SIZE)),
                    );
                    if !edit.has_focus() && !edit.lost_focus() {
                        edit.request_focus();
                    }
                    let (enter, escape) = ui.input(|i| {
                        (
                            i.key_pressed(egui::Key::Enter),
                            i.key_pressed(egui::Key::Escape),
                        )
                    });
                    if escape {
                        state.editing = None;
                    } else if edit.lost_focus() || enter {
                        actions.correct = Some((*w, text.clone()));
                        state.editing = None;
                    }
                }
                let clicked_item = hovered.filter(|_| response.clicked());
                let modifiers = ui.input(|i| i.modifiers);
                if response.clicked() || response.drag_started() || response.secondary_clicked() {
                    focused = true;
                }
                if let Some(index) = clicked_item {
                    if modifiers.command {
                        if !state.selection.remove(&index) {
                            state.selection.insert(index);
                        }
                        state.anchor = Some(index);
                    } else if modifiers.shift
                        && let Some(anchor) = state.anchor
                    {
                        state.selection = (anchor.min(index)..=anchor.max(index)).collect();
                    } else {
                        state.selection = BTreeSet::from([index]);
                        state.anchor = Some(index);
                        actions.seek = Some(model.item_span(model.items[index]).0);
                    }
                    state.followed = None;
                } else if response.clicked() {
                    state.selection.clear();
                    state.anchor = None;
                }
                if response.double_clicked()
                    && let Some(Item::Word(w)) = hovered.map(|i| model.items[i])
                {
                    state.selection = model.sentence_items(w).into_iter().collect();
                    state.anchor = state.selection.first().copied();
                }
                if response.dragged()
                    && let (Some(origin_pos), Some(pos)) = (
                        ui.input(|i| i.pointer.press_origin()),
                        response.interact_pointer_pos(),
                    )
                    && let (Some(from), Some(to)) = (
                        layout.hit(origin_pos - origin.to_vec2()),
                        layout.hit(pos - origin.to_vec2()),
                    )
                {
                    state.selection = (from.min(to)..=from.max(to)).collect();
                    state.anchor = Some(from);
                }
                if response.secondary_clicked()
                    && let Some(index) = hovered
                    && !state.selection.contains(&index)
                {
                    if let Some(anchor) = state.anchor.filter(|_| touch) {
                        // Touch: the long press extends the selection from the anchor.
                        state.selection = (anchor.min(index)..=anchor.max(index)).collect();
                    } else {
                        state.selection = BTreeSet::from([index]);
                        state.anchor = Some(index);
                    }
                }
                let menu_word = hovered
                    .or(state.anchor)
                    .and_then(|i| match model.items.get(i) {
                        Some(Item::Word(w)) => Some(*w),
                        _ => None,
                    });
                response.context_menu(|ui| {
                    if let Some(w) = menu_word {
                        if ui.button(t!("transcript.correct")).clicked() {
                            actions.edit = Some(w);
                            ui.close();
                        }
                        if ui.button(t!("transcript.select_sentence")).clicked() {
                            state.selection = model.sentence_items(w).into_iter().collect();
                            state.anchor = state.selection.first().copied();
                            ui.close();
                        }
                        if ui.button(t!("transcript.play_from_here")).clicked() {
                            actions.seek = Some(model.words[w].start);
                            ui.close();
                        }
                    }
                    if ui
                        .add_enabled(
                            !state.selection.is_empty(),
                            egui::Button::new(t!("transcript.cut", count = state.selection.len())),
                        )
                        .clicked()
                    {
                        actions.cut = true;
                        ui.close();
                    }
                    if ui.button(t!("auto_cut.title")).clicked() {
                        actions.auto_cut = Some(menu_word);
                        ui.close();
                    }
                });
                if let Some(index) = hovered
                    && let Item::Word(w) = model.items[index]
                {
                    let word = &model.words[w];
                    let mut tips = Vec::new();
                    if let Some(c) = word.confidence.filter(|c| *c < low_confidence) {
                        tips.push(
                            t!("transcript.uncertain", percent = (c * 100.0).round() as u32)
                                .into_owned(),
                        );
                    }
                    if word.text != word.original {
                        tips.push(
                            t!("transcript.corrected_from", original = &word.original).into_owned(),
                        );
                    }
                    match word.mark {
                        Mark::FalseStart => tips.push(t!("auto_cut.false_start").into_owned()),
                        Mark::Filler => tips.push(t!("auto_cut.filler").into_owned()),
                        Mark::Speech => {}
                    }
                    if !tips.is_empty() {
                        response.clone().on_hover_text(tips.join("\n"));
                    }
                }
                response.rect
            });
        if ui.input(|i| i.pointer.any_pressed())
            && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
        {
            focused = area.inner_rect.contains(pos);
        }
        self.transcript.focused = focused;
        self.transcript.model = Some(model_owned);
        self.transcript.layout = Some(layout_owned);
        // Keys of the panel: Del and Ctrl+A arrive through the shortcuts.
        let typing = ui.ctx().egui_wants_keyboard_input();
        if self.transcript.focused && !typing && self.transcript.editing.is_none() {
            let (escape, rename) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Escape),
                    i.key_pressed(egui::Key::F2) || i.key_pressed(egui::Key::Enter),
                )
            });
            if escape {
                self.transcript.selection.clear();
            }
            if rename && self.transcript.selection.len() == 1 {
                let model = self.transcript.model.as_ref().unwrap();
                if let Some(Item::Word(w)) = self
                    .transcript
                    .selection
                    .first()
                    .and_then(|&i| model.items.get(i).copied())
                {
                    actions.edit = Some(w);
                }
            }
        }
        if playing {
            ui.ctx().request_repaint();
        }
    }

    fn run_transcript_actions(&mut self, ctx: &egui::Context, actions: Actions) {
        if actions.broll {
            let count = self.insert_auto_broll();
            let message = if count == 0 {
                t!("broll.none").into_owned()
            } else {
                t!("broll.done", count = count).into_owned()
            };
            self.toast = Some((message, std::time::Instant::now()));
        }
        if let Some(w) = actions.edit
            && let Some(word) = self.transcript.model.as_ref().and_then(|m| m.words.get(w))
        {
            self.transcript.editing = Some((w, word.text.clone()));
        }
        if let Some((w, text)) = actions.correct {
            self.correct_transcript_word(w, &text);
        }
        if let Some(seconds) = actions.seek {
            self.seek_transcript(seconds);
        }
        if actions.cut {
            self.cut_transcript_selection();
        }
        if let Some(word) = actions.auto_cut {
            self.auto_cut_from_transcript(ctx, word);
        }
    }
}
