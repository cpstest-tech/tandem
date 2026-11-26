//! Trims, rolls, ripple trims and retime handles of the clip edges.

use super::*;

pub(super) fn begin_trim(
    state: &mut TimelineState,
    visuals: &[ClipVisual],
    project: &Project,
    visual: &ClipVisual,
    zone: EdgeZone,
) {
    // A ripple trim moves the neighbour instead of rolling into it.
    let zone = if state.tool == TimelineTool::Ripple {
        EdgeZone::Trim(zone.edge())
    } else {
        zone
    };
    let edge = zone.edge();
    let key = (visual.track_index, visual.clip.id);
    let others: Vec<(ClipKey, TrimEdge)> = match zone {
        EdgeZone::Trim(_) => drag_group_for(&state.selected, visuals, key)
            .into_iter()
            .filter(|k| *k != key)
            .map(|k| (k, edge))
            .collect(),
        // Only the two clips in contact, each with its own linked group.
        EdgeZone::Roll { neighbor, .. } => {
            let opposite = match edge {
                TrimEdge::Start => TrimEdge::End,
                TrimEdge::End => TrimEdge::Start,
            };
            expand_to_linked_groups(visuals, [key])
                .into_iter()
                .filter(|k| *k != key)
                .map(|k| (k, edge))
                .chain(
                    expand_to_linked_groups(visuals, [neighbor])
                        .into_iter()
                        .map(|k| (k, opposite)),
                )
                .collect()
        }
    };
    let ripple_all = state.tool == TimelineTool::Ripple;
    let (min_value, max_value, followers) = combined_trim_range(
        visuals,
        project,
        key,
        edge,
        &others,
        ripple_track(state),
        ripple_all,
    );
    let original_value = match edge {
        TrimEdge::Start => visual.clip.timeline_start,
        TrimEdge::End => visual.clip.timeline_end(),
    };
    state.gesture = Some(Gesture::Trim(TrimState {
        clip_id: visual.clip.id,
        track_index: visual.track_index,
        edge,
        original_value,
        accum_px: 0.0,
        min_value,
        max_value: max_value.max(min_value),
        followers,
        roll: matches!(zone, EdgeZone::Roll { .. }),
    }));
}

/// The moves of a ripple trim: everything after the trimmed edge, on every
/// unlocked track, follows it; a trimmed start keeps the clip where it was
/// and pulls the rest by the same amount.
pub(super) fn ripple_trim_moves(
    tl: &vv_core::Timeline,
    trims: &[(ClipId, usize, TrimEdge, FrameIdx)],
) -> Vec<(ClipId, usize, usize, FrameIdx)> {
    let Some(&(clip_id, track, edge, new_value)) = trims.first() else {
        return Vec::new();
    };
    let Some(clip) = tl.clip(track, clip_id) else {
        return Vec::new();
    };
    let old_end = clip.timeline_end();
    let delta = match edge {
        TrimEdge::End => new_value - old_end,
        TrimEdge::Start => clip.timeline_start - new_value,
    };
    if delta == 0 {
        return Vec::new();
    }
    let trimmed: BTreeSet<ClipId> = trims.iter().map(|t| t.0).collect();
    let mut moves: Vec<(ClipId, usize, usize, FrameIdx)> = tl
        .tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.locked)
        .flat_map(|(i, t)| {
            t.clips
                .iter()
                .filter(|c| c.timeline_start >= old_end && !trimmed.contains(&c.id))
                .map(move |c| (c.id, i, i, (c.timeline_start + delta).max(0)))
        })
        .collect();
    if edge == TrimEdge::Start {
        // `tl` is before the trim: the trimmed clips go back to that start.
        for &(id, t, _, _) in trims {
            if let Some(c) = tl.clip(t, id) {
                moves.push((id, t, t, c.timeline_start));
            }
        }
    }
    moves
}

/// Range of the trimmed edge combined with that of the `others`, and their
/// offsets for `TrimState::followers`.
pub(super) fn combined_trim_range(
    visuals: &[ClipVisual],
    project: &Project,
    primary: ClipKey,
    edge: TrimEdge,
    others: &[(ClipKey, TrimEdge)],
    ripple_track: Option<usize>,
    ripple_all: bool,
) -> (FrameIdx, FrameIdx, Vec<TrimFollower>) {
    let find = |(track, id): ClipKey| {
        visuals
            .iter()
            .find(|v| v.track_index == track && v.clip.id == id)
    };
    let Some(primary_visual) = find(primary) else {
        return (0, FrameIdx::MAX, Vec::new());
    };
    let edge_value = |clip: &Clip, edge: TrimEdge| match edge {
        TrimEdge::Start => clip.timeline_start,
        TrimEdge::End => clip.timeline_end(),
    };
    let primary_value = edge_value(&primary_visual.clip, edge);
    let trimmed: Vec<(&ClipVisual, TrimEdge)> = std::iter::once((primary_visual, edge))
        .chain(others.iter().filter_map(|&(k, e)| find(k).map(|v| (v, e))))
        .collect();

    let mut min_value = FrameIdx::MIN;
    let mut max_value = FrameIdx::MAX;
    let mut followers = Vec::new();
    for &(v, v_edge) in &trimmed {
        let offset = edge_value(&v.clip, v_edge) - primary_value;
        let (mut o_min, mut o_max) = vv_core::edit::trim_range(project, &v.clip, v_edge);
        for n in visuals.iter().filter(|n| {
            n.track_index == v.track_index
                && !ripple_all
                && Some(n.track_index) != ripple_track
                && n.clip.id != v.clip.id
                && !trimmed
                    .iter()
                    .any(|(t, _)| t.track_index == n.track_index && t.clip.id == n.clip.id)
        }) {
            match v_edge {
                TrimEdge::End if n.clip.timeline_start >= v.clip.timeline_end() => {
                    o_max = o_max.min(n.clip.timeline_start);
                }
                TrimEdge::Start if n.clip.timeline_end() <= v.clip.timeline_start => {
                    o_min = o_min.max(n.clip.timeline_end());
                }
                _ => {}
            }
        }
        // Two clips trimmed together on the same track must not
        // lengthen over each other; in a roll, instead, the edge of the
        // neighbor moves with this one.
        for &(w, w_edge) in trimmed.iter().filter(|(w, w_edge)| {
            w.track_index == v.track_index && w.clip.id != v.clip.id && *w_edge == v_edge
        }) {
            match w_edge {
                TrimEdge::End if w.clip.timeline_start >= v.clip.timeline_end() => {
                    o_max = o_max.min(w.clip.timeline_start);
                }
                TrimEdge::Start if w.clip.timeline_end() <= v.clip.timeline_start => {
                    o_min = o_min.max(w.clip.timeline_end());
                }
                _ => {}
            }
        }
        min_value = min_value.max(o_min.saturating_sub(offset));
        max_value = max_value.min(o_max.saturating_sub(offset));
        if v.clip.id != primary_visual.clip.id || v.track_index != primary_visual.track_index {
            followers.push((v.clip.id, v.track_index, offset, v_edge));
        }
    }
    (min_value, max_value, followers)
}

pub(super) fn edge_zones(
    width: f32,
    start_neighbor: Option<ClipKey>,
    end_neighbor: Option<ClipKey>,
) -> EdgeZones {
    EdgeZones {
        width,
        roll_px: ROLL_HANDLE_PX.min(width / 6.0),
        trim_px: TRIM_HANDLE_PX.min(width / 3.0),
        start_neighbor,
        end_neighbor,
    }
}

/// egui has no custom cursors: the system one is hidden and
/// this is drawn in its place. Brackets "[" / "]" like the edges of a
/// clip, with the drag arrows.
pub(super) fn paint_edge_cursor(ctx: &egui::Context, pos: egui::Pos2, cursor: EdgeCursor) {
    ctx.set_cursor_icon(egui::CursorIcon::None);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("timeline_edge_cursor"),
    ));
    if cursor == EdgeCursor::Retime {
        paint_retime_cursor(&painter, pos);
        return;
    }
    const HALF_H: f32 = 8.0;
    const TICK: f32 = 4.0;
    let bracket = |x: f32, towards: f32| {
        vec![
            egui::pos2(x + towards * TICK, pos.y - HALF_H),
            egui::pos2(x, pos.y - HALF_H),
            egui::pos2(x, pos.y + HALF_H),
            egui::pos2(x + towards * TICK, pos.y + HALF_H),
        ]
    };
    // (tip x, direction)
    let arrow = |tip: f32, dir: f32| {
        vec![
            egui::pos2(tip, pos.y),
            egui::pos2(tip - dir * 5.0, pos.y - 4.5),
            egui::pos2(tip - dir * 5.0, pos.y + 4.5),
        ]
    };
    let (brackets, arrows) = match cursor {
        EdgeCursor::TrimEnd => (
            vec![bracket(pos.x, -1.0)],
            vec![arrow(pos.x - 11.0, -1.0), arrow(pos.x + 8.0, 1.0)],
        ),
        EdgeCursor::TrimStart => (
            vec![bracket(pos.x, 1.0)],
            vec![arrow(pos.x - 8.0, -1.0), arrow(pos.x + 11.0, 1.0)],
        ),
        EdgeCursor::Roll => (
            vec![bracket(pos.x - 2.0, -1.0), bracket(pos.x + 2.0, 1.0)],
            vec![arrow(pos.x - 10.0, -1.0), arrow(pos.x + 10.0, 1.0)],
        ),
        // "[ ]" with the arrows inside: the clip stays, its content moves.
        EdgeCursor::Slip => (
            vec![bracket(pos.x - 10.0, 1.0), bracket(pos.x + 10.0, -1.0)],
            vec![arrow(pos.x - 6.0, -1.0), arrow(pos.x + 6.0, 1.0)],
        ),
        EdgeCursor::Retime => unreachable!("drawn by paint_retime_cursor"),
    };
    for points in &brackets {
        painter.line(points.clone(), egui::Stroke::new(4.0, egui::Color32::BLACK));
    }
    for points in brackets {
        painter.line(points, egui::Stroke::new(2.0, egui::Color32::WHITE));
    }
    for points in arrows {
        painter.add(egui::Shape::convex_polygon(
            points,
            egui::Color32::WHITE,
            egui::Stroke::new(1.0, egui::Color32::BLACK),
        ));
    }
}

/// A white double arrow with "%" over it: unlike the brackets of the trim and
/// the black system arrow of the fades.
pub(super) fn paint_retime_cursor(painter: &egui::Painter, pos: egui::Pos2) {
    const HALF_W: f32 = 11.0;
    let outline = egui::Stroke::new(1.0, egui::Color32::BLACK);
    painter.line_segment(
        [
            pos - egui::vec2(HALF_W - 4.0, 0.0),
            pos + egui::vec2(HALF_W - 4.0, 0.0),
        ],
        egui::Stroke::new(5.0, egui::Color32::BLACK),
    );
    painter.line_segment(
        [
            pos - egui::vec2(HALF_W - 4.0, 0.0),
            pos + egui::vec2(HALF_W - 4.0, 0.0),
        ],
        egui::Stroke::new(3.0, egui::Color32::WHITE),
    );
    for dir in [-1.0f32, 1.0] {
        let tip = pos.x + dir * HALF_W;
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(tip, pos.y),
                egui::pos2(tip - dir * 6.0, pos.y - 5.5),
                egui::pos2(tip - dir * 6.0, pos.y + 5.5),
            ],
            egui::Color32::WHITE,
            outline,
        ));
    }
    let label = pos - egui::vec2(0.0, 10.0);
    for offset in [
        egui::vec2(-1.0, 0.0),
        egui::vec2(1.0, 0.0),
        egui::vec2(0.0, -1.0),
        egui::vec2(0.0, 1.0),
    ] {
        painter.text(
            label + offset,
            egui::Align2::CENTER_BOTTOM,
            "%",
            egui::FontId::proportional(11.0),
            egui::Color32::BLACK,
        );
    }
    painter.text(
        label,
        egui::Align2::CENTER_BOTTOM,
        "%",
        egui::FontId::proportional(11.0),
        egui::Color32::WHITE,
    );
}

/// The bar on top of a clip with its speed handles shown: the speed, with
/// the presets menu, and `×` to hide it.
pub(super) fn retime_bar(
    ui: &egui::Ui,
    painter: &egui::Painter,
    clip_rect: egui::Rect,
    id: egui::Id,
    speed: vv_core::Rational,
    interactive: bool,
) -> Option<RetimeBarAction> {
    let height = retime_bar_height(clip_rect);
    let bar = egui::Rect::from_min_size(clip_rect.min, egui::vec2(clip_rect.width(), height));
    painter.rect_filled(bar, 2.0, crate::theme::ACCENT_FILL);
    let font = egui::FontId::proportional(11.0);
    let galley = painter.layout_no_wrap(
        format!("{} ⏷", format_speed(speed)),
        font.clone(),
        egui::Color32::WHITE,
    );
    // Centered on the visible part of the clip, however long or zoomed in.
    let visible = bar.intersect(painter.clip_rect());
    if visible.width() <= 0.0 {
        return None;
    }
    let menu_width = (galley.size().x + 8.0).min(visible.width());
    let menu_left = (visible.center().x - menu_width / 2.0).max(visible.left());
    let menu_rect = egui::Rect::from_min_size(
        egui::pos2(menu_left, bar.top()),
        egui::vec2(menu_width, height),
    );
    let close_rect = egui::Rect::from_min_size(
        egui::pos2(visible.right() - height, bar.top()),
        egui::vec2(height, height),
    );
    painter.with_clip_rect(visible).galley(
        menu_rect.min + egui::vec2(4.0, (height - galley.size().y) / 2.0),
        galley,
        egui::Color32::WHITE,
    );
    let show_close = visible.width() >= menu_width + 2.0 * close_rect.width();
    if show_close {
        painter.text(
            close_rect.center(),
            egui::Align2::CENTER_CENTER,
            "×",
            font,
            egui::Color32::WHITE,
        );
    }
    if !interactive {
        return None;
    }
    let mut action = None;
    if show_close
        && ui
            .interact(close_rect, id.with("retime_close"), egui::Sense::click())
            .clicked()
    {
        action = Some(RetimeBarAction::Close);
    }
    let menu = ui.interact(menu_rect, id.with("retime_menu"), egui::Sense::click());
    egui::Popup::menu(&menu).show(|ui| {
        for percent in SPEED_PRESETS {
            if ui.button(format!("{percent:.0}%")).clicked() {
                action = Some(RetimeBarAction::Percent(percent));
            }
        }
        ui.separator();
        if ui.button(t!("timeline.change_clip_speed")).clicked() {
            action = Some(RetimeBarAction::Dialog);
        }
    });
    action
}
