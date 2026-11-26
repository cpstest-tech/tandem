//! Ruler, ticks and the scroll that keeps the timeline under it.

use super::*;

/// Ticks on three levels: major ones with timecode, medium ones every N frames,
/// one per frame; a level denser than `MIN_TICK_SPACING_PX` is not
/// drawn.
pub(super) fn draw_ruler_ticks(
    painter: &egui::Painter,
    origin: egui::Pos2,
    visible_x: egui::Rect,
    pixels_per_sec: f32,
    fps: f64,
) {
    if !visible_x.is_positive() {
        return; // ruler completely outside the scrolled viewport
    }

    const MIN_TICK_SPACING_PX: f32 = 8.0;
    let tick_color = egui::Color32::from_gray(110);
    let label_color = egui::Color32::from_gray(200);
    let minor_color = egui::Color32::from_gray(70);
    let medium_color = egui::Color32::from_gray(90);

    // Heights of the three tick levels (from bottom to top).
    const FRAME_TICK_HEIGHT: f32 = 5.0;
    const MEDIUM_TICK_HEIGHT: f32 = 10.0;
    const MAJOR_TICK_HEIGHT: f32 = RULER_TICKS_HEIGHT;

    // Range in seconds actually visible, not the whole duration of the
    // timeline (thousands of off-screen ticks otherwise).
    let visible_start_secs = ((visible_x.min.x - origin.x) / pixels_per_sec.max(1e-6)) as f64;
    let visible_end_secs = ((visible_x.max.x - origin.x) / pixels_per_sec.max(1e-6)) as f64;

    // Level 2: major ticks with an HH:MM:SS:FF label, adaptive
    // "clean" interval (1-2-5 sequence) — always visible.
    let major_secs = nice_tick_interval_secs(pixels_per_sec);
    let first_major = (visible_start_secs / major_secs).floor() as i64;
    let last_major = (visible_end_secs / major_secs).ceil() as i64;
    for i in first_major..=last_major {
        let secs = i as f64 * major_secs;
        if secs < 0.0 {
            continue;
        }
        let x = origin.x + (secs * pixels_per_sec as f64) as f32;
        painter.line_segment(
            [
                egui::pos2(x, origin.y),
                egui::pos2(x, origin.y + MAJOR_TICK_HEIGHT),
            ],
            egui::Stroke::new(1.0, tick_color),
        );
        painter.text(
            egui::pos2(x + 3.0, origin.y + 2.0),
            egui::Align2::LEFT_TOP,
            format_timecode(secs, fps),
            egui::FontId::proportional(10.0),
            label_color,
        );
    }

    // Medium ticks, only if denser than the major ones.
    let px_per_frame = pixels_per_sec / fps.max(1e-9) as f32;

    // Computes the interval in frames for the medium ticks: the smallest
    // "clean" multiple (1, 2, 5, 10, 25, 50...) that keeps the ticks at
    // least MIN_TICK_SPACING_PX apart.
    let medium_interval_frames = {
        const MEDIUM_CANDIDATES: &[i64] = &[1, 2, 5, 10, 25, 50, 100, 250, 500];
        MEDIUM_CANDIDATES
            .iter()
            .copied()
            .find(|&c| c as f32 * px_per_frame >= MIN_TICK_SPACING_PX)
            .unwrap_or(*MEDIUM_CANDIDATES.last().unwrap())
    };

    // The medium ticks are useful only if they are closer than the major ones and
    // do not overlap them exactly (otherwise they would be redundant).
    let major_interval_frames = (major_secs * fps) as i64;
    if medium_interval_frames < major_interval_frames {
        let first_frame = (visible_start_secs * fps).floor().max(0.0) as i64;
        let last_frame = (visible_end_secs * fps).ceil().max(0.0) as i64;
        for frame in (first_frame..=last_frame).step_by(medium_interval_frames as usize) {
            // Skips the positions where there is already a major tick (redundant).
            let secs = frame as f64 / fps;
            let major_at_this_pos = ((secs / major_secs).round() * major_secs - secs).abs() < 1e-9;
            if major_at_this_pos {
                continue;
            }
            let x = origin.x + frame as f32 * px_per_frame;
            painter.line_segment(
                [
                    egui::pos2(x, origin.y + RULER_TICKS_HEIGHT - MEDIUM_TICK_HEIGHT),
                    egui::pos2(x, origin.y + RULER_TICKS_HEIGHT),
                ],
                egui::Stroke::new(1.0, medium_color),
            );
        }
    }

    // Level 0: ticks for every single frame — the shortest ones, visible only
    // when the zoom is high enough not to make them touch.
    if px_per_frame >= MIN_TICK_SPACING_PX {
        let first_frame = (visible_start_secs * fps).floor().max(0.0) as i64;
        let last_frame = (visible_end_secs * fps).ceil().max(0.0) as i64;
        for frame in first_frame..=last_frame {
            // Skips the positions where there is already a medium or major tick.
            let is_medium_pos = frame % medium_interval_frames == 0;
            let secs = frame as f64 / fps;
            let is_major_pos = ((secs / major_secs).round() * major_secs - secs).abs() < 1e-9;
            if is_medium_pos || is_major_pos {
                continue;
            }
            let x = origin.x + frame as f32 * px_per_frame;
            painter.line_segment(
                [
                    egui::pos2(x, origin.y + RULER_TICKS_HEIGHT - FRAME_TICK_HEIGHT),
                    egui::pos2(x, origin.y + RULER_TICKS_HEIGHT),
                ],
                egui::Stroke::new(1.0, minor_color),
            );
        }
    }
}

/// Ruler: ticks, cached frames strip and export markers; click and
/// drag move the playhead.
pub(super) fn show_ruler(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    origin: egui::Pos2,
    content_width: f32,
    state: &mut TimelineState,
    visuals: &[ClipVisual],
    fps: f64,
    px_per_frame: f32,
    snapping_enabled: bool,
    buffered_ranges: &[(FrameIdx, FrameIdx)],
    keyframe_ranges: &[(FrameIdx, FrameIdx)],
    max_end_frames: FrameIdx,
    pending: &mut Option<PendingAction>,
) {
    let ruler_rect = egui::Rect::from_min_size(origin, egui::vec2(content_width, RULER_HEIGHT));
    painter.rect_filled(ruler_rect, 0.0, crate::theme::BG_PANEL);
    painter.rect_filled(
        markers::lane_rect(origin, content_width),
        0.0,
        crate::theme::BG_INSET,
    );
    let ruler_resp = ui.interact(
        ruler_rect,
        ui.id().with("timeline_ruler"),
        egui::Sense::click_and_drag(),
    );
    ruler_resp.context_menu(|ui| {
        if ui.button(t!("timeline.add_marker")).clicked() {
            *pending = Some(PendingAction::Marker(markers::MarkerChange::AddAtPlayhead));
            ui.close();
        }
    });
    state.scrubbing = ruler_resp.is_pointer_button_down_on();
    // On a click what counts is where it was released: if the frame
    // arrived late, `interact_pointer_pos` is already
    // the last mouse position after the release.
    let ruler_pos = if ruler_resp.clicked() {
        ui.input(|i| {
            i.events.iter().rev().find_map(|e| match e {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                } => Some(*pos),
                _ => None,
            })
        })
        .or_else(|| ruler_resp.interact_pointer_pos())
    } else {
        ruler_resp.interact_pointer_pos()
    };
    if let Some(pos) = ruler_pos {
        let raw_frame = (((pos.x - origin.x) / px_per_frame).round() as FrameIdx).max(0);
        state.playhead = snap_frame(
            raw_frame,
            0,
            visuals,
            &[],
            &[],
            px_per_frame,
            snapping_enabled,
        );
    }

    // Only the visible ticks: a long timeline zoomed to the frame would have
    // thousands of them off screen.
    let visible_x = ui.clip_rect().intersect(ruler_rect);
    draw_ruler_ticks(painter, origin, visible_x, state.pixels_per_sec, fps);

    // Below the playhead line, so it stays visible.
    const BUFFERED_STRIP_HEIGHT: f32 = 4.0;
    let buffered_color = egui::Color32::from_rgba_unmultiplied(120, 190, 255, 140);
    // Orange as the KEYFRAMES badge, under the frames decoded one by one.
    let keyframe_color = crate::theme::ACCENT.gamma_multiply(0.75);
    let strips = keyframe_ranges
        .iter()
        .map(|r| (r, keyframe_color))
        .chain(buffered_ranges.iter().map(|r| (r, buffered_color)));
    for (&(start, end), color) in strips {
        let x0 = origin.x + start as f32 * px_per_frame;
        let x1 = origin.x + (end + 1) as f32 * px_per_frame;
        let strip_rect = egui::Rect::from_min_max(
            egui::pos2(x0, origin.y + RULER_TICKS_HEIGHT - BUFFERED_STRIP_HEIGHT),
            egui::pos2(x1, origin.y + RULER_TICKS_HEIGHT),
        );
        painter.rect_filled(strip_rect, 0.0, color);
    }

    if !state.export_marks.is_full(max_end_frames) {
        let (mark_in, mark_out) = state.export_marks.resolve(max_end_frames);
        let band = egui::Rect::from_min_max(
            egui::pos2(origin.x + mark_in as f32 * px_per_frame, origin.y),
            egui::pos2(
                origin.x + mark_out as f32 * px_per_frame,
                origin.y + RULER_TICKS_HEIGHT - BUFFERED_STRIP_HEIGHT,
            ),
        );
        painter.rect_filled(
            band,
            0.0,
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 28),
        );
        for x in [band.left(), band.right()] {
            painter.vline(
                x,
                band.y_range(),
                egui::Stroke::new(1.0, egui::Color32::from_gray(185)),
            );
        }
    }
}

/// Corrects the saved scroll of the ScrollArea `scroll_id` before its `show`:
/// on zoom the playhead stays still on screen, during playback it stays visible;
/// it also handles the horizontal kinetic touchpad scroll (swipe + inertia
/// after release), since the vertical wheel scroll is already consumed
/// elsewhere for the Video/Audio boxes.
pub(super) fn sync_timeline_scroll(
    ctx: &egui::Context,
    scroll_id: egui::Id,
    state: &mut TimelineState,
    fps: f64,
    px_per_frame: f32,
    viewport_width: f32,
    content_width: f32,
    playback_active: bool,
    panel_rect: egui::Rect,
    kinetic_scroll_enabled: bool,
) {
    // Zoom changed in this frame: the saved scroll of the
    // ScrollArea (same id) is corrected before the `show`, so the playhead stays still on
    // screen.
    if state.pixels_per_sec != state.last_rendered_pps
        && let Some(mut scroll_state) = egui::containers::scroll_area::State::load(ctx, scroll_id)
    {
        let playhead_secs = state.playhead as f64 / fps;
        scroll_state.offset.x +=
            (playhead_secs as f32) * (state.pixels_per_sec - state.last_rendered_pps);
        scroll_state.store(ctx, scroll_id);
    }
    state.last_rendered_pps = state.pixels_per_sec;

    // During playback (or after a programmatic jump) the playhead stays visible:
    // if it leaves, the view "turns the page" bringing it to a third from the
    // left. Clamp like egui's in `begin`.
    let reveal = std::mem::take(&mut state.reveal_playhead);
    if playback_active || reveal {
        let playhead_x = state.playhead as f32 * px_per_frame;
        let visible_start = match egui::containers::scroll_area::State::load(ctx, scroll_id) {
            Some(st) => st.offset.x,
            None => 0.0,
        };
        let visible_end = visible_start + viewport_width;
        const FOLLOW_MARGIN_FRAC: f32 = 1.0 / 3.0;
        if playhead_x < visible_start || playhead_x > visible_end {
            let target = (playhead_x - viewport_width * FOLLOW_MARGIN_FRAC)
                .clamp(0.0, (content_width - viewport_width).max(0.0));
            if let Some(mut scroll_state) =
                egui::containers::scroll_area::State::load(ctx, scroll_id)
            {
                scroll_state.offset.x = target;
                scroll_state.store(ctx, scroll_id);
            } else {
                // The ScrollArea has no state before its first `show` (a
                // project opened at startup): try again next frame.
                state.reveal_playhead = reveal;
            }
        }
    }

    let max_offset_x = (content_width - viewport_width).max(0.0);
    let dt = ctx.input(|i| i.stable_dt).min(0.1);
    // During playback the view follows the playhead: a residual inertia would
    // make it slide away from the point it just centered it on above.
    if !kinetic_scroll_enabled || playback_active {
        state.hscroll_vel = 0.0;
    }
    let hovering_panel = pointer_over(ctx, panel_rect);
    // Horizontal swipe in progress: applied immediately (as the
    // ScrollArea would), and its instantaneous speed becomes the inertia to
    // damp when the gesture ends.
    let wheel_x = if hovering_panel {
        ctx.input(|i| i.smooth_scroll_delta.x)
    } else {
        0.0
    };
    if wheel_x != 0.0 {
        if let Some(mut scroll_state) = egui::containers::scroll_area::State::load(ctx, scroll_id) {
            scroll_state.offset.x = (scroll_state.offset.x - wheel_x).clamp(0.0, max_offset_x);
            scroll_state.store(ctx, scroll_id);
        }
        state.hscroll_vel = if kinetic_scroll_enabled && !playback_active && dt > 0.0 {
            -KINETIC_VELOCITY_GAIN * wheel_x / dt
        } else {
            0.0
        };
        // Consumed here: the ScrollArea must not reapply it in its `show`.
        ctx.input_mut(|i| i.smooth_scroll_delta.x = 0.0);
    } else if let Some(mut scroll_state) =
        egui::containers::scroll_area::State::load(ctx, scroll_id)
    {
        let mut offset_x = scroll_state.offset.x;
        if apply_kinetic_scroll(&mut offset_x, &mut state.hscroll_vel, max_offset_x, dt) {
            scroll_state.offset.x = offset_x;
            scroll_state.store(ctx, scroll_id);
            ctx.request_repaint();
        }
    }
}

/// Scrolls the view while a drag holds the pointer near its left or right
/// edge; the gesture is shifted by the same amount so it stays under the
/// pointer.
pub(super) fn edge_autoscroll(
    ctx: &egui::Context,
    scroll_id: egui::Id,
    state: &mut TimelineState,
    viewport_x: egui::Rangef,
    panel_y: egui::Rangef,
    content_width: f32,
    dt: f32,
) {
    let gesture = matches!(
        state.gesture,
        Some(Gesture::Move(_) | Gesture::Trim(_) | Gesture::Retime(_) | Gesture::Marquee(_))
    );
    let media = egui::DragAndDrop::has_payload_of_type::<MediaDragSet>(ctx)
        || egui::DragAndDrop::has_payload_of_type::<Generator>(ctx);
    let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    if dt <= 0.0 || !(gesture || (media && panel_y.contains(pos.y))) {
        return;
    }
    let depth = if pos.x < viewport_x.min + EDGE_SCROLL_ZONE {
        pos.x - (viewport_x.min + EDGE_SCROLL_ZONE)
    } else if pos.x > viewport_x.max - EDGE_SCROLL_ZONE {
        pos.x - (viewport_x.max - EDGE_SCROLL_ZONE)
    } else {
        return;
    };
    let frac = (depth / EDGE_SCROLL_ZONE).clamp(-1.0, 1.0);
    // Quadratic: slow near the inner side of the strip, fast at the edge.
    let speed = EDGE_SCROLL_MAX_SPEED * frac * frac.abs();
    let Some(mut scroll) = egui::containers::scroll_area::State::load(ctx, scroll_id) else {
        return;
    };
    let max_offset = (content_width - viewport_x.span()).max(0.0);
    let new_offset = (scroll.offset.x + speed * dt).clamp(0.0, max_offset);
    let applied = new_offset - scroll.offset.x;
    if applied == 0.0 {
        return;
    }
    scroll.offset.x = new_offset;
    scroll.store(ctx, scroll_id);
    state.hscroll_vel = 0.0;
    match &mut state.gesture {
        Some(Gesture::Move(d)) => d.accum_px += applied,
        Some(Gesture::Trim(t)) => t.accum_px += applied,
        Some(Gesture::Retime(r)) => r.accum_px += applied,
        _ => {}
    }
    ctx.request_repaint();
}
