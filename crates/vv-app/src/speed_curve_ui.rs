//! The "Curve" side of the speed tab: presets and a graph of draggable points.

use egui::{Color32, Pos2, Rect, Stroke, pos2, vec2};
use vv_core::speed_curve::{MAX_SPEED, MIN_SPEED, normalized, speed_at};
use vv_core::{ClipId, CurvePreset, SetClipSpeedCurve};

use crate::{PanelTarget, VenturiApp, theme, timeline_ui};

const GRAPH_HEIGHT: f32 = 140.0;
const POINT_RADIUS: f32 = 5.0;

fn preset_label(preset: CurvePreset) -> std::borrow::Cow<'static, str> {
    match preset {
        CurvePreset::RampUp => t!("props.curve_ramp_up"),
        CurvePreset::RampDown => t!("props.curve_ramp_down"),
        CurvePreset::SlowMiddle => t!("props.curve_slow_middle"),
        CurvePreset::FastMiddle => t!("props.curve_fast_middle"),
        CurvePreset::FlashIn => t!("props.curve_flash_in"),
        CurvePreset::FlashOut => t!("props.curve_flash_out"),
        CurvePreset::Pulse => t!("props.curve_pulse"),
    }
}

/// Speeds on a log scale, so 0.5x and 2x sit at the same distance from 1x.
fn speed_to_y(rect: Rect, speed: f32) -> f32 {
    let t = (speed.ln() - MIN_SPEED.ln()) / (MAX_SPEED.ln() - MIN_SPEED.ln());
    rect.bottom() - t * rect.height()
}

fn y_to_speed(rect: Rect, y: f32) -> f32 {
    let t = ((rect.bottom() - y) / rect.height()).clamp(0.0, 1.0);
    (MIN_SPEED.ln() + t * (MAX_SPEED.ln() - MIN_SPEED.ln())).exp()
}

fn to_screen(rect: Rect, [x, speed]: [f32; 2]) -> Pos2 {
    pos2(rect.left() + x * rect.width(), speed_to_y(rect, speed))
}

/// Edits `points` in place; `true` when a gesture ended and the curve should be committed.
fn graph(ui: &mut egui::Ui, points: &mut Vec<[f32; 2]>) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), GRAPH_HEIGHT),
        egui::Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, theme::RADIUS_CONTROL, theme::BG_INSET);
    for speed in [0.1_f32, 0.5, 1.0, 2.0, 5.0, 10.0] {
        let y = speed_to_y(rect, speed);
        let color = if speed == 1.0 {
            theme::TEXT_DISABLED
        } else {
            theme::BORDER_SUBTLE
        };
        painter.hline(rect.x_range(), y, Stroke::new(1.0, color));
        painter.text(
            pos2(rect.left() + 4.0, y - 1.0),
            egui::Align2::LEFT_BOTTOM,
            format!("{speed}x"),
            egui::FontId::proportional(theme::FONT_SMALL - 1.0),
            theme::TEXT_SECONDARY,
        );
    }
    let line: Vec<Pos2> = (0..=64)
        .map(|i| {
            let x = i as f64 / 64.0;
            to_screen(rect, [x as f32, speed_at(points, x) as f32])
        })
        .collect();
    painter.add(egui::Shape::line(line, Stroke::new(2.0, theme::ACCENT)));

    let drag_id = ui.id().with("speed_curve_drag");
    let mut dragged: Option<usize> = ui.data(|d| d.get_temp(drag_id));
    let pointer = response.interact_pointer_pos();
    let nearest = |pos: Pos2, points: &[[f32; 2]]| {
        points
            .iter()
            .enumerate()
            .map(|(i, p)| (i, to_screen(rect, *p).distance(pos)))
            .filter(|(_, d)| *d <= POINT_RADIUS * 2.5)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    let mut commit = false;
    if response.drag_started()
        && let Some(pos) = pointer
    {
        dragged = nearest(pos, points);
    }
    if let (Some(i), Some(pos)) = (dragged, pointer)
        && response.dragged()
    {
        let last = points.len() - 1;
        // The ends stay at 0 and 1; the others stay between their neighbours.
        let x = if i == 0 {
            0.0
        } else if i == last {
            1.0
        } else {
            let lo = points[i - 1][0] + 0.01;
            let hi = points[i + 1][0] - 0.01;
            ((pos.x - rect.left()) / rect.width()).clamp(lo, hi.max(lo))
        };
        points[i] = [x, y_to_speed(rect, pos.y)];
    }
    if response.drag_stopped() && dragged.is_some() {
        dragged = None;
        commit = true;
    }
    if response.double_clicked()
        && let Some(pos) = pointer
    {
        match nearest(pos, points) {
            Some(i) if i != 0 && i != points.len() - 1 => {
                points.remove(i);
            }
            Some(_) => {}
            None => {
                let x = ((pos.x - rect.left()) / rect.width()).clamp(0.01, 0.99);
                points.push([x, y_to_speed(rect, pos.y)]);
                *points = normalized(points);
            }
        }
        commit = true;
    }
    ui.data_mut(|d| match dragged {
        Some(i) => {
            d.insert_temp(drag_id, i);
        }
        None => d.remove::<usize>(drag_id),
    });
    for (i, p) in points.iter().enumerate() {
        let fill = if Some(i) == dragged {
            theme::TEXT_PRIMARY
        } else {
            theme::ACCENT
        };
        painter.circle(
            to_screen(rect, *p),
            POINT_RADIUS,
            fill,
            Stroke::new(1.0, Color32::BLACK),
        );
    }
    response.on_hover_text(t!("props.curve_graph_hint"));
    commit
}

impl VenturiApp {
    pub(crate) fn show_speed_curve(&mut self, ui: &mut egui::Ui, targets: &[PanelTarget]) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let timeline = &self.session.project.timelines[timeline_id];
        let fps = timeline.fps.as_f64().max(1e-9);
        let media_clips: Vec<&vv_core::Clip> = targets
            .iter()
            .filter_map(|t| timeline.clip(t.track_index, t.clip_id))
            .filter(|c| matches!(c.source, vv_core::ClipSource::Media(_)))
            .collect();
        let Some(first) = media_clips.first() else {
            ui.small(t!("props.speed_media_only"));
            return;
        };
        let clips: Vec<(usize, ClipId)> = targets
            .iter()
            .filter(|t| {
                timeline
                    .clip(t.track_index, t.clip_id)
                    .is_some_and(|c| matches!(c.source, vv_core::ClipSource::Media(_)))
            })
            .map(|t| (t.track_index, t.clip_id))
            .collect();
        let applied = first.active_curve().map(|c| c.points.clone());
        let base_secs = {
            let base = first
                .active_curve()
                .map_or(first.speed(), |c| c.base_speed)
                .as_f64();
            first.timeline_len as f64 / fps * first.speed().as_f64() / base.max(1e-9)
        };

        let mut chosen: Option<Option<Vec<[f32; 2]>>> = None;
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(applied.is_none(), t!("props.curve_none"))
                .clicked()
            {
                chosen = Some(None);
            }
            for preset in CurvePreset::ALL {
                let points = normalized(&preset.points());
                let on = applied.as_ref() == Some(&points);
                if ui.selectable_label(on, preset_label(preset)).clicked() {
                    chosen = Some(Some(points));
                }
            }
        });
        ui.add_space(6.0);

        let edit_key = egui::Id::new(("speed_curve_edit", first.id));
        let baseline = applied
            .clone()
            .unwrap_or_else(|| vec![[0.0, 1.0], [1.0, 1.0]]);
        let mut points: Vec<[f32; 2]> = ui
            .data(|d| d.get_temp(edit_key))
            .unwrap_or_else(|| baseline.clone());
        if graph(ui, &mut points) {
            chosen = Some(Some(normalized(&points)));
        }
        // Only a drag in progress lives here: otherwise the clip's curve is shown.
        if chosen.is_none() && points != baseline {
            ui.data_mut(|d| d.insert_temp(edit_key, points.clone()));
        }

        let shown = chosen.clone().unwrap_or(applied.clone());
        let mean = shown.as_deref().map_or(1.0, vv_core::speed_curve::mean);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t!("props.speed_duration")).color(theme::TEXT_SECONDARY));
            ui.label(timeline_ui::format_timecode(base_secs / mean, fps));
        });
        ui.small(t!("props.curve_silent"));

        if let Some(points) = chosen {
            ui.data_mut(|d| d.remove::<Vec<[f32; 2]>>(edit_key));
            self.session
                .apply(Box::new(SetClipSpeedCurve::new(timeline_id, clips, points)));
        }
    }
}
