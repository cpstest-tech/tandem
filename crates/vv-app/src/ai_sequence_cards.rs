//! Cards of the infographic panel and of the AI properties section.
use crate::ai_sequence::Improvement;
use crate::theme;
use egui::{Color32, Painter, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};

pub(crate) type Art = fn(&Painter, Rect);

const ART_HEIGHT: f32 = 58.0;

pub(crate) fn section_title(ui: &mut egui::Ui, number: &str, text: &str) {
    ui.add_space(theme::SPACE_2);
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(number).color(theme::ACCENT).strong());
        ui.label(crate::ui_kit::section_title(text));
    });
}

pub(crate) const CARD_WIDTH: f32 = 104.0;

/// A row of cards that scrolls sideways when it does not fit; the mouse wheel
/// scrolls it too, handing over to the page once an end is reached.
pub(crate) fn card_strip(ui: &mut egui::Ui, id: &str, add: impl FnOnce(&mut egui::Ui)) {
    let output = egui::ScrollArea::horizontal()
        .id_salt(id)
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                add(ui);
            });
            ui.add_space(theme::SPACE_1);
        });
    let max = output.content_size.x - output.inner_rect.width();
    if max <= 0.0 || !ui.rect_contains_pointer(output.inner_rect) {
        return;
    }
    let delta = ui.input(|input| input.smooth_scroll_delta);
    if delta.y == 0.0 || delta.x != 0.0 {
        return;
    }
    let mut state = output.state;
    let offset = (state.offset.x - delta.y).clamp(0.0, max);
    if offset != state.offset.x {
        state.offset.x = offset;
        state.store(ui.ctx(), output.id);
        ui.input_mut(|input| input.smooth_scroll_delta.y = 0.0);
        ui.ctx().request_repaint();
    }
}

/// Two or more equal segments; returns the clicked index.
pub(crate) fn tab_bar(ui: &mut egui::Ui, labels: &[&str], selected: usize) -> Option<usize> {
    let height = theme::FIELD_HEIGHT + 4.0;
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, theme::RADIUS_PANEL, theme::BG_INSET);
    let width = rect.width() / labels.len().max(1) as f32;
    let mut clicked = None;
    for (index, label) in labels.iter().enumerate() {
        let segment = Rect::from_min_size(
            pos2(rect.left() + width * index as f32, rect.top()),
            vec2(width, height),
        )
        .shrink(2.0);
        let response = ui
            .interact(segment, ui.id().with(("tab", index)), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let active = index == selected;
        let fill = if active {
            theme::BG_RAISED
        } else if response.hovered() {
            theme::BG_PANEL
        } else {
            Color32::TRANSPARENT
        };
        let painter = ui.painter();
        painter.rect_filled(segment, theme::RADIUS_CONTROL, fill);
        if active {
            painter.rect_filled(
                Rect::from_min_size(
                    pos2(segment.left() + 10.0, segment.bottom() - 2.0),
                    vec2(segment.width() - 20.0, 2.0),
                ),
                1.0,
                theme::ACCENT,
            );
        }
        painter.text(
            segment.center(),
            egui::Align2::CENTER_CENTER,
            *label,
            egui::FontId::proportional(theme::FONT_SECTION),
            if active {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_SECONDARY
            },
        );
        if response.clicked() {
            clicked = Some(index);
        }
    }
    clicked
}

pub(crate) fn choice_card(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    art: Art,
) -> egui::Response {
    let width = CARD_WIDTH;
    let pad = 5.0;
    let text_color = if selected {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_SECONDARY
    };
    let text = ui.painter().layout(
        label.to_owned(),
        egui::FontId::proportional(theme::FONT_SMALL),
        text_color,
        (width - pad * 2.0).max(10.0),
    );
    let text_height = text.size().y;
    let (rect, response) = ui.allocate_exact_size(
        vec2(width, pad * 3.0 + ART_HEIGHT + text_height),
        egui::Sense::click(),
    );
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = response.hovered();
    let fill = if response.is_pointer_button_down_on() {
        theme::BG_SELECTED
    } else if hovered {
        theme::BG_RAISED_HOVER
    } else {
        theme::BG_RAISED
    };
    let stroke = if selected {
        Stroke::new(1.5, theme::ACCENT)
    } else if hovered {
        Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.5))
    } else {
        Stroke::new(1.0, theme::BORDER_SUBTLE)
    };
    let painter = ui.painter();
    painter.rect(rect, theme::RADIUS_PANEL, fill, stroke, StrokeKind::Inside);
    art(
        painter,
        Rect::from_min_size(
            rect.min + vec2(pad, pad),
            vec2((width - pad * 2.0).max(1.0), ART_HEIGHT),
        ),
    );
    painter.galley(
        pos2(rect.left() + pad, rect.top() + pad * 2.0 + ART_HEIGHT),
        text,
        text_color,
    );
    response
}

pub(crate) fn art_simple(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 3.0, Color32::from_gray(0xee));
    let x = rect.left() + rect.width() * 0.12;
    painter.rect_filled(
        Rect::from_min_size(
            pos2(x, rect.top() + rect.height() * 0.2),
            vec2(rect.width() * 0.45, 4.0),
        ),
        2.0,
        Color32::from_gray(0x22),
    );
    for (index, fraction) in [0.7_f32, 0.55].into_iter().enumerate() {
        painter.rect_filled(
            Rect::from_min_size(
                pos2(x, rect.top() + rect.height() * 0.48 + index as f32 * 8.0),
                vec2(rect.width() * fraction, 2.5),
            ),
            1.0,
            Color32::from_gray(0x99),
        );
    }
}

pub(crate) fn art_modern(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 3.0, Color32::from_rgb(0x12, 0x1f, 0x24));
    let base = rect.bottom() - 6.0;
    let bar_width = rect.width() * 0.14;
    for (index, height) in [0.35_f32, 0.6, 0.85].into_iter().enumerate() {
        let x = rect.left() + rect.width() * 0.2 + index as f32 * (bar_width + rect.width() * 0.08);
        let bar = (rect.height() - 12.0) * height;
        painter.rect_filled(
            Rect::from_min_max(pos2(x, base - bar), pos2(x + bar_width, base)),
            2.0,
            theme::ACCENT.gamma_multiply(0.5 + height * 0.5),
        );
    }
    painter.circle_filled(
        pos2(rect.right() - 10.0, rect.top() + 10.0),
        3.5,
        theme::ACCENT,
    );
}

pub(crate) fn art_vivid(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 3.0, Color32::from_rgb(0x5b, 0x2a, 0xa8));
    painter.circle_filled(
        pos2(rect.left() + rect.width() * 0.28, rect.center().y),
        rect.height() * 0.28,
        Color32::from_rgb(0xff, 0x8a, 0x3d),
    );
    painter.rect_filled(
        Rect::from_min_size(
            pos2(
                rect.left() + rect.width() * 0.5,
                rect.top() + rect.height() * 0.28,
            ),
            vec2(rect.width() * 0.38, 7.0),
        ),
        3.5,
        Color32::from_rgb(0xff, 0x4f, 0x9a),
    );
    painter.rect_filled(
        Rect::from_min_size(
            pos2(
                rect.left() + rect.width() * 0.5,
                rect.top() + rect.height() * 0.56,
            ),
            vec2(rect.width() * 0.26, 7.0),
        ),
        3.5,
        Color32::from_rgb(0xff, 0xd2, 0x3f),
    );
}

fn bars(painter: &Painter, rect: Rect, colors: [Color32; 3], radius: f32) {
    let base = rect.bottom() - 8.0;
    let width = rect.width() * 0.13;
    for (index, height) in [0.4_f32, 0.65, 0.9].into_iter().enumerate() {
        let x = rect.left() + rect.width() * 0.22 + index as f32 * rect.width() * 0.2;
        painter.rect_filled(
            Rect::from_min_max(
                pos2(x, base - (rect.height() - 18.0) * height),
                pos2(x + width, base),
            ),
            radius,
            colors[index],
        );
    }
}

pub(crate) fn art_corporate(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 3.0, Color32::from_rgb(0xf4, 0xf6, 0xfa));
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(rect.width(), 9.0)),
        egui::CornerRadius {
            nw: 3,
            ne: 3,
            sw: 0,
            se: 0,
        },
        Color32::from_rgb(0x1b, 0x2f, 0x5c),
    );
    let navy = Color32::from_rgb(0x2c, 0x4f, 0x9e);
    bars(
        painter,
        rect.with_min_y(rect.top() + 6.0),
        [navy.gamma_multiply(0.5), navy.gamma_multiply(0.75), navy],
        1.0,
    );
}

pub(crate) fn art_neon(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 3.0, Color32::from_rgb(0x0b, 0x06, 0x1a));
    let cyan = Color32::from_rgb(0x2e, 0xf2, 0xff);
    let pink = Color32::from_rgb(0xff, 0x3c, 0xd2);
    let points: Vec<Pos2> = [0.0_f32, 0.25, 0.45, 0.7, 1.0]
        .iter()
        .zip([0.75_f32, 0.5, 0.62, 0.3, 0.2])
        .map(|(x, y)| {
            pos2(
                rect.left() + 8.0 + (rect.width() - 16.0) * x,
                rect.top() + rect.height() * y,
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        points.clone(),
        Stroke::new(5.0, cyan.gamma_multiply(0.25)),
    ));
    painter.add(egui::Shape::line(points, Stroke::new(1.6, cyan)));
    painter.circle_stroke(
        pos2(rect.right() - 14.0, rect.top() + 14.0),
        6.0,
        Stroke::new(1.6, pink),
    );
    painter.circle_stroke(
        pos2(rect.right() - 14.0, rect.top() + 14.0),
        8.5,
        Stroke::new(3.0, pink.gamma_multiply(0.25)),
    );
}

pub(crate) fn art_editorial(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 3.0, Color32::from_rgb(0xf3, 0xec, 0xdf));
    let ink = Color32::from_rgb(0x1a, 0x1a, 0x1a);
    painter.text(
        pos2(rect.left() + 7.0, rect.top() + 5.0),
        egui::Align2::LEFT_TOP,
        "Aa",
        egui::FontId::proportional(20.0),
        ink,
    );
    painter.line_segment(
        [
            pos2(rect.left() + 7.0, rect.top() + 30.0),
            pos2(rect.right() - 7.0, rect.top() + 30.0),
        ],
        Stroke::new(1.0, ink),
    );
    for row in 0..3 {
        let y = rect.top() + 36.0 + row as f32 * 6.0;
        painter.rect_filled(
            Rect::from_min_size(
                pos2(rect.left() + 7.0, y),
                vec2(rect.width() * if row == 2 { 0.4 } else { 0.55 }, 2.0),
            ),
            0.0,
            Color32::from_gray(0x77),
        );
    }
    painter.rect_filled(
        Rect::from_min_max(
            pos2(rect.right() - rect.width() * 0.3, rect.top() + 36.0),
            pos2(rect.right() - 7.0, rect.bottom() - 6.0),
        ),
        0.0,
        Color32::from_rgb(0xc8, 0x2f, 0x2f),
    );
}

pub(crate) fn art_glass(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 3.0, Color32::from_rgb(0x3a, 0x2f, 0x7a));
    let clip = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    clip.circle_filled(
        pos2(rect.left() + 16.0, rect.bottom() - 8.0),
        22.0,
        Color32::from_rgb(0xff, 0x7a, 0xa8),
    );
    clip.circle_filled(
        pos2(rect.right() - 12.0, rect.top() + 6.0),
        20.0,
        Color32::from_rgb(0x4f, 0xc3, 0xff),
    );
    let card = rect.shrink2(vec2(rect.width() * 0.18, rect.height() * 0.2));
    painter.rect(
        card,
        6.0,
        Color32::from_white_alpha(70),
        Stroke::new(1.0, Color32::from_white_alpha(150)),
        StrokeKind::Inside,
    );
    painter.rect_filled(
        Rect::from_min_size(card.min + vec2(6.0, 7.0), vec2(card.width() * 0.6, 3.0)),
        1.5,
        Color32::WHITE,
    );
    painter.rect_filled(
        Rect::from_min_size(card.min + vec2(6.0, 14.0), vec2(card.width() * 0.4, 2.5)),
        1.0,
        Color32::from_white_alpha(190),
    );
}

pub(crate) fn art_sketch(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 3.0, Color32::from_rgb(0xfa, 0xfa, 0xf5));
    let marker = Stroke::new(1.6, Color32::from_rgb(0x22, 0x33, 0x66));
    let wobble = |x: f32, y: f32| {
        pos2(
            rect.left() + x * rect.width(),
            rect.top() + y * rect.height(),
        )
    };
    painter.add(egui::Shape::line(
        vec![
            wobble(0.12, 0.3),
            wobble(0.3, 0.26),
            wobble(0.42, 0.33),
            wobble(0.4, 0.62),
            wobble(0.14, 0.66),
            wobble(0.12, 0.3),
        ],
        marker,
    ));
    painter.add(egui::Shape::line(
        vec![wobble(0.46, 0.46), wobble(0.62, 0.44)],
        marker,
    ));
    painter.add(egui::Shape::line(
        vec![wobble(0.56, 0.36), wobble(0.63, 0.44), wobble(0.56, 0.52)],
        marker,
    ));
    painter.circle_stroke(
        wobble(0.8, 0.46),
        rect.height() * 0.2,
        Stroke::new(1.6, Color32::from_rgb(0xe0, 0x4a, 0x3a)),
    );
}

pub(crate) fn art_retro(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 3.0, Color32::from_rgb(0xf6, 0xe3, 0xc2));
    let clip = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let sun = pos2(rect.center().x, rect.bottom() - 6.0);
    for (radius, color) in [
        (30.0, Color32::from_rgb(0xe8, 0x5d, 0x2a)),
        (22.0, Color32::from_rgb(0xf2, 0x9a, 0x2e)),
        (14.0, Color32::from_rgb(0xf7, 0xc9, 0x48)),
    ] {
        clip.circle_filled(sun, radius, color);
    }
    for row in 0..3 {
        clip.rect_filled(
            Rect::from_min_size(
                pos2(rect.left(), rect.bottom() - 18.0 + row as f32 * 6.0),
                vec2(rect.width(), 2.0),
            ),
            0.0,
            Color32::from_rgb(0xf6, 0xe3, 0xc2),
        );
    }
    painter.rect_filled(
        Rect::from_min_size(rect.min + vec2(7.0, 7.0), vec2(rect.width() * 0.4, 5.0)),
        0.0,
        Color32::from_rgb(0x3b, 0x2a, 0x20),
    );
}

pub(crate) fn art_elegant(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 3.0, Color32::from_rgb(0x0e, 0x0e, 0x10));
    let gold = Color32::from_rgb(0xd4, 0xaf, 0x6a);
    painter.rect_stroke(
        rect.shrink(5.0),
        1.0,
        Stroke::new(0.8, gold.gamma_multiply(0.7)),
        StrokeKind::Inside,
    );
    painter.text(
        pos2(rect.center().x, rect.center().y - 5.0),
        egui::Align2::CENTER_CENTER,
        "42%",
        egui::FontId::proportional(17.0),
        gold,
    );
    painter.line_segment(
        [
            pos2(rect.center().x - 16.0, rect.center().y + 10.0),
            pos2(rect.center().x + 16.0, rect.center().y + 10.0),
        ],
        Stroke::new(1.0, gold),
    );
}

pub(crate) fn art_above_head(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 3.0, Color32::from_gray(0x2e));
    let x = rect.center().x;
    let color = Color32::from_gray(0x78);
    painter.circle_filled(pos2(x, rect.bottom() - 22.0), 6.0, color);
    painter.rect_filled(
        Rect::from_center_size(pos2(x, rect.bottom() - 6.0), vec2(22.0, 14.0)),
        5.0,
        color,
    );
    painter.rect_filled(
        Rect::from_center_size(pos2(x, rect.top() + 11.0), vec2(rect.width() * 0.56, 11.0)),
        3.0,
        theme::ACCENT,
    );
}

fn timeline_art(painter: &Painter, rect: Rect, highlight: impl Fn(usize) -> bool) {
    painter.rect_filled(rect, 3.0, theme::BG_INSET);
    let widths = [0.22_f32, 0.3, 0.18, 0.2];
    let mut x = rect.left() + 5.0;
    let span = rect.width() - 10.0;
    for (index, width) in widths.into_iter().enumerate() {
        let clip = Rect::from_min_size(
            pos2(x, rect.center().y - 9.0),
            vec2(span * width - 2.0, 18.0),
        );
        if highlight(index) {
            painter.rect(
                clip,
                2.0,
                theme::CLIP_VIDEO,
                Stroke::new(1.5, theme::ACCENT),
                StrokeKind::Inside,
            );
        } else {
            painter.rect_filled(clip, 2.0, Color32::from_gray(0x3a));
        }
        x += span * width;
    }
}

pub(crate) fn art_scope_clip(painter: &Painter, rect: Rect) {
    timeline_art(painter, rect, |index| index == 1);
}

pub(crate) fn art_scope_timeline(painter: &Painter, rect: Rect) {
    timeline_art(painter, rect, |_| true);
}

fn person(painter: &Painter, rect: Rect, x: f32) {
    let color = Color32::from_gray(0x78);
    painter.circle_filled(pos2(x, rect.top() + 14.0), 5.0, color);
    painter.rect_filled(
        Rect::from_center_size(pos2(x, rect.top() + 31.0), vec2(18.0, 12.0)),
        5.0,
        color,
    );
}

pub(crate) fn art_pill(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 3.0, Color32::from_gray(0x2e));
    person(painter, rect, rect.center().x);
    painter.rect_filled(
        Rect::from_min_size(
            pos2(rect.left() + rect.width() * 0.18, rect.bottom() - 13.0),
            vec2(rect.width() * 0.64, 8.0),
        ),
        4.0,
        theme::ACCENT,
    );
}

pub(crate) fn art_side(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 3.0, Color32::from_gray(0x2e));
    person(painter, rect, rect.left() + rect.width() * 0.25);
    let panel = Rect::from_min_max(
        pos2(rect.left() + rect.width() * 0.52, rect.top() + 4.0),
        pos2(rect.right() - 4.0, rect.bottom() - 4.0),
    );
    painter.rect_filled(panel, 3.0, theme::ACCENT_FILL);
    for row in 0..3 {
        painter.rect_filled(
            Rect::from_min_size(
                pos2(panel.left() + 4.0, panel.top() + 6.0 + row as f32 * 10.0),
                vec2(
                    (panel.width() - 8.0) * if row == 0 { 0.9 } else { 0.6 },
                    3.0,
                ),
            ),
            1.5,
            Color32::from_white_alpha(200),
        );
    }
}

pub(crate) fn art_fullscreen(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 3.0, theme::ACCENT_FILL);
    let base = rect.bottom() - 7.0;
    for (index, height) in [0.4_f32, 0.65, 0.9].into_iter().enumerate() {
        let x = rect.left() + rect.width() * 0.25 + index as f32 * rect.width() * 0.2;
        painter.rect_filled(
            Rect::from_min_max(
                pos2(x, base - (rect.height() - 14.0) * height),
                pos2(x + rect.width() * 0.12, base),
            ),
            2.0,
            Color32::from_white_alpha(210),
        );
    }
}

enum Glyph {
    Sparkle,
    Motion,
    Text,
    Color,
}

fn glyph_for(idea: &Improvement) -> Glyph {
    let text = format!("{} {}", idea.title, idea.instruction).to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| text.contains(*word));
    if has(&[
        "anim",
        "appear",
        "one by one",
        "compar",
        "fluid",
        "motion",
        "movimento",
        "uno alla",
    ]) {
        Glyph::Motion
    } else if has(&[
        "color",
        "colour",
        "background",
        "contrast",
        "sfond",
        "colore",
        "palette",
        "tema",
    ]) {
        Glyph::Color
    } else if has(&[
        "text", "title", "font", "bigger", "larger", "readab", "testo", "titolo", "ingrand", "legg",
    ]) {
        Glyph::Text
    } else {
        Glyph::Sparkle
    }
}

fn paint_glyph(painter: &Painter, center: Pos2, glyph: &Glyph, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    match glyph {
        Glyph::Sparkle => {
            for (dx, dy) in [(5.5, 0.0), (0.0, 5.5)] {
                painter.line_segment([center - vec2(dx, dy), center + vec2(dx, dy)], stroke);
            }
            for (dx, dy) in [(3.0, 3.0), (3.0, -3.0)] {
                painter.line_segment(
                    [center - vec2(dx, dy) * 0.6, center + vec2(dx, dy) * 0.6],
                    stroke,
                );
            }
        }
        Glyph::Motion => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    center + vec2(-3.0, -5.0),
                    center + vec2(-3.0, 5.0),
                    center + vec2(5.0, 0.0),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Glyph::Text => {
            painter.text(
                center,
                egui::Align2::CENTER_CENTER,
                "Aa",
                egui::FontId::proportional(crate::theme::FONT_SMALL),
                color,
            );
        }
        Glyph::Color => {
            painter.circle_stroke(center, 5.0, stroke);
            painter.circle_filled(center + vec2(2.0, 0.0), 2.6, color);
        }
    }
}

/// A suggested edit: clicking it is the whole interaction.
pub(crate) fn improvement_card(
    ui: &mut egui::Ui,
    idea: &Improvement,
    enabled: bool,
) -> egui::Response {
    let pad = theme::SPACE_2;
    let badge = 26.0;
    let text_width = (ui.available_width() - pad * 3.0 - badge).max(40.0);
    let title = ui.painter().layout(
        idea.title.clone(),
        egui::FontId::proportional(theme::FONT_SECTION),
        if enabled {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_DISABLED
        },
        text_width,
    );
    let body = ui.painter().layout(
        idea.description.clone(),
        egui::FontId::proportional(theme::FONT_BODY),
        if enabled {
            theme::TEXT_SECONDARY
        } else {
            theme::TEXT_DISABLED
        },
        text_width,
    );
    let title_height = title.size().y;
    let height = (pad * 2.0 + title_height + 2.0 + body.size().y).max(pad * 2.0 + badge);
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), height),
        if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        },
    );
    let hovered = enabled && response.hovered();
    let pressed = enabled && response.is_pointer_button_down_on();
    let fill = if pressed {
        theme::ACCENT_FILL.gamma_multiply(0.6)
    } else if hovered {
        theme::BG_RAISED_HOVER
    } else {
        theme::BG_RAISED
    };
    let stroke = if hovered || pressed {
        Stroke::new(1.0, theme::ACCENT)
    } else {
        Stroke::new(1.0, theme::BORDER_SUBTLE)
    };
    let glyph = glyph_for(idea);
    let painter = ui.painter();
    painter.rect(rect, theme::RADIUS_PANEL, fill, stroke, StrokeKind::Inside);
    let center = pos2(
        rect.left() + pad + badge / 2.0,
        rect.top() + pad + badge / 2.0,
    );
    let accent = if enabled {
        theme::ACCENT
    } else {
        theme::TEXT_DISABLED
    };
    painter.circle_filled(
        center,
        badge / 2.0,
        accent.gamma_multiply(if hovered { 0.35 } else { 0.18 }),
    );
    paint_glyph(painter, center, &glyph, accent);
    let text_x = rect.left() + pad * 2.0 + badge;
    painter.galley(pos2(text_x, rect.top() + pad), title, theme::TEXT_PRIMARY);
    painter.galley(
        pos2(text_x, rect.top() + pad + title_height + 2.0),
        body,
        theme::TEXT_SECONDARY,
    );
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}
