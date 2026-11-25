//! Canvas section of the Video tab: what fills the frame behind a clip.

use super::*;
use crate::properties_panel::section_header;
use vv_core::{CANVAS_BLUR_RADII, Canvas, Rgba};

const SWATCHES: [Rgba; 8] = [
    Rgba::BLACK,
    Rgba::WHITE,
    Rgba {
        r: 0.5,
        g: 0.5,
        b: 0.5,
        a: 1.0,
    },
    Rgba {
        r: 0.9,
        g: 0.25,
        b: 0.25,
        a: 1.0,
    },
    Rgba {
        r: 0.98,
        g: 0.7,
        b: 0.2,
        a: 1.0,
    },
    Rgba {
        r: 0.25,
        g: 0.7,
        b: 0.4,
        a: 1.0,
    },
    Rgba {
        r: 0.2,
        g: 0.45,
        b: 0.9,
        a: 1.0,
    },
    Rgba {
        r: 0.6,
        g: 0.3,
        b: 0.85,
        a: 1.0,
    },
];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    None,
    Blur,
    Color,
    Image,
}

fn kind_of(canvas: Option<&Canvas>) -> Kind {
    match canvas {
        None => Kind::None,
        Some(Canvas::Blur { .. }) => Kind::Blur,
        Some(Canvas::Color(_)) => Kind::Color,
        Some(Canvas::Image(_)) => Kind::Image,
    }
}

fn to_color32(c: Rgba) -> egui::Color32 {
    egui::Rgba::from_rgba_unmultiplied(c.r, c.g, c.b, c.a).into()
}

impl VenturiApp {
    pub(crate) fn show_canvas_section(
        &mut self,
        ui: &mut egui::Ui,
        targets: &[PanelTarget],
        pending: &mut Vec<BoxedCommand>,
    ) {
        ui.add_space(crate::theme::SPACE_2);
        let (open, _) = section_header(ui, &t!("props.canvas"), false);
        if !open {
            return;
        }
        let Some(first) = targets.first() else {
            return;
        };
        let timeline = &self.session.project.timelines[first.timeline];
        let current = timeline
            .clip(first.track_index, first.clip_id)
            .and_then(|c| c.effects.canvas.clone());
        let kind = kind_of(current.as_ref());
        let mut chosen: Option<Option<Canvas>> = None;
        let mut pick_image = false;

        let shown_color = match &current {
            Some(Canvas::Color(c)) => to_color32(*c),
            _ => to_color32(SWATCHES[6]),
        };
        ui.horizontal_wrapped(|ui| {
            for (option, label, hint) in [
                (
                    Kind::None,
                    t!("props.canvas_none"),
                    t!("props.canvas_none_hint"),
                ),
                (
                    Kind::Blur,
                    t!("props.canvas_blur"),
                    t!("props.canvas_blur_hint"),
                ),
                (
                    Kind::Color,
                    t!("props.canvas_color"),
                    t!("props.canvas_color_hint"),
                ),
                (
                    Kind::Image,
                    t!("props.canvas_image"),
                    t!("props.canvas_image_hint"),
                ),
            ] {
                if canvas_tile(ui, option, kind == option, &label, &hint, shown_color).clicked()
                    && kind != option
                {
                    match option {
                        Kind::None => chosen = Some(None),
                        Kind::Blur => {
                            chosen = Some(Some(Canvas::Blur {
                                radius: CANVAS_BLUR_RADII[1],
                            }));
                        }
                        Kind::Color => chosen = Some(Some(Canvas::Color(Rgba::BLACK))),
                        Kind::Image => pick_image = true,
                    }
                }
            }
        });

        match &current {
            Some(Canvas::Blur { radius }) => {
                ui.label(t!("props.canvas_strength"));
                ui.horizontal(|ui| {
                    for (level, &value) in CANVAS_BLUR_RADII.iter().enumerate() {
                        let selected = (radius - value).abs() < 0.5;
                        if ui
                            .add(egui::Button::selectable(selected, (level + 1).to_string()))
                            .clicked()
                        {
                            chosen = Some(Some(Canvas::Blur { radius: value }));
                        }
                    }
                });
            }
            Some(Canvas::Color(color)) => {
                ui.horizontal_wrapped(|ui| {
                    for swatch in SWATCHES {
                        let size = egui::vec2(theme::CONTROL_HEIGHT, theme::CONTROL_HEIGHT);
                        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
                        ui.painter()
                            .rect_filled(rect, theme::RADIUS_CONTROL, to_color32(swatch));
                        if swatch == *color {
                            ui.painter().rect_stroke(
                                rect,
                                theme::RADIUS_CONTROL,
                                egui::Stroke::new(2.0, theme::ACCENT),
                                egui::StrokeKind::Outside,
                            );
                        }
                        if response.clicked() {
                            chosen = Some(Some(Canvas::Color(swatch)));
                        }
                    }
                    let mut rgba = [color.r, color.g, color.b, 1.0];
                    if ui.color_edit_button_rgba_unmultiplied(&mut rgba).changed() {
                        chosen = Some(Some(Canvas::Color(Rgba {
                            r: rgba[0],
                            g: rgba[1],
                            b: rgba[2],
                            a: 1.0,
                        })));
                    }
                });
            }
            Some(Canvas::Image(path)) => {
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(vv_session::file_label(path)).truncate());
                    if ui.button(t!("props.canvas_change_image")).clicked() {
                        pick_image = true;
                    }
                });
            }
            None => {}
        }

        if current.is_some()
            && ui
                .button(t!("props.canvas_apply_all"))
                .on_hover_text(t!("props.canvas_apply_all_hint"))
                .clicked()
        {
            let track = &timeline.tracks[first.track_index];
            for clip in &track.clips {
                if clip.effects.canvas != current {
                    pending.push(Box::new(vv_core::set_clip_canvas(
                        first.timeline,
                        first.track_index,
                        clip.id,
                        current.clone(),
                    )));
                }
            }
        }

        if pick_image {
            self.pick_canvas_image(targets);
        }
        if let Some(canvas) = chosen {
            pending.extend(canvas_commands(targets, &canvas));
        }
    }

    fn pick_canvas_image(&mut self, targets: &[PanelTarget]) {
        let clips = targets
            .iter()
            .map(|t| (t.timeline, t.track_index, t.clip_id))
            .collect();
        self.spawn_file_dialog(project_io::DialogKind::CanvasImage(clips), |dlg| {
            dlg.add_filter(
                "images",
                &project_io::case_insensitive(vv_media::IMAGE_EXTENSIONS),
            )
            .pick_file()
        });
    }

    /// The picked picture becomes the canvas of the clips it was picked for.
    pub(crate) fn set_canvas_image(
        &mut self,
        clips: Vec<(TimelineId, usize, ClipId)>,
        path: PathBuf,
    ) {
        let preload = path.clone();
        std::thread::spawn(move || frame_provider::preload_canvas_image(&preload));
        let group = self.session.history.begin_group();
        for (timeline, track_index, clip_id) in clips {
            self.session.apply(Box::new(vv_core::set_clip_canvas(
                timeline,
                track_index,
                clip_id,
                Some(Canvas::Image(path.clone())),
            )));
        }
        self.session
            .history
            .end_group_as(group, vv_core::CommandLabel::Canvas);
    }
}

const PREVIEW: egui::Vec2 = egui::vec2(64.0, 36.0);
const SKY: egui::Color32 = egui::Color32::from_rgb(0x6f, 0xa8, 0xdc);
const SUN: egui::Color32 = egui::Color32::from_rgb(0xff, 0xd2, 0x5a);
const GROUND: egui::Color32 = egui::Color32::from_rgb(0x4c, 0x9a, 0x5b);

/// A choice drawn as what it does: a vertical clip in a wide frame, the
/// sides filled as the choice would fill them.
fn canvas_tile(
    ui: &mut egui::Ui,
    kind: Kind,
    selected: bool,
    label: &str,
    hint: &str,
    color: egui::Color32,
) -> egui::Response {
    let pad = theme::SPACE_1;
    let label_h = theme::FONT_SMALL + 4.0;
    let size = PREVIEW + egui::vec2(2.0 * pad, 2.0 * pad + label_h);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let response = response.on_hover_text(hint);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let fill = if selected {
            theme::BG_SELECTED
        } else if response.hovered() {
            theme::BG_RAISED_HOVER
        } else {
            theme::BG_RAISED
        };
        painter.rect_filled(rect, theme::RADIUS_CONTROL, fill);
        if selected {
            painter.rect_stroke(
                rect,
                theme::RADIUS_CONTROL,
                egui::Stroke::new(1.5, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        let frame = egui::Rect::from_min_size(rect.min + egui::vec2(pad, pad), PREVIEW);
        paint_canvas_preview(&painter.with_clip_rect(frame), frame, kind, color);
        painter.text(
            egui::pos2(rect.center().x, frame.bottom() + pad / 2.0 + label_h / 2.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(theme::FONT_SMALL),
            if selected {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_SECONDARY
            },
        );
    }
    response
}

fn paint_canvas_preview(
    painter: &egui::Painter,
    frame: egui::Rect,
    kind: Kind,
    color: egui::Color32,
) {
    let clip = egui::Rect::from_center_size(
        frame.center(),
        egui::vec2(frame.height() * 9.0 / 16.0, frame.height()),
    );
    match kind {
        Kind::None => {
            painter.rect_filled(frame, 0.0, egui::Color32::BLACK);
        }
        Kind::Blur => paint_blurred_scene(painter, frame),
        Kind::Color => {
            painter.rect_filled(frame, 0.0, color);
        }
        Kind::Image => paint_picture(painter, frame),
    }
    paint_scene(&painter.with_clip_rect(clip), clip);
    painter.rect_stroke(
        clip,
        0.0,
        egui::Stroke::new(1.0, egui::Color32::from_black_alpha(120)),
        egui::StrokeKind::Outside,
    );
}

/// The clip itself: sky, sun and a hill.
fn paint_scene(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_filled(rect, 0.0, SKY);
    let unit = rect.width().min(rect.height());
    painter.circle_filled(rect.lerp_inside(egui::vec2(0.68, 0.28)), unit * 0.16, SUN);
    painter.circle_filled(
        rect.lerp_inside(egui::vec2(0.3, 1.05)),
        rect.height() * 0.42,
        GROUND,
    );
    painter.rect_filled(
        egui::Rect::from_min_max(rect.lerp_inside(egui::vec2(0.0, 0.82)), rect.max),
        0.0,
        GROUND,
    );
}

/// The clip enlarged to fill the frame with its shapes melted into halos.
fn paint_blurred_scene(painter: &egui::Painter, frame: egui::Rect) {
    painter.rect_filled(frame, 0.0, SKY.gamma_multiply(0.8));
    let unit = frame.height();
    let sun = frame.lerp_inside(egui::vec2(0.75, 0.1));
    for (radius, alpha) in [(0.7, 0.18), (0.5, 0.25), (0.32, 0.35)] {
        painter.circle_filled(sun, unit * radius, SUN.gamma_multiply(alpha));
    }
    let ground_top = frame.lerp_inside(egui::vec2(0.0, 0.55)).y;
    for (step, alpha) in [(0.0, 0.25), (0.12, 0.45), (0.24, 0.8)] {
        let top = ground_top + unit * step;
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(frame.left(), top), frame.max),
            0.0,
            GROUND.gamma_multiply(alpha),
        );
    }
    painter.rect_filled(frame, 0.0, egui::Color32::from_black_alpha(50));
}

/// A picture other than the clip: a sunset over mountains.
fn paint_picture(painter: &egui::Painter, frame: egui::Rect) {
    let horizon = frame.lerp_inside(egui::vec2(0.0, 0.55)).y;
    painter.rect_filled(
        egui::Rect::from_min_max(frame.min, egui::pos2(frame.right(), horizon)),
        0.0,
        egui::Color32::from_rgb(0xf0, 0x96, 0x5a),
    );
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(frame.left(), horizon), frame.max),
        0.0,
        egui::Color32::from_rgb(0x6b, 0x4a, 0x8c),
    );
    let mountain = egui::Color32::from_rgb(0x3b, 0x2e, 0x5a);
    for (left, peak, right, height) in [(-0.1, 0.12, 0.35, 0.45), (0.62, 0.85, 1.1, 0.5)] {
        painter.add(egui::Shape::convex_polygon(
            vec![
                frame.lerp_inside(egui::vec2(left, 1.0)),
                frame.lerp_inside(egui::vec2(peak, 1.0 - height)),
                frame.lerp_inside(egui::vec2(right, 1.0)),
            ],
            mountain,
            egui::Stroke::NONE,
        ));
    }
}

fn canvas_commands(targets: &[PanelTarget], canvas: &Option<Canvas>) -> Vec<BoxedCommand> {
    targets
        .iter()
        .map(|t| {
            Box::new(vv_core::set_clip_canvas(
                t.timeline,
                t.track_index,
                t.clip_id,
                canvas.clone(),
            )) as BoxedCommand
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/canvas_ui.rs"]
mod tests;
