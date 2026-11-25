//! Command bar above the timeline, the empty-timeline placeholder and the
//! hand-drawn icons shared with the track headers (some platforms lack the
//! glyphs in egui's fonts).

use crate::settings::{Action, Keymap};
use crate::timeline_ui::{TimelineCommand, TimelineDrag, TimelineState, TimelineTool};

type PaintIcon = dyn Fn(&egui::Painter, egui::Pos2, egui::Color32);

const BUTTON_SIZE: egui::Vec2 = egui::vec2(28.0, 26.0);
const ZOOM_SLIDER_WIDTH: f32 = 96.0;
const ZOOM_STEP: f32 = 0.1;
const TOOL_ROW_SIZE: egui::Vec2 = egui::vec2(200.0, 28.0);

const TOOLS: [(TimelineTool, Action); 5] = [
    (TimelineTool::Select, Action::SelectTool),
    (TimelineTool::Blade, Action::BladeTool),
    (TimelineTool::SelectLeft, Action::SelectLeftTool),
    (TimelineTool::SelectRight, Action::SelectRightTool),
    (TimelineTool::Slip, Action::SlipTool),
];

fn tool_label(tool: TimelineTool) -> std::borrow::Cow<'static, str> {
    match tool {
        TimelineTool::Select => t!("timeline_bar.mode_select"),
        TimelineTool::Blade => t!("timeline_bar.mode_blade"),
        TimelineTool::SelectLeft => t!("timeline_bar.mode_select_left"),
        TimelineTool::SelectRight => t!("timeline_bar.mode_select_right"),
        TimelineTool::Slip => t!("timeline_bar.mode_slip"),
    }
}

/// The bar above the timeline. Requests go to `state.commands`, the
/// toggles are flipped in place.
pub(crate) fn show_bar(
    ui: &mut egui::Ui,
    state: &mut TimelineState,
    snapping: &mut bool,
    keymap: &Keymap,
    can_undo: bool,
    can_redo: bool,
    can_auto_cut: bool,
    can_voiceover: bool,
) {
    let has_selection = !state.selected.is_empty();
    ui.horizontal(|ui| {
        ui.set_height(BUTTON_SIZE.y);
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            // Right to left: the first added is the rightmost.
            zoom_controls(ui, state, keymap);
            divider(ui);
            let toggles: [(&mut bool, &PaintIcon, _, _); 4] = [
                (
                    &mut state.preview_axis,
                    &paint_preview_axis,
                    t!("timeline_bar.preview_axis_hint"),
                    Action::TogglePreviewAxis,
                ),
                (
                    &mut state.linkage,
                    &paint_linkage,
                    t!("timeline_bar.linkage_hint"),
                    Action::ToggleLinkage,
                ),
                (
                    snapping,
                    &paint_snapping,
                    t!("timeline_bar.snapping_hint"),
                    Action::ToggleSnapping,
                ),
                (
                    &mut state.magnet,
                    &paint_magnet,
                    t!("timeline_bar.magnet_hint"),
                    Action::ToggleMagnet,
                ),
            ];
            for (value, paint, label, action) in toggles {
                if icon_button(ui, true, *value, paint)
                    .on_hover_text(keymap.menu_label(&label, action))
                    .clicked()
                {
                    *value = !*value;
                }
            }
            // A narrow window scrolls the commands instead of clipping them.
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt("timeline_bar_commands")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            tool_menu(ui, state, keymap);
                            if icon_button(ui, can_voiceover, false, &paint_microphone)
                                .on_hover_text(t!("voiceover.title"))
                                .clicked()
                            {
                                state.commands.push(TimelineCommand::Voiceover);
                            }
                            divider(ui);

                            let mut command =
                                |ui: &mut egui::Ui,
                                 enabled: bool,
                                 paint: &PaintIcon,
                                 label: std::borrow::Cow<str>,
                                 action: Option<Action>,
                                 command: TimelineCommand| {
                                    let tip = match action {
                                        Some(action) => keymap.menu_label(&label, action),
                                        None => label.into_owned(),
                                    };
                                    if icon_button(ui, enabled, false, paint)
                                        .on_hover_text(tip)
                                        .clicked()
                                    {
                                        state.commands.push(command);
                                    }
                                };
                            command(
                                ui,
                                can_undo,
                                &paint_undo,
                                t!("timeline_bar.undo"),
                                Some(Action::Undo),
                                TimelineCommand::Undo,
                            );
                            command(
                                ui,
                                can_redo,
                                &paint_redo,
                                t!("timeline_bar.redo"),
                                Some(Action::Redo),
                                TimelineCommand::Redo,
                            );
                            divider(ui);
                            command(
                                ui,
                                true,
                                &paint_split,
                                t!("timeline_bar.split"),
                                Some(Action::Split),
                                TimelineCommand::SplitAtPlayhead,
                            );
                            command(
                                ui,
                                true,
                                &|p, c, col| paint_delete_side(p, c, col, true),
                                t!("timeline_bar.delete_left"),
                                Some(Action::DeleteLeft),
                                TimelineCommand::DeleteLeft,
                            );
                            command(
                                ui,
                                true,
                                &|p, c, col| paint_delete_side(p, c, col, false),
                                t!("timeline_bar.delete_right"),
                                Some(Action::DeleteRight),
                                TimelineCommand::DeleteRight,
                            );
                            command(
                                ui,
                                has_selection,
                                &paint_trash,
                                t!("timeline_bar.delete"),
                                Some(Action::Delete),
                                TimelineCommand::Delete,
                            );
                            divider(ui);
                            command(
                                ui,
                                true,
                                &paint_freeze,
                                t!("timeline.freeze_frame_action"),
                                None,
                                TimelineCommand::FreezeFrame,
                            );
                            command(
                                ui,
                                has_selection,
                                &paint_mirror,
                                t!("timeline.mirror"),
                                None,
                                TimelineCommand::Mirror,
                            );

                            if has_selection
                                && ui
                                    .add_enabled(
                                        can_auto_cut,
                                        egui::Button::new(t!("auto_cut.title")),
                                    )
                                    .on_disabled_hover_text(t!("auto_cut.select_source"))
                                    .clicked()
                            {
                                state.commands.push(TimelineCommand::AutoCut);
                            }
                            if has_selection
                                && ui
                                    .add_enabled(
                                        can_auto_cut,
                                        egui::Button::new(t!("auto_zoom.title")),
                                    )
                                    .on_hover_text(t!("auto_zoom.button_hint"))
                                    .on_disabled_hover_text(t!("auto_zoom.select_video"))
                                    .clicked()
                            {
                                state.commands.push(TimelineCommand::AutoZoom);
                            }
                            if has_selection
                                && ui
                                    .button(t!("reframe.button"))
                                    .on_hover_text(t!("reframe.button_hint"))
                                    .clicked()
                            {
                                state.commands.push(TimelineCommand::Reframe);
                            }
                            if ui.button(t!("dub.title")).clicked() {
                                state.commands.push(TimelineCommand::Dub);
                            }
                            if ui
                                .button(t!("beat.button"))
                                .on_hover_text(t!("beat.button_hint"))
                                .clicked()
                            {
                                state.commands.push(TimelineCommand::BeatSync);
                            }
                        });
                    });
            });
        });
    });
}

fn zoom_controls(ui: &mut egui::Ui, state: &mut TimelineState, keymap: &Keymap) {
    let mut fraction = state.zoom_fraction();
    if icon_button(ui, true, false, &|p, c, col| paint_zoom(p, c, col, true))
        .on_hover_text(keymap.menu_label(&t!("timeline_bar.zoom_in"), Action::ZoomIn))
        .clicked()
    {
        fraction = (fraction + ZOOM_STEP).min(1.0);
    }
    zoom_slider(ui, &mut fraction).on_hover_text(t!("timeline_bar.zoom"));
    if icon_button(ui, true, false, &|p, c, col| paint_zoom(p, c, col, false))
        .on_hover_text(keymap.menu_label(&t!("timeline_bar.zoom_out"), Action::ZoomOut))
        .clicked()
    {
        fraction = (fraction - ZOOM_STEP).max(0.0);
    }
    if fraction != state.zoom_fraction() {
        state.set_zoom_fraction(fraction);
    }
    if icon_button(ui, true, false, &paint_fit)
        .on_hover_text(keymap.menu_label(&t!("timeline_bar.fit"), Action::FitTimeline))
        .clicked()
    {
        state.fit_requested = true;
    }
}

/// A thin line with a small white knob.
fn zoom_slider(ui: &mut egui::Ui, fraction: &mut f32) -> egui::Response {
    const KNOB_RADIUS: f32 = 5.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ZOOM_SLIDER_WIDTH, BUTTON_SIZE.y),
        egui::Sense::click_and_drag(),
    );
    let track = rect.shrink2(egui::vec2(KNOB_RADIUS, 0.0)).x_range();
    if let Some(pos) = response.interact_pointer_pos() {
        *fraction = ((pos.x - track.min) / track.span()).clamp(0.0, 1.0);
    }
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let y = rect.center().y;
        let knob_x = track.min + track.span() * *fraction;
        p.hline(track, y, egui::Stroke::new(2.0, crate::theme::BG_SELECTED));
        p.hline(
            egui::Rangef::new(track.min, knob_x),
            y,
            egui::Stroke::new(2.0, crate::theme::TEXT_SECONDARY),
        );
        let radius = if response.hovered() || response.dragged() {
            KNOB_RADIUS + 1.0
        } else {
            KNOB_RADIUS
        };
        p.circle_filled(egui::pos2(knob_x, y), radius, egui::Color32::WHITE);
    }
    response
}

fn divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, BUTTON_SIZE.y), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.shrink2(egui::vec2(0.0, 6.0)).y_range(),
        egui::Stroke::new(1.0, crate::theme::BORDER_SUBTLE.gamma_multiply(1.6)),
    );
}

/// The current tool's icon with a chevron; the popup lists the tools with
/// their shortcut on the right, as in CapCut.
fn tool_menu(ui: &mut egui::Ui, state: &mut TimelineState, keymap: &Keymap) {
    let size = egui::vec2(BUTTON_SIZE.x + 12.0, BUTTON_SIZE.y);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    if ui.is_rect_visible(rect) {
        let color = if response.hovered() || open {
            crate::theme::TEXT_PRIMARY
        } else {
            crate::theme::TEXT_SECONDARY
        };
        if response.hovered() || open {
            ui.painter()
                .rect_filled(rect, 4.0, crate::theme::BG_RAISED_HOVER);
        }
        let icon_center = egui::pos2(rect.left() + BUTTON_SIZE.x / 2.0, rect.center().y);
        paint_tool_icon(ui.painter(), icon_center, state.tool, color);
        let chevron = egui::pos2(rect.right() - 8.0, rect.center().y);
        ui.painter().add(egui::Shape::line(
            vec![
                chevron + egui::vec2(-3.0, -1.5),
                chevron + egui::vec2(0.0, 1.5),
                chevron + egui::vec2(3.0, -1.5),
            ],
            egui::Stroke::new(1.2, color),
        ));
    }
    let response =
        response.on_hover_text(keymap.menu_label(&tool_label(state.tool), tool_action(state.tool)));
    egui::Popup::menu(&response).show(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for (tool, action) in TOOLS {
            if tool_row(ui, tool, state.tool == tool, keymap, action).clicked() {
                state.tool = tool;
                ui.close();
            }
        }
    });
}

fn tool_action(tool: TimelineTool) -> Action {
    TOOLS
        .iter()
        .find(|(t, _)| *t == tool)
        .map_or(Action::SelectTool, |(_, a)| *a)
}

fn tool_row(
    ui: &mut egui::Ui,
    tool: TimelineTool,
    current: bool,
    keymap: &Keymap,
    action: Action,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(TOOL_ROW_SIZE, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if response.hovered() {
            p.rect_filled(rect, 4.0, crate::theme::BG_RAISED_HOVER);
        }
        let color = if current {
            crate::theme::ACCENT
        } else {
            crate::theme::TEXT_PRIMARY
        };
        paint_tool_icon(
            p,
            egui::pos2(rect.left() + 16.0, rect.center().y),
            tool,
            color,
        );
        p.text(
            egui::pos2(rect.left() + 34.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            tool_label(tool),
            egui::FontId::proportional(crate::theme::FONT_SECTION),
            color,
        );
        if let Some(shortcut) = keymap.shortcuts(action).first() {
            p.text(
                egui::pos2(rect.right() - 10.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                shortcut.to_string(),
                egui::FontId::proportional(crate::theme::FONT_BODY),
                crate::theme::TEXT_SECONDARY,
            );
        }
    }
    response
}

fn icon_button(
    ui: &mut egui::Ui,
    enabled: bool,
    active: bool,
    paint: &PaintIcon,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(BUTTON_SIZE, sense);
    if ui.is_rect_visible(rect) {
        let hovered = enabled && response.hovered();
        if hovered {
            ui.painter()
                .rect_filled(rect, 4.0, crate::theme::BG_RAISED_HOVER);
        }
        let color = match (enabled, active, hovered) {
            (false, ..) => crate::theme::TEXT_DISABLED,
            (true, true, _) => crate::theme::ACCENT,
            (true, false, true) => crate::theme::TEXT_PRIMARY,
            (true, false, false) => crate::theme::TEXT_SECONDARY,
        };
        paint(ui.painter(), rect.center(), color);
    }
    response
}

fn stroke(color: egui::Color32) -> egui::Stroke {
    egui::Stroke::new(1.5, color)
}

fn paint_microphone(p: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    let capsule = egui::Rect::from_center_size(c + egui::vec2(0.0, -2.5), egui::vec2(6.0, 10.0));
    p.rect_stroke(capsule, 3.0, stroke(color), egui::StrokeKind::Inside);
    p.add(egui::Shape::line(
        arc(c, 5.5, 0.0, std::f32::consts::PI),
        stroke(color),
    ));
    p.line_segment(
        [c + egui::vec2(0.0, 5.5), c + egui::vec2(0.0, 8.5)],
        stroke(color),
    );
}

fn arc(center: egui::Pos2, radius: f32, from: f32, to: f32) -> Vec<egui::Pos2> {
    (0..=12)
        .map(|i| {
            let a = from + (to - from) * (i as f32 / 12.0);
            center + egui::vec2(a.cos(), a.sin()) * radius
        })
        .collect()
}

fn arrow_head(tip: egui::Pos2, dir: egui::Vec2, size: f32, color: egui::Color32) -> egui::Shape {
    let side = egui::vec2(-dir.y, dir.x);
    egui::Shape::convex_polygon(
        vec![
            tip,
            tip - dir * size + side * (size * 0.6),
            tip - dir * size - side * (size * 0.6),
        ],
        color,
        egui::Stroke::NONE,
    )
}

fn paint_undo(p: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    use std::f32::consts::PI;
    p.add(egui::Shape::line(
        arc(c, 6.0, -PI * 0.9, PI * 0.1),
        stroke(color),
    ));
    let tip = c + egui::vec2(-6.0 * 0.95, -6.0 * 0.3);
    p.add(arrow_head(tip, egui::vec2(0.0, -1.0), 5.0, color));
}

fn paint_redo(p: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    use std::f32::consts::PI;
    p.add(egui::Shape::line(
        arc(c, 6.0, PI * 0.9, PI * -0.1 + 2.0 * PI),
        stroke(color),
    ));
    let tip = c + egui::vec2(6.0 * 0.95, -6.0 * 0.3);
    p.add(arrow_head(tip, egui::vec2(0.0, -1.0), 5.0, color));
}

/// Two pieces of clip and the cut between them.
fn paint_split(p: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    for dx in [-8.0, 2.0] {
        let r = egui::Rect::from_min_size(c + egui::vec2(dx, -4.0), egui::vec2(6.0, 8.0));
        p.rect_stroke(r, 1.0, stroke(color), egui::StrokeKind::Inside);
    }
    p.line_segment(
        [c + egui::vec2(0.0, -8.0), c + egui::vec2(0.0, 8.0)],
        egui::Stroke::new(2.0, color),
    );
}

/// The playhead with the discarded side filled and crossed.
fn paint_delete_side(p: &egui::Painter, c: egui::Pos2, color: egui::Color32, left: bool) {
    let side = if left { -1.0 } else { 1.0 };
    let gone = egui::Rect::from_two_pos(c + egui::vec2(side * 9.0, -5.0), c + egui::vec2(0.0, 5.0));
    p.rect_filled(
        gone,
        1.0,
        egui::Color32::from_rgb(200, 70, 70).gamma_multiply(0.6),
    );
    p.line_segment([gone.left_top(), gone.right_bottom()], stroke(color));
    p.line_segment([gone.right_top(), gone.left_bottom()], stroke(color));
    p.line_segment(
        [c + egui::vec2(0.0, -8.0), c + egui::vec2(0.0, 8.0)],
        egui::Stroke::new(2.0, color),
    );
}

fn paint_trash(p: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    let s = stroke(color);
    p.line_segment([c + egui::vec2(-6.0, -5.0), c + egui::vec2(6.0, -5.0)], s);
    p.line_segment([c + egui::vec2(-2.0, -7.0), c + egui::vec2(2.0, -7.0)], s);
    p.add(egui::Shape::line(
        vec![
            c + egui::vec2(-5.0, -5.0),
            c + egui::vec2(-4.0, 7.0),
            c + egui::vec2(4.0, 7.0),
            c + egui::vec2(5.0, -5.0),
        ],
        s,
    ));
    for x in [-1.5, 1.5] {
        p.line_segment(
            [c + egui::vec2(x, -2.0), c + egui::vec2(x, 4.0)],
            egui::Stroke::new(1.0, color),
        );
    }
}

/// A snowflake: the frame under the playhead held as a still.
fn paint_freeze(p: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    let s = stroke(color);
    for i in 0..3 {
        let a = std::f32::consts::PI / 3.0 * i as f32 + std::f32::consts::FRAC_PI_2;
        let d = egui::vec2(a.cos(), a.sin()) * 7.0;
        p.line_segment([c - d, c + d], s);
        for end in [c - d, c + d] {
            let back = (c - end).normalized() * 3.0;
            let side = egui::vec2(-back.y, back.x);
            p.line_segment(
                [end + back * 0.6, end + back + side * 0.8],
                egui::Stroke::new(1.2, color),
            );
            p.line_segment(
                [end + back * 0.6, end + back - side * 0.8],
                egui::Stroke::new(1.2, color),
            );
        }
    }
}

/// Two triangles facing each other across an axis.
fn paint_mirror(p: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    p.add(egui::Shape::convex_polygon(
        vec![
            c + egui::vec2(-2.0, -6.0),
            c + egui::vec2(-2.0, 6.0),
            c + egui::vec2(-8.0, 6.0),
        ],
        color,
        egui::Stroke::NONE,
    ));
    p.add(egui::Shape::closed_line(
        vec![
            c + egui::vec2(2.0, -6.0),
            c + egui::vec2(2.0, 6.0),
            c + egui::vec2(8.0, 6.0),
        ],
        stroke(color),
    ));
    for y in [-8.0, -4.0, 0.0, 4.0] {
        p.line_segment(
            [c + egui::vec2(0.0, y), c + egui::vec2(0.0, y + 2.5)],
            egui::Stroke::new(1.2, color),
        );
    }
}

fn paint_magnet(p: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    use std::f32::consts::PI;
    let center = c + egui::vec2(0.0, 0.5);
    p.add(egui::Shape::line(
        arc(center, 5.0, 0.0, PI),
        egui::Stroke::new(3.0, color),
    ));
    for x in [-5.0, 5.0] {
        p.line_segment(
            [center + egui::vec2(x, 0.0), center + egui::vec2(x, -6.5)],
            egui::Stroke::new(3.0, color),
        );
    }
    let bg = crate::theme::BG_PANEL;
    for x in [-5.0, 5.0] {
        p.line_segment(
            [
                center + egui::vec2(x - 1.5, -4.0),
                center + egui::vec2(x + 1.5, -4.0),
            ],
            egui::Stroke::new(1.0, bg),
        );
    }
}

/// Two clips whose edges meet on a guide line.
fn paint_snapping(p: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    let s = stroke(color);
    p.rect_stroke(
        egui::Rect::from_min_max(c + egui::vec2(-8.0, -6.0), c + egui::vec2(-1.0, -1.0)),
        1.0,
        s,
        egui::StrokeKind::Inside,
    );
    p.rect_stroke(
        egui::Rect::from_min_max(c + egui::vec2(1.0, 1.0), c + egui::vec2(8.0, 6.0)),
        1.0,
        s,
        egui::StrokeKind::Inside,
    );
    p.line_segment(
        [c + egui::vec2(0.0, -8.0), c + egui::vec2(0.0, 8.0)],
        egui::Stroke::new(1.2, color),
    );
}

/// Two chain links.
fn paint_linkage(p: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    let s = stroke(color);
    let dir = egui::vec2(1.0, -1.0).normalized();
    for offset in [-3.2, 3.2] {
        let center = c + dir * offset;
        let half = dir * 3.0;
        let side = egui::vec2(-dir.y, dir.x) * 2.6;
        let link: Vec<egui::Pos2> = (0..=16)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / 16.0;
                center + half * a.cos() * 1.3 + side * a.sin()
            })
            .collect();
        p.add(egui::Shape::line(link, s));
    }
}

/// A playhead over a frame: the player follows the pointer.
fn paint_preview_axis(p: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    let s = stroke(color);
    p.rect_stroke(
        egui::Rect::from_center_size(c + egui::vec2(0.0, 1.5), egui::vec2(14.0, 10.0)),
        1.5,
        s,
        egui::StrokeKind::Inside,
    );
    p.line_segment(
        [c + egui::vec2(0.0, -6.0), c + egui::vec2(0.0, 8.0)],
        egui::Stroke::new(1.2, color),
    );
    p.add(egui::Shape::convex_polygon(
        vec![
            c + egui::vec2(-3.0, -8.0),
            c + egui::vec2(3.0, -8.0),
            c + egui::vec2(0.0, -5.0),
        ],
        color,
        egui::Stroke::NONE,
    ));
}

/// A magnifier with "+" or "-".
fn paint_zoom(p: &egui::Painter, c: egui::Pos2, color: egui::Color32, zoom_in: bool) {
    let s = stroke(color);
    let lens = c + egui::vec2(-1.5, -1.5);
    p.circle_stroke(lens, 5.0, s);
    p.line_segment(
        [lens + egui::vec2(3.6, 3.6), c + egui::vec2(6.5, 6.5)],
        egui::Stroke::new(2.0, color),
    );
    p.line_segment(
        [lens - egui::vec2(2.5, 0.0), lens + egui::vec2(2.5, 0.0)],
        egui::Stroke::new(1.2, color),
    );
    if zoom_in {
        p.line_segment(
            [lens - egui::vec2(0.0, 2.5), lens + egui::vec2(0.0, 2.5)],
            egui::Stroke::new(1.2, color),
        );
    }
}

/// Arrows pushing out to the edges: the whole timeline fits the view.
fn paint_fit(p: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
    let s = stroke(color);
    for dir in [-1.0f32, 1.0] {
        p.line_segment(
            [
                c + egui::vec2(dir * 8.0, -6.0),
                c + egui::vec2(dir * 8.0, 6.0),
            ],
            s,
        );
        p.line_segment(
            [
                c + egui::vec2(dir * 1.5, 0.0),
                c + egui::vec2(dir * 5.0, 0.0),
            ],
            s,
        );
        p.add(arrow_head(
            c + egui::vec2(dir * 6.5, 0.0),
            egui::vec2(dir, 0.0),
            3.0,
            color,
        ));
    }
}

pub(crate) fn paint_tool_icon(
    p: &egui::Painter,
    c: egui::Pos2,
    tool: TimelineTool,
    color: egui::Color32,
) {
    let s = stroke(color);
    match tool {
        TimelineTool::Select => {
            let tip = c + egui::vec2(-3.5, -7.0);
            let outline = [
                (0.0, 0.0),
                (0.0, 12.0),
                (3.0, 9.2),
                (5.2, 13.8),
                (7.2, 12.9),
                (5.0, 8.4),
                (8.8, 8.4),
            ]
            .map(|(x, y)| tip + egui::vec2(x, y));
            p.add(egui::Shape::closed_line(outline.to_vec(), s));
        }
        TimelineTool::Blade => {
            p.line_segment([c + egui::vec2(-6.0, 7.0), c + egui::vec2(6.0, -7.0)], s);
            p.line_segment(
                [c + egui::vec2(-1.0, 3.0), c + egui::vec2(3.0, 5.0)],
                egui::Stroke::new(2.5, color),
            );
            p.circle_stroke(
                c + egui::vec2(-5.0, -5.0),
                2.2,
                egui::Stroke::new(1.2, color),
            );
            p.line_segment([c + egui::vec2(-3.5, -3.5), c + egui::vec2(5.0, 6.0)], s);
        }
        TimelineTool::SelectLeft | TimelineTool::SelectRight => {
            let towards = if tool == TimelineTool::SelectLeft {
                1.0
            } else {
                -1.0
            };
            let x = c.x - towards * 6.0;
            p.add(egui::Shape::line(
                vec![
                    egui::pos2(x + towards * 4.0, c.y - 7.0),
                    egui::pos2(x, c.y - 7.0),
                    egui::pos2(x, c.y + 7.0),
                    egui::pos2(x + towards * 4.0, c.y + 7.0),
                ],
                egui::Stroke::new(2.0, color),
            ));
            let tip = c + egui::vec2(-towards * 8.0, 0.0);
            p.line_segment([tip, c + egui::vec2(towards * 2.0, 0.0)], s);
            p.add(arrow_head(tip, egui::vec2(-towards, 0.0), 4.0, color));
        }
        TimelineTool::Slip => {
            for (x, towards) in [(c.x - 7.0, 1.0), (c.x + 7.0, -1.0)] {
                p.add(egui::Shape::line(
                    vec![
                        egui::pos2(x + towards * 3.0, c.y - 6.0),
                        egui::pos2(x, c.y - 6.0),
                        egui::pos2(x, c.y + 6.0),
                        egui::pos2(x + towards * 3.0, c.y + 6.0),
                    ],
                    s,
                ));
            }
            p.line_segment([c - egui::vec2(3.0, 0.0), c + egui::vec2(3.0, 0.0)], s);
            for dir in [-1.0f32, 1.0] {
                p.add(arrow_head(
                    c + egui::vec2(dir * 4.5, 0.0),
                    egui::vec2(dir, 0.0),
                    3.0,
                    color,
                ));
            }
        }
    }
}

/// Camera: the track holds picture.
pub(crate) fn paint_camera_icon(p: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let body = egui::Rect::from_center_size(c + egui::vec2(-1.5, 0.0), egui::vec2(10.0, 8.0));
    p.rect_stroke(body, 1.5, stroke(color), egui::StrokeKind::Inside);
    p.add(egui::Shape::convex_polygon(
        vec![
            c + egui::vec2(4.0, -1.0),
            c + egui::vec2(8.0, -4.0),
            c + egui::vec2(8.0, 4.0),
            c + egui::vec2(4.0, 1.0),
        ],
        color,
        egui::Stroke::NONE,
    ));
}

/// Music note: the track holds sound.
pub(crate) fn paint_note_icon(p: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    p.circle_filled(c + egui::vec2(-2.5, 4.5), 2.8, color);
    p.line_segment(
        [c + egui::vec2(0.0, 4.0), c + egui::vec2(0.0, -6.0)],
        stroke(color),
    );
    p.add(egui::Shape::line(
        vec![
            c + egui::vec2(0.0, -6.0),
            c + egui::vec2(5.0, -4.0),
            c + egui::vec2(5.0, -1.0),
        ],
        stroke(color),
    ));
}

pub(crate) fn paint_text_icon(p: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "T",
        egui::FontId::proportional(15.0),
        color,
    );
}

/// A pencil.
pub(crate) fn paint_pencil_icon(p: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let s = stroke(color);
    let tip = c + egui::vec2(-5.5, 5.5);
    p.add(egui::Shape::closed_line(
        vec![
            tip,
            c + egui::vec2(-5.5, 2.5),
            c + egui::vec2(3.0, -6.0),
            c + egui::vec2(6.0, -3.0),
            c + egui::vec2(-2.5, 5.5),
        ],
        s,
    ));
    p.line_segment([c + egui::vec2(0.5, -3.0), c + egui::vec2(3.5, 0.0)], s);
}

/// An eye, crossed out when the layer is hidden.
pub(crate) fn paint_eye_icon(
    p: &egui::Painter,
    rect: egui::Rect,
    visible: bool,
    color: egui::Color32,
) {
    let c = rect.center();
    let s = stroke(color);
    let upper: Vec<egui::Pos2> = (0..=12)
        .map(|i| {
            let x = -7.0 + 14.0 * (i as f32 / 12.0);
            c + egui::vec2(x, -4.5 * (1.0 - (x / 7.0).powi(2)))
        })
        .collect();
    let lower: Vec<egui::Pos2> = upper
        .iter()
        .map(|pt| egui::pos2(pt.x, 2.0 * c.y - pt.y))
        .collect();
    p.add(egui::Shape::line(upper, s));
    p.add(egui::Shape::line(lower, s));
    p.circle_filled(c, 2.0, color);
    if !visible {
        p.line_segment([c + egui::vec2(-6.0, 6.0), c + egui::vec2(6.0, -6.0)], s);
    }
}

/// A speaker, crossed out when muted.
pub(crate) fn paint_speaker_icon(
    p: &egui::Painter,
    rect: egui::Rect,
    audible: bool,
    color: egui::Color32,
) {
    use std::f32::consts::PI;
    let c = rect.center();
    let s = stroke(color);
    p.add(egui::Shape::convex_polygon(
        vec![
            c + egui::vec2(-7.0, -2.5),
            c + egui::vec2(-4.0, -2.5),
            c + egui::vec2(0.0, -6.0),
            c + egui::vec2(0.0, 6.0),
            c + egui::vec2(-4.0, 2.5),
            c + egui::vec2(-7.0, 2.5),
        ],
        color,
        egui::Stroke::NONE,
    ));
    if audible {
        p.add(egui::Shape::line(
            arc(c + egui::vec2(0.0, 0.0), 4.0, -PI * 0.28, PI * 0.28),
            s,
        ));
        p.add(egui::Shape::line(
            arc(c + egui::vec2(0.0, 0.0), 7.0, -PI * 0.28, PI * 0.28),
            s,
        ));
    } else {
        p.line_segment([c + egui::vec2(2.5, -3.0), c + egui::vec2(7.5, 3.0)], s);
        p.line_segment([c + egui::vec2(2.5, 3.0), c + egui::vec2(7.5, -3.0)], s);
    }
}

/// Thick bracket on a clip edge while the pointer is over its trim handle.
pub(crate) fn paint_trim_bracket(p: &egui::Painter, clip_rect: egui::Rect, start: bool) {
    const TICK: f32 = 5.0;
    let towards = if start { 1.0 } else { -1.0 };
    let x = if start {
        clip_rect.left() + 1.0
    } else {
        clip_rect.right() - 1.0
    };
    let (top, bottom) = (clip_rect.top() + 1.0, clip_rect.bottom() - 1.0);
    let points = vec![
        egui::pos2(x + towards * TICK, top),
        egui::pos2(x, top),
        egui::pos2(x, bottom),
        egui::pos2(x + towards * TICK, bottom),
    ];
    p.add(egui::Shape::line(
        points.clone(),
        egui::Stroke::new(6.0, egui::Color32::BLACK),
    ));
    p.add(egui::Shape::line(
        points,
        egui::Stroke::new(4.0, egui::Color32::WHITE),
    ));
}

/// What the timeline shows before the first media: ruler, an empty Main
/// Track and the hint. A media dragged in is returned.
pub(crate) fn show_empty(ui: &mut egui::Ui, header_width: f32) -> Option<TimelineDrag> {
    let rect = ui.available_rect_before_wrap();
    let painter = ui.painter_at(rect);
    let dark = ui.visuals().dark_mode;
    let muted = ui.visuals().widgets.noninteractive.fg_stroke.color;

    let ruler = egui::Rect::from_min_size(
        rect.min + egui::vec2(header_width, 0.0),
        egui::vec2(rect.width() - header_width, 20.0),
    );
    painter.rect_filled(
        ruler,
        0.0,
        if dark {
            egui::Color32::from_gray(38)
        } else {
            egui::Color32::from_gray(225)
        },
    );
    let mut x = ruler.left();
    let mut second = 0;
    while x < ruler.right() {
        painter.line_segment(
            [
                egui::pos2(x, ruler.bottom() - 6.0),
                egui::pos2(x, ruler.bottom()),
            ],
            egui::Stroke::new(1.0, muted),
        );
        painter.text(
            egui::pos2(x + 3.0, ruler.top() + 2.0),
            egui::Align2::LEFT_TOP,
            format!("{second}s"),
            egui::FontId::proportional(crate::theme::FONT_SMALL),
            muted,
        );
        x += 60.0 * 5.0;
        second += 5;
    }

    let track_height = 40.0;
    let free = (rect.height() - ruler.height()).max(track_height);
    let track = egui::Rect::from_min_size(
        egui::pos2(
            rect.left(),
            ruler.bottom() + ((free - track_height) / 2.0).floor(),
        ),
        egui::vec2(rect.width(), track_height),
    );
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(track.left() + header_width, track.top()),
            track.max,
        ),
        0.0,
        if dark {
            egui::Color32::from_gray(32)
        } else {
            egui::Color32::from_gray(238)
        },
    );
    let header = egui::Rect::from_min_size(track.min, egui::vec2(header_width, track_height));
    paint_camera_icon(
        &painter,
        egui::Rect::from_min_size(header.min + egui::vec2(2.0, 11.0), egui::vec2(20.0, 18.0)),
        muted,
    );
    painter.text(
        header.min + egui::vec2(26.0, track_height / 2.0),
        egui::Align2::LEFT_CENTER,
        t!("timeline.main_track"),
        egui::FontId::proportional(crate::theme::FONT_BODY),
        muted,
    );

    let drop_zone = egui::Rect::from_min_max(
        egui::pos2(rect.left() + header_width, ruler.bottom()),
        rect.max,
    );
    let response = ui.interact(
        drop_zone,
        ui.id().with("timeline_drop_zone_empty"),
        egui::Sense::hover(),
    );
    if TimelineDrag::hovered(&response).is_some() {
        let accent = crate::theme::ACCENT;
        painter.rect_filled(track, 0.0, accent.gamma_multiply(0.2));
        painter.rect_stroke(
            track,
            0.0,
            egui::Stroke::new(2.0, accent),
            egui::StrokeKind::Inside,
        );
    }
    painter.text(
        egui::pos2(drop_zone.center().x, track.center().y),
        egui::Align2::CENTER_CENTER,
        t!("timeline.empty_hint"),
        egui::FontId::proportional(crate::theme::FONT_BODY),
        muted.gamma_multiply(0.7),
    );
    ui.allocate_rect(rect, egui::Sense::hover());
    TimelineDrag::released(&response)
}
