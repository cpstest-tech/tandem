//! Fill / Stroke / Shadow / Glow / 3D / Background sections of a text clip,
//! with a live preview. Rows and widgets are the ones of the rest of the
//! Properties panel.

use super::*;
use crate::properties_panel::{
    RowKeyframe, RowResponse, axis_field, color_row, param_row, preview_combo, section_header,
    slider_field,
};
use vv_core::{
    BackgroundShape, Gradient, GradientKind, GradientStop, Rgba, StrokeJoin, TextExtrude, TextFill,
    TextGlow, TextShadow, TextStroke, TitleParams,
};

const PREVIEW_HEIGHT: f32 = 90.0;

/// Frame colour behind the preview, as in the library cards.
fn preview_frame_color() -> egui::Color32 {
    crate::theme::BG_INSET_ALT
}

/// The title as drawn, refreshed only when it changes.
pub(crate) fn live_preview(ui: &mut egui::Ui, title: &TitleParams, timeline_size: (u32, u32)) {
    let id = egui::Id::new("text_style_preview");
    let cached: Option<(TitleParams, (u32, u32), egui::TextureHandle)> =
        ui.data_mut(|d| d.get_temp(id));
    let texture = match cached {
        Some((t, size, texture)) if t == *title && size == timeline_size => texture,
        _ => {
            let image = crate::library_ribbon::title_preview_image(title, timeline_size);
            let texture =
                ui.ctx()
                    .load_texture("text_style_preview", image, egui::TextureOptions::LINEAR);
            ui.data_mut(|d| d.insert_temp(id, (title.clone(), timeline_size, texture.clone())));
            texture
        }
    };
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, PREVIEW_HEIGHT), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, crate::theme::RADIUS_CONTROL, preview_frame_color());
    let aspect = texture.aspect_ratio();
    let size = if rect.width() / rect.height() > aspect {
        egui::vec2(rect.height() * aspect, rect.height())
    } else {
        egui::vec2(rect.width(), rect.width() / aspect)
    };
    ui.painter().image(
        texture.id(),
        egui::Rect::from_center_size(rect.center(), size),
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

fn small_button(ui: &mut egui::Ui, label: &str, hint: &str) -> bool {
    ui.add_sized(
        egui::vec2(crate::theme::CONTROL_HEIGHT, crate::theme::CONTROL_HEIGHT),
        egui::Button::new(label),
    )
    .on_hover_text(hint)
    .clicked()
}

fn item_header(ui: &mut egui::Ui, label: String) -> bool {
    let mut remove = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(crate::theme::TEXT_SECONDARY));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            remove = small_button(ui, "×", &t!("style.remove"));
        });
    });
    remove
}

fn slider_row(
    ui: &mut egui::Ui,
    label: String,
    value: &mut f32,
    default: f32,
    range: std::ops::RangeInclusive<f32>,
    decimals: usize,
) -> bool {
    let speed = if decimals == 0 { 0.5 } else { 0.01 };
    let row = param_row(ui, &label, None, |ui| {
        slider_field(ui, value, range, speed, decimals)
    });
    if row.reset {
        *value = default;
    }
    row.changed || row.reset
}

fn color_field(ui: &mut egui::Ui, label: String, color: &mut Rgba, default: Rgba) -> bool {
    let row = color_row(ui, &label, color);
    if row.reset {
        *color = default;
    }
    row.changed || row.reset
}

/// Edits a gradient; `offset_key` makes the offset row animatable.
fn gradient_editor(
    ui: &mut egui::Ui,
    g: &mut Gradient,
    offset_key: Option<RowKeyframe>,
    offset_row: &mut RowResponse,
) -> bool {
    let mut changed = false;
    let mut remove = None;
    let can_remove = g.stops.len() > 2;
    for (i, stop) in g.stops.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let mut rgba: [f32; 4] = stop.color.into();
            if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
                stop.color = rgba.into();
                changed = true;
            }
            ui.spacing_mut().slider_width = (ui.available_width() - 110.0).clamp(60.0, 200.0);
            changed |= ui
                .add(egui::Slider::new(&mut stop.pos, 0.0..=1.0).show_value(false))
                .changed();
            let mut pos = stop.pos as f64;
            if crate::properties_panel::drag_field(ui, &mut pos, 0.01, 0.0..=1.0, 2, "") {
                stop.pos = pos as f32;
                changed = true;
            }
            if can_remove && small_button(ui, "×", &t!("style.remove_stop")) {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        g.stops.remove(i);
        changed = true;
    }
    if ui.button(t!("style.add_stop")).clicked() {
        let t = 0.5;
        let color = g.color_at(t);
        g.stops.push(GradientStop { pos: t, color });
        changed = true;
    }
    g.stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    if g.kind == GradientKind::Linear {
        changed |= slider_row(
            ui,
            t!("style.angle").to_string(),
            &mut g.angle,
            90.0,
            0.0..=360.0,
            0,
        );
    }
    let mut offset = g.offset.default;
    let row = param_row(ui, &t!("style.gradient_offset"), offset_key, |ui| {
        slider_field(ui, &mut offset, 0.0..=1.0, 0.005, 2)
    });
    if row.changed {
        g.offset.default = offset;
        changed = true;
    }
    if row.reset {
        g.offset.default = 0.0;
        changed = true;
    }
    *offset_row = row;
    changed
}

fn fill_section(
    ui: &mut egui::Ui,
    title: &mut TitleParams,
    offset_key: RowKeyframe,
    offset_row: &mut RowResponse,
) -> bool {
    let mut changed = false;
    let (open, reset) = section_header(ui, &t!("style.fill"), true);
    if reset {
        title.style.fill = TextFill::Solid;
        title.color = TitleParams::default().color;
        return true;
    }
    if !open {
        return false;
    }
    #[derive(Clone, PartialEq)]
    enum Kind {
        Solid,
        Gradient(GradientKind),
    }
    let mut kind = match &title.style.fill {
        TextFill::Solid => Kind::Solid,
        TextFill::Gradient(g) => Kind::Gradient(g.kind),
    };
    let items = [
        (Kind::Solid, t!("style.fill_solid").to_string(), true),
        (
            Kind::Gradient(GradientKind::Linear),
            t!("style.fill_linear").to_string(),
            true,
        ),
        (
            Kind::Gradient(GradientKind::Radial),
            t!("style.fill_radial").to_string(),
            true,
        ),
    ];
    let row = param_row(ui, &t!("style.type"), None, |ui| {
        preview_combo(ui, "title_fill_kind", &mut kind, &items, None, None, None)
    });
    if row.changed {
        title.style.fill = match (kind, std::mem::take(&mut title.style.fill)) {
            (Kind::Solid, _) => TextFill::Solid,
            (Kind::Gradient(k), TextFill::Gradient(mut g)) => {
                g.kind = k;
                TextFill::Gradient(g)
            }
            (Kind::Gradient(k), TextFill::Solid) => TextFill::Gradient(Gradient::two_stops(
                k,
                title.color,
                Rgba {
                    r: 1.0,
                    g: 0.35,
                    b: 0.75,
                    a: 1.0,
                },
            )),
        };
        changed = true;
    }
    match &mut title.style.fill {
        TextFill::Solid => {
            changed |= color_field(
                ui,
                t!("props.color").to_string(),
                &mut title.color,
                Rgba::WHITE,
            );
        }
        TextFill::Gradient(g) => {
            changed |= gradient_editor(ui, g, Some(offset_key), offset_row);
        }
    }
    changed
}

fn stroke_section(ui: &mut egui::Ui, title: &mut TitleParams) -> bool {
    let mut changed = false;
    let (open, reset) = section_header(ui, &t!("style.stroke"), true);
    if reset {
        title.style.strokes.clear();
        return true;
    }
    if !open {
        return false;
    }
    let mut remove = None;
    for (i, s) in title.style.strokes.iter_mut().enumerate() {
        if item_header(ui, format!("{} {}", t!("style.stroke"), i + 1)) {
            remove = Some(i);
        }
        let d = TextStroke::default();
        changed |= color_field(ui, t!("props.color").to_string(), &mut s.color, d.color);
        changed |= slider_row(
            ui,
            t!("style.width").to_string(),
            &mut s.width,
            d.width,
            0.0..=100.0,
            0,
        );
        let items = [
            (StrokeJoin::Round, t!("style.join_round").to_string(), true),
            (StrokeJoin::Miter, t!("style.join_miter").to_string(), true),
            (StrokeJoin::Bevel, t!("style.join_bevel").to_string(), true),
        ];
        let row = param_row(ui, &t!("style.join"), None, |ui| {
            preview_combo(
                ui,
                &format!("stroke_join_{i}"),
                &mut s.join,
                &items,
                None,
                None,
                None,
            )
        });
        changed |= row.changed;
    }
    if let Some(i) = remove {
        title.style.strokes.remove(i);
        changed = true;
    }
    if ui.button(t!("style.add_stroke")).clicked() {
        title.style.strokes.push(TextStroke::default());
        changed = true;
    }
    changed
}

fn shadow_section(ui: &mut egui::Ui, title: &mut TitleParams, frame: (f32, f32)) -> bool {
    let mut changed = false;
    let (open, reset) = section_header(ui, &t!("style.shadow"), true);
    if reset {
        title.style.shadows.clear();
        return true;
    }
    if !open {
        return false;
    }
    let mut remove = None;
    for (i, s) in title.style.shadows.iter_mut().enumerate() {
        if item_header(ui, format!("{} {}", t!("style.shadow"), i + 1)) {
            remove = Some(i);
        }
        let d = TextShadow::default();
        changed |= color_field(ui, t!("props.color").to_string(), &mut s.color, d.color);
        let row = param_row(ui, &t!("style.offset"), None, |ui| {
            axis_field(ui, "X", &mut s.offset[0], 0.5, 0, -frame.0..=frame.0)
                | axis_field(ui, "Y", &mut s.offset[1], 0.5, 0, -frame.1..=frame.1)
        });
        if row.reset {
            s.offset = d.offset;
        }
        changed |= row.changed || row.reset;
        changed |= slider_row(
            ui,
            t!("style.blur").to_string(),
            &mut s.blur,
            d.blur,
            0.0..=100.0,
            0,
        );
        changed |= slider_row(
            ui,
            t!("props.opacity").to_string(),
            &mut s.opacity,
            d.opacity,
            0.0..=100.0,
            0,
        );
    }
    if let Some(i) = remove {
        title.style.shadows.remove(i);
        changed = true;
    }
    ui.horizontal(|ui| {
        if ui.button(t!("style.add_shadow")).clicked() {
            title.style.shadows.push(TextShadow::default());
            changed = true;
        }
        if ui.button(t!("style.add_hard_shadow")).clicked() {
            title.style.shadows.push(TextShadow {
                blur: 0.0,
                opacity: 100.0,
                ..TextShadow::default()
            });
            changed = true;
        }
    });
    changed
}

fn glow_section(ui: &mut egui::Ui, title: &mut TitleParams) -> bool {
    let mut changed = false;
    let (open, reset) = section_header(ui, &t!("style.glow"), true);
    if reset {
        title.style.glows.clear();
        return true;
    }
    if !open {
        return false;
    }
    let mut remove = None;
    for (i, g) in title.style.glows.iter_mut().enumerate() {
        if item_header(ui, format!("{} {}", t!("style.glow"), i + 1)) {
            remove = Some(i);
        }
        let d = TextGlow::default();
        changed |= color_field(ui, t!("props.color").to_string(), &mut g.color, d.color);
        changed |= slider_row(
            ui,
            t!("style.radius").to_string(),
            &mut g.radius,
            d.radius,
            0.0..=200.0,
            0,
        );
        changed |= slider_row(
            ui,
            t!("style.intensity").to_string(),
            &mut g.intensity,
            d.intensity,
            0.0..=3.0,
            2,
        );
    }
    if let Some(i) = remove {
        title.style.glows.remove(i);
        changed = true;
    }
    if ui.button(t!("style.add_glow")).clicked() {
        title.style.glows.push(TextGlow::default());
        changed = true;
    }
    changed
}

fn extrude_section(ui: &mut egui::Ui, title: &mut TitleParams) -> bool {
    let mut changed = false;
    let (open, reset) = section_header(ui, "3D", true);
    if reset {
        title.style.extrude = None;
        return true;
    }
    if !open {
        return false;
    }
    let mut on = title.style.extrude.is_some();
    ui.horizontal(|ui| {
        if crate::properties_panel::toggle_switch(ui, &mut on).changed() {
            title.style.extrude = on.then(TextExtrude::default);
            changed = true;
        }
        ui.label(t!("style.extrude"));
    });
    if let Some(e) = &mut title.style.extrude {
        let d = TextExtrude::default();
        changed |= color_field(ui, t!("props.color").to_string(), &mut e.color, d.color);
        changed |= slider_row(
            ui,
            t!("style.depth").to_string(),
            &mut e.depth,
            d.depth,
            0.0..=100.0,
            0,
        );
        changed |= slider_row(
            ui,
            t!("style.angle").to_string(),
            &mut e.angle,
            d.angle,
            -180.0..=180.0,
            0,
        );
    }
    changed
}

pub(crate) fn shape_label(shape: BackgroundShape) -> std::borrow::Cow<'static, str> {
    match shape {
        BackgroundShape::RoundedRect => t!("style.shape_rect"),
        BackgroundShape::Pill => t!("style.shape_pill"),
        BackgroundShape::Ellipse => t!("style.shape_ellipse"),
        BackgroundShape::Speech => t!("style.shape_speech"),
        BackgroundShape::Thought => t!("style.shape_thought"),
        BackgroundShape::Burst => t!("style.shape_burst"),
    }
}

/// Shape, gradient, padding and rotation of the background, after the rows
/// the section already had.
pub(crate) fn background_extras(ui: &mut egui::Ui, title: &mut TitleParams) -> bool {
    let bg = &mut title.background;
    let d = vv_core::TitleBackground::default();
    let mut changed = false;
    let items = [
        (
            BackgroundShape::RoundedRect,
            t!("style.shape_rect").to_string(),
            true,
        ),
        (
            BackgroundShape::Pill,
            t!("style.shape_pill").to_string(),
            true,
        ),
        (
            BackgroundShape::Ellipse,
            t!("style.shape_ellipse").to_string(),
            true,
        ),
        (
            BackgroundShape::Speech,
            t!("style.shape_speech").to_string(),
            true,
        ),
        (
            BackgroundShape::Thought,
            t!("style.shape_thought").to_string(),
            true,
        ),
        (
            BackgroundShape::Burst,
            t!("style.shape_burst").to_string(),
            true,
        ),
    ];
    let row = param_row(ui, &t!("style.shape"), None, |ui| {
        preview_combo(ui, "bg_shape", &mut bg.shape, &items, None, None, None)
    });
    changed |= row.changed;
    let mut gradient = bg.gradient.is_some();
    ui.horizontal(|ui| {
        if crate::properties_panel::toggle_switch(ui, &mut gradient).changed() {
            bg.gradient = gradient.then(|| {
                Gradient::two_stops(
                    GradientKind::Linear,
                    bg.color,
                    Rgba {
                        r: 1.0,
                        g: 0.35,
                        b: 0.75,
                        a: 1.0,
                    },
                )
            });
            changed = true;
        }
        ui.label(t!("style.gradient"));
    });
    if let Some(g) = &mut bg.gradient {
        changed |= gradient_editor(ui, g, None, &mut RowResponse::default());
    }
    changed |= slider_row(
        ui,
        t!("style.padding").to_string(),
        &mut bg.padding,
        d.padding,
        0.0..=2.0,
        2,
    );
    changed |= slider_row(
        ui,
        t!("props.rotation").to_string(),
        &mut bg.rotation,
        d.rotation,
        -180.0..=180.0,
        0,
    );
    changed
}

/// All the sections except the background, which lives in `title_editor`.
/// Returns whether anything changed; `offset_row` reports the keyframe
/// controls of the gradient offset.
pub(crate) fn style_sections(
    ui: &mut egui::Ui,
    title: &mut TitleParams,
    offset_key: RowKeyframe,
    timeline_size: (u32, u32),
    offset_row: &mut RowResponse,
) -> bool {
    let frame = (timeline_size.0 as f32, timeline_size.1 as f32);
    ui.add_space(crate::theme::SPACE_2);
    live_preview(ui, title, timeline_size);
    ui.add_space(crate::theme::SPACE_1);
    let mut changed = fill_section(ui, title, offset_key, offset_row);
    changed |= stroke_section(ui, title);
    changed |= shadow_section(ui, title, frame);
    changed |= glow_section(ui, title);
    changed |= extrude_section(ui, title);
    changed
}

impl crate::VenturiApp {
    /// «Bubble»: one click puts the title in a speech balloon, white with a
    /// black outline; the Background section tunes it afterwards.
    pub(crate) fn show_bubble_section(
        &mut self,
        ui: &mut egui::Ui,
        targets: &[crate::PanelTarget],
        pending: &mut Vec<crate::properties_panel::BoxedCommand>,
    ) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let (open, _) =
            crate::properties_panel::section_header(ui, &t!("props.bubble_effects"), false);
        if !open {
            return;
        }
        let tl = &self.session.project.timelines[timeline_id];
        let titles: Vec<(usize, vv_core::ClipId, TitleParams)> = targets
            .iter()
            .filter(|t| t.is_text)
            .filter_map(|t| {
                let title = tl.clip(t.track_index, t.clip_id)?.effects.title.clone()?;
                Some((t.track_index, t.clip_id, title))
            })
            .collect();
        let current = titles
            .first()
            .map(|(_, _, t)| t.background.enabled.then_some(t.background.shape));
        let mut chosen: Option<Option<BackgroundShape>> = None;
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(current == Some(None), t!("props.curve_none"))
                .clicked()
            {
                chosen = Some(None);
            }
            for shape in BackgroundShape::BUBBLES {
                if ui
                    .selectable_label(current == Some(Some(shape)), shape_label(shape))
                    .clicked()
                {
                    chosen = Some(Some(shape));
                }
            }
        });
        let Some(choice) = chosen else {
            return;
        };
        for (track, clip, mut title) in titles {
            match choice {
                None => title.background.enabled = false,
                Some(shape) => {
                    let bg = &mut title.background;
                    if !bg.enabled || !BackgroundShape::BUBBLES.contains(&bg.shape) {
                        bg.color = Rgba::WHITE;
                        bg.outline_color = Rgba::BLACK;
                        bg.outline_width = 4.0;
                        bg.opacity = 100.0;
                        bg.padding = bg.padding.max(0.6);
                        title.color = Rgba::BLACK;
                    }
                    title.background.enabled = true;
                    title.background.shape = shape;
                }
            }
            pending.push(Box::new(vv_core::set_clip_title(
                timeline_id,
                track,
                clip,
                title,
            )));
        }
    }
}
