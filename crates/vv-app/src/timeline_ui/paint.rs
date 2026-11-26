//! Drawing of the clips: overlays, waveforms, filmstrips, markers.

use super::*;

/// Label, "disabled" and warning badges, link icon and veil of the locked
/// tracks, on top of the waveform. Returns the warning badge's rect.
pub(super) fn paint_clip_overlay(
    painter: &egui::Painter,
    clip_rect: egui::Rect,
    visual: &ClipVisual,
    is_proxy_backed: bool,
    top_inset: f32,
) -> Option<egui::Rect> {
    let veil_rect = clip_rect;
    let clip_rect = clip_rect.with_min_y(clip_rect.top() + top_inset);
    let label_offset_y = if is_proxy_backed {
        paint_proxy_strip(painter, clip_rect);
        2.0 + PROXY_STRIP_HEIGHT
    } else {
        2.0
    };
    let mut label_pos = clip_rect.left_top() + egui::vec2(4.0, label_offset_y);
    if visual.clip.disabled {
        paint_disabled_badge(painter, label_pos);
        label_pos.x += DISABLED_BADGE_SIZE + 4.0;
    }
    let warning_badge = visual.warning.is_some().then(|| {
        let rect = paint_warning_badge(painter, label_pos);
        label_pos.x += DISABLED_BADGE_SIZE + 4.0;
        rect
    });
    let label_color = if visual.muted {
        egui::Color32::from_gray(185)
    } else {
        egui::Color32::BLACK
    };
    let show_link_icon = visual.clip.linked_group.is_some() && clip_rect.width() > LINK_ICON_WIDTH;
    let reserved_right = if show_link_icon { LINK_ICON_WIDTH } else { 4.0 };
    paint_clip_label(
        painter,
        &visual.label,
        label_pos,
        clip_rect.right() - reserved_right - label_pos.x,
        label_color,
    );
    if show_link_icon {
        // Two hand-drawn rings: on some platforms (Asahi) egui's fonts do not
        // have 🔗.
        let center = clip_rect.right_top() + egui::vec2(-9.0, 8.0);
        let ring_color = if visual.muted {
            egui::Color32::from_gray(185)
        } else {
            egui::Color32::BLACK
        };
        let ring_stroke = egui::Stroke::new(1.3, ring_color);
        painter.circle_stroke(center + egui::vec2(-2.5, 0.0), 3.5, ring_stroke);
        painter.circle_stroke(center + egui::vec2(2.5, 0.0), 3.5, ring_stroke);
    }
    paint_keyframe_diamonds(painter, clip_rect, &visual.clip);
    if visual.locked {
        painter.rect_filled(
            veil_rect,
            4.0,
            egui::Color32::from_rgba_unmultiplied(70, 70, 70, 140),
        );
    }
    warning_badge
}

/// The marker of a transition: a colored band at the bottom of the clip from the
/// edge to `x`. A single edge (`is_crossing: false`) shows a
/// dot on the fixed side (the real edge of the clip, against
/// transparency) and a bracket on the end `x` (the draggable side) —
/// "X]" for `In`, "[X" for `Out`. A crossing (`is_crossing: true`) has no
/// "fixed" side: the edge shared with the neighbor shows another
/// bracket, opening towards the inside of its own half — the two halves,
/// drawn one per clip, sit side by side there as "][".
pub(super) fn paint_transition_marker(
    painter: &egui::Painter,
    clip_rect: egui::Rect,
    edge: FadeEdge,
    x: f32,
    selected: bool,
    is_crossing: bool,
) {
    let x = x.clamp(clip_rect.left(), clip_rect.right());
    let band = egui::Rect::from_min_max(
        egui::pos2(
            clip_rect.left(),
            clip_rect.bottom() - TRANSITION_HANDLE_ZONE_HEIGHT,
        ),
        clip_rect.max,
    );
    let body = match edge {
        FadeEdge::In => egui::Rect::from_min_max(band.min, egui::pos2(x, band.bottom())),
        FadeEdge::Out => egui::Rect::from_min_max(egui::pos2(x, band.top()), band.max),
    };
    let color = match (is_crossing, selected) {
        (false, false) => TRANSITION_COLOR,
        (false, true) => TRANSITION_SELECTED_COLOR,
        (true, false) => CROSSING_COLOR,
        (true, true) => CROSSING_SELECTED_COLOR,
    };
    painter.rect_filled(body, 0.0, color);
    let edge_x = match edge {
        FadeEdge::In => clip_rect.left(),
        FadeEdge::Out => clip_rect.right(),
    };
    if is_crossing {
        let opposite = match edge {
            FadeEdge::In => FadeEdge::Out,
            FadeEdge::Out => FadeEdge::In,
        };
        paint_bracket_icon(
            painter,
            egui::pos2(edge_x, band.center().y),
            band.height() * 0.7,
            opposite,
            egui::Color32::WHITE,
        );
    } else {
        painter.circle_filled(
            egui::pos2(edge_x, band.center().y),
            band.height() * 0.22,
            egui::Color32::WHITE,
        );
    }
    paint_bracket_icon(
        painter,
        egui::pos2(x, band.center().y),
        band.height() * 0.7,
        edge,
        egui::Color32::WHITE,
    );
}

/// Waveform of an audio clip, one line per visible column. The bin of
/// each column comes from the absolute time in the audio: splitting the clip does not
/// move the waveform.
pub(crate) fn draw_clip_waveform(
    painter: &egui::Painter,
    clip_rect: egui::Rect,
    peaks: &[f32],
    clip_start_secs: f64,
    clip_end_secs: f64,
    media_fps: f64,
    audio_duration_secs: f64,
    visible_rect: egui::Rect,
    gain_db: &Keyframed<f32>,
) {
    if peaks.is_empty() || audio_duration_secs <= 0.0 || media_fps <= 0.0 {
        return;
    }
    if clip_end_secs <= clip_start_secs {
        return;
    }

    // Visible portion of the clip (no column outside the viewport).
    let vis = clip_rect.intersect(visible_rect);
    if !vis.is_positive() {
        return;
    }

    let center_y = clip_rect.center().y;
    let half_height = clip_rect.height() / 2.0;
    let stroke = egui::Stroke::new(
        1.0,
        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 140),
    );
    let clipped_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 90, 90, 200));

    let width = clip_rect.width();
    let constant_gain = gain_db
        .is_constant()
        .then(|| vv_audio::mixer::db_to_linear(gain_db.default));
    let mut x = vis.min.x;
    while x < vis.max.x {
        let frac = ((x - clip_rect.min.x) / width) as f64;
        let bin = waveform_bin_for_column(
            frac,
            clip_start_secs,
            clip_end_secs,
            audio_duration_secs,
            peaks.len(),
        );
        // The gain lives in *source* frames, as in the mixer: the shape
        // drawn is the one that will really be heard, clipping included.
        let gain = constant_gain.unwrap_or_else(|| {
            let secs = clip_start_secs + frac * (clip_end_secs - clip_start_secs);
            let source_frame = (secs * media_fps).floor() as FrameIdx;
            vv_audio::mixer::db_to_linear(gain_db.value_at(source_frame))
        });
        let amplified = peaks[bin] * gain;
        let h = (half_height * amplified.min(1.0)).max(0.5);
        painter.line_segment(
            [egui::pos2(x, center_y - h), egui::pos2(x, center_y + h)],
            if amplified > 1.0 {
                clipped_stroke
            } else {
                stroke
            },
        );
        x += 1.0;
    }
}

/// `offline`: the media of the clip is no longer in the media pool (deleted from
/// there, see `vv_core::RemoveMedia`) — the clip stays on the timeline but turns
/// red, and the player shows "Media offline".
pub(super) fn clip_label_and_color(
    clip: &Clip,
    track: &Track,
    offline: bool,
    media_labels: &dyn Fn(vv_core::MediaId) -> String,
) -> (String, egui::Color32) {
    match &clip.source {
        vv_core::ClipSource::Media(media_id) => {
            if offline {
                return (t!("timeline.media_offline").into_owned(), OFFLINE_COLOR);
            }
            let mut label = media_labels(*media_id);
            if clip.freeze.is_some() {
                label = format!("{label} ({})", t!("timeline.freeze_frame"));
            } else if clip.active_curve().is_some() {
                label = format!("{label} ({})", t!("props.speed_curve"));
            } else if !clip.speed().is_one() {
                label = format!("{label} ({})", format_speed(clip.speed()));
            }
            if clip.reversed {
                label = format!("{label} ({})", t!("timeline.reverse"));
            }
            let color = if track.kind == TrackKind::Video {
                VIDEO_CLIP_COLOR
            } else {
                AUDIO_CLIP_COLOR
            };
            (label, clip_box_color(color, clip))
        }
        vv_core::ClipSource::SolidColor => (
            t!("generator.solid_color").into_owned(),
            clip_box_color(VIDEO_CLIP_COLOR, clip),
        ),
        vv_core::ClipSource::Text => (
            clip.effects
                .title
                .as_ref()
                .and_then(|t| {
                    let text = t.content.value_at(clip.source_in());
                    text.lines().next().map(str::to_string)
                })
                .unwrap_or_else(|| t!("generator.text").into_owned()),
            clip_box_color(TEXT_CLIP_COLOR, clip),
        ),
        vv_core::ClipSource::Adjustment => (
            clip.effects
                .assets
                .first()
                .and_then(|fx| {
                    vv_render::assets::library()
                        .get(&fx.id)
                        .map(|a| a.name.get(&rust_i18n::locale()).to_owned())
                })
                .unwrap_or_else(|| t!("generator.adjustment").into_owned()),
            clip_box_color(EFFECT_CLIP_COLOR, clip),
        ),
    }
}

/// Tiles the pool thumbnail across a video clip, under a solid strip that
/// keeps the label readable.
pub(super) fn paint_filmstrip(
    painter: &egui::Painter,
    clip_rect: egui::Rect,
    texture: egui::TextureId,
    size: egui::Vec2,
    base: egui::Color32,
) {
    let height = clip_rect.height();
    let tile_width = (size.x / size.y.max(1.0) * height).max(8.0);
    let visible = painter.clip_rect().intersect(clip_rect);
    if visible.width() <= 0.0 {
        return;
    }
    let first = ((visible.left() - clip_rect.left()) / tile_width)
        .floor()
        .max(0.0) as usize;
    let tint = egui::Color32::from_white_alpha(170);
    let mut x = clip_rect.left() + first as f32 * tile_width;
    while x < visible.right() {
        let right = (x + tile_width).min(clip_rect.right());
        let uv_right = (right - x) / tile_width;
        let tile = egui::Rect::from_min_max(
            egui::pos2(x, clip_rect.top()),
            egui::pos2(right, clip_rect.bottom()),
        );
        painter.image(
            texture,
            tile,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(uv_right, 1.0)),
            tint,
        );
        x += tile_width;
    }
    let strip = egui::Rect::from_min_size(
        clip_rect.min,
        egui::vec2(clip_rect.width(), LABEL_FONT_SIZE + 6.0),
    );
    painter.rect_filled(strip, 0.0, base.gamma_multiply(0.85));
    painter.rect_stroke(
        clip_rect,
        clip_corner_radius(clip_rect),
        egui::Stroke::new(1.0, egui::Color32::from_gray(15)),
        egui::StrokeKind::Inside,
    );
}

pub(super) fn paint_ai_placeholder(
    painter: &egui::Painter,
    pending: &AiPlaceholder,
    rect: egui::Rect,
    time: f32,
) {
    let accent = if pending.failed {
        crate::theme::ERROR
    } else if pending.done {
        crate::theme::SUCCESS
    } else {
        crate::theme::ACCENT
    };
    painter.rect_filled(rect, 4.0, accent.gamma_multiply(0.18));
    let stripe = 18.0;
    let shift = if pending.failed || pending.done {
        0.0
    } else {
        (time * 24.0) % (stripe * 2.0)
    };
    let clipped = painter.with_clip_rect(painter.clip_rect().intersect(rect));
    let mut x = rect.left() - rect.height() - stripe * 2.0 + shift;
    while x < rect.right() {
        clipped.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(x, rect.bottom()),
                egui::pos2(x + stripe, rect.bottom()),
                egui::pos2(x + stripe + rect.height(), rect.top()),
                egui::pos2(x + rect.height(), rect.top()),
            ],
            accent.gamma_multiply(0.14),
            egui::Stroke::NONE,
        ));
        x += stripe * 2.0;
    }
    painter.rect_stroke(
        rect,
        4.0,
        egui::Stroke::new(1.5, accent),
        egui::StrokeKind::Inside,
    );
    let text = format!("{} · {}", pending.label, pending.detail);
    clipped.text(
        rect.left_center() + egui::vec2(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(12.0),
        egui::Color32::WHITE,
    );
}

/// Hand-drawn bracket (no Unicode glyph, see `paint_fx_icon`):
/// a vertical line with two notches opening towards the fixed edge of the
/// clip (`In`: notches on the left, towards the dot; `Out`: on the right).
pub(crate) fn paint_bracket_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    height: f32,
    edge: FadeEdge,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(1.6, color);
    let half = height / 2.0;
    let tick = height * 0.3;
    let tick_dir = match edge {
        FadeEdge::In => -1.0,
        FadeEdge::Out => 1.0,
    };
    painter.line_segment(
        [
            egui::pos2(center.x, center.y - half),
            egui::pos2(center.x, center.y + half),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x, center.y - half),
            egui::pos2(center.x + tick * tick_dir, center.y - half),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x, center.y + half),
            egui::pos2(center.x + tick * tick_dir, center.y + half),
        ],
        stroke,
    );
}
