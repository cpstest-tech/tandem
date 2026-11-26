//! Track headers: names, toggles and the drag that reorders tracks.

use super::*;

/// Fixed column of the headers. Drawn by hand: an `add_space`
/// tied to the height would make the panel grow indefinitely.
pub(super) fn draw_track_headers(
    ui: &mut egui::Ui,
    track_kinds: &[TrackKind],
    track_labels: &[String],
    track_flags: &[TrackFlags],
    row_order: &[usize],
    row_y: &[f32],
    layout: &PaneLayout,
    video_pane_height: &mut Option<f32>,
    pending: &mut Option<PendingAction>,
    playhead: FrameIdx,
    fps: f64,
    can_record: bool,
    main_track: Option<usize>,
    text_tracks: &[bool],
    effects_tracks: &[bool],
) {
    let (rect, _resp) = ui.allocate_exact_size(
        egui::vec2(TRACK_HEADER_WIDTH, RULER_HEIGHT),
        egui::Sense::hover(),
    );
    let origin = rect.min;
    let full_clip = ui.clip_rect();
    let text_color = ui.visuals().text_color();

    // Timestamp of the playhead position in HH:MM:SS:FF format, in the
    // ruler row.
    let playhead_secs = playhead as f64 / fps;
    ui.painter().text(
        egui::pos2(
            origin.x + TRACK_HEADER_WIDTH / 2.0,
            origin.y + RULER_HEIGHT / 2.0,
        ),
        egui::Align2::CENTER_CENTER,
        format_timecode(playhead_secs, fps),
        egui::FontId::monospace(14.0),
        egui::Color32::WHITE,
    );

    for &track_index in row_order {
        let kind = track_kinds[track_index];
        let pane = layout.pane(kind);
        ui.set_clip_rect(full_clip.intersect(egui::Rect::from_x_y_ranges(
            rect.x_range(),
            (origin.y + pane.min)..=(origin.y + pane.max),
        )));
        let row_rect = egui::Rect::from_min_size(
            egui::pos2(origin.x, origin.y + row_y[track_index]),
            egui::vec2(TRACK_HEADER_WIDTH, layout.row_height),
        );
        let flags = track_flags[track_index];
        let is_main = main_track == Some(track_index);

        // Behind the buttons, which win the hit-test: the right-click menu
        // of the whole header.
        let header_resp = ui.interact(
            row_rect,
            ui.id().with("header_row").with(track_index),
            egui::Sense::click(),
        );
        header_resp.context_menu(|ui| {
            if kind == TrackKind::Audio {
                if ui
                    .checkbox(&mut { flags.solo }, t!("timeline.solo"))
                    .clicked()
                {
                    *pending = Some(PendingAction::SetTrackFlag(
                        track_index,
                        TrackFlag::Solo,
                        !flags.solo,
                    ));
                    ui.close();
                }
                if ui
                    .checkbox(&mut { flags.armed }, t!("timeline.arm"))
                    .clicked()
                {
                    *pending = Some(if flags.armed || can_record {
                        PendingAction::SetTrackFlag(track_index, TrackFlag::Armed, !flags.armed)
                    } else {
                        PendingAction::ArmNeedsSave
                    });
                    ui.close();
                }
                ui.separator();
            }
            let is_last_of_kind = track_kinds.iter().filter(|k| **k == kind).count() <= 1;
            let removable = !is_last_of_kind && !is_main;
            if ui
                .add_enabled(removable, egui::Button::new(t!("timeline.remove_track")))
                .on_disabled_hover_text(t!("timeline.cannot_remove_last_track"))
                .clicked()
            {
                *pending = Some(PendingAction::RemoveTrack(track_index));
                ui.close();
            }
        });

        let icon_rect = egui::Rect::from_min_size(
            row_rect.left_top() + egui::vec2(4.0, 3.0),
            egui::vec2(20.0, 16.0),
        );
        match kind {
            TrackKind::Video if effects_tracks[track_index] => {
                paint_fx_icon(ui.painter(), icon_rect.center(), 14.0, text_color)
            }
            TrackKind::Video if text_tracks[track_index] => {
                crate::timeline_bar::paint_text_icon(ui.painter(), icon_rect, text_color)
            }
            TrackKind::Video => {
                crate::timeline_bar::paint_camera_icon(ui.painter(), icon_rect, text_color)
            }
            TrackKind::Audio => {
                crate::timeline_bar::paint_note_icon(ui.painter(), icon_rect, text_color)
            }
        }
        let label = if is_main {
            t!("timeline.main_track").into_owned()
        } else if effects_tracks[track_index] {
            t!("timeline.effects_track").into_owned()
        } else {
            track_labels[track_index].clone()
        };
        ui.painter().text(
            row_rect.left_top() + egui::vec2(28.0, 11.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(11.0),
            text_color,
        );

        // Buttons on the lower line: the row is at least 40 px high.
        let button_y = row_rect.bottom() - 12.0;
        let button =
            |x: f32,
             id: &str,
             enabled: bool,
             hover: &str,
             paint: &dyn Fn(&egui::Painter, egui::Rect, egui::Color32)| {
                let rect = egui::Rect::from_center_size(
                    egui::pos2(row_rect.left() + x, button_y),
                    egui::vec2(22.0, 18.0),
                );
                let sense = if enabled {
                    egui::Sense::click()
                } else {
                    egui::Sense::hover()
                };
                let resp = ui
                    .interact(rect, ui.id().with(id).with(track_index), sense)
                    .on_hover_text(hover);
                if enabled && resp.hovered() {
                    ui.painter()
                        .rect_filled(rect, 3.0, egui::Color32::from_gray(60));
                }
                let color = if enabled {
                    text_color
                } else {
                    text_color.gamma_multiply(0.35)
                };
                paint(ui.painter(), rect, color);
                enabled && resp.clicked()
            };
        if button(
            18.0,
            "lock_track",
            true,
            &t!("timeline.lock_track"),
            &|p, r, _| paint_lock_icon(p, r, flags.locked),
        ) {
            *pending = Some(PendingAction::SetTrackFlag(
                track_index,
                TrackFlag::Locked,
                !flags.locked,
            ));
        }
        if is_main
            && button(
                96.0,
                "edit_cover",
                true,
                &t!("timeline.edit_cover"),
                &|p, r, c| crate::timeline_bar::paint_pencil_icon(p, r, c),
            )
        {
            *pending = Some(PendingAction::EditCover);
        }
        let is_video = kind == TrackKind::Video;
        let eye_hover = if is_video {
            t!("timeline.hide_layer")
        } else {
            t!("timeline.no_visual_layer")
        };
        if button(44.0, "hide_track", is_video, &eye_hover, &|p, r, c| {
            crate::timeline_bar::paint_eye_icon(p, r, !(is_video && flags.muted), c)
        }) {
            *pending = Some(PendingAction::SetTrackFlag(
                track_index,
                TrackFlag::Muted,
                !flags.muted,
            ));
        }
        let speaker_hover = if is_video {
            t!("timeline.audio_on_audio_tracks")
        } else {
            t!("timeline.mute")
        };
        if button(70.0, "mute_track", !is_video, &speaker_hover, &|p, r, c| {
            crate::timeline_bar::paint_speaker_icon(p, r, is_video || !flags.muted, c)
        }) {
            *pending = Some(PendingAction::SetTrackFlag(
                track_index,
                TrackFlag::Muted,
                !flags.muted,
            ));
        }
    }

    // The empty part of each box, above the video rows and below the audio
    // ones: right click adds a track there.
    let empty_parts = [
        (
            TrackKind::Video,
            layout.video_pane,
            layout.video_pane.min,
            layout.video_rows_top,
        ),
        (
            TrackKind::Audio,
            layout.audio_pane,
            layout.audio_rows_bottom(),
            layout.audio_pane.max,
        ),
    ];
    for (kind, pane, top, bottom) in empty_parts {
        let (top, bottom) = (top.max(pane.min), bottom.min(pane.max));
        if bottom - top < 1.0 {
            continue;
        }
        let empty =
            egui::Rect::from_x_y_ranges(rect.x_range(), (origin.y + top)..=(origin.y + bottom));
        ui.set_clip_rect(full_clip.intersect(empty));
        let resp = ui.interact(
            empty,
            ui.id().with(("header_empty", kind == TrackKind::Video)),
            egui::Sense::click(),
        );
        resp.context_menu(|ui| {
            let label = match kind {
                TrackKind::Video => t!("timeline.add_video_track"),
                TrackKind::Audio => t!("timeline.add_audio_track"),
            };
            if ui.button(label).clicked() {
                *pending = Some(PendingAction::AddTrack(kind));
            }
        });
    }

    ui.set_clip_rect(full_clip);

    if layout.divider_height > 0.0 {
        let divider_rect = egui::Rect::from_min_size(
            egui::pos2(origin.x, origin.y + layout.video_pane.max),
            egui::vec2(TRACK_HEADER_WIDTH, layout.divider_height),
        );
        interact_divider(
            ui,
            ui.painter(),
            divider_rect,
            ui.id().with("timeline_header_track_split"),
            layout,
            video_pane_height,
        );
    }
}
