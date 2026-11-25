use std::sync::atomic::Ordering;

use vv_core::FrameIdx;
use vv_core::auto_cut::{Segment, SegmentType};

use crate::auto_cut::{Analysis, Modal};
use crate::{VenturiApp, theme};

fn type_label(kind: SegmentType) -> std::borrow::Cow<'static, str> {
    match kind {
        SegmentType::Speech => t!("auto_cut.speech"),
        SegmentType::Silence => t!("auto_cut.silence"),
        SegmentType::FalseStart => t!("auto_cut.false_start"),
        SegmentType::Filler => t!("auto_cut.filler"),
    }
}

pub(crate) fn type_color(kind: SegmentType) -> egui::Color32 {
    match kind {
        SegmentType::Speech => theme::CLIP_VIDEO,
        SegmentType::Silence => theme::CLIP_AUDIO_FX,
        SegmentType::FalseStart => theme::CLIP_TEXT,
        SegmentType::Filler => theme::CLIP_EFFECT,
    }
}

pub(crate) fn timecode(seconds: f64) -> String {
    let ms = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

fn error_text(error: &str) -> String {
    match error {
        "locked" => t!("auto_cut.error_locked").into_owned(),
        "unsupported" => t!("auto_cut.error_unsupported").into_owned(),
        "offline" => t!("auto_cut.error_offline").into_owned(),
        "source_changed" => t!("auto_cut.error_source_changed").into_owned(),
        "timeline_changed" => t!("auto_cut.error_timeline_changed").into_owned(),
        "model_missing" => t!("auto_cut.error_model_missing").into_owned(),
        "no_audio" => t!("auto_cut.error_no_audio").into_owned(),
        "cancelled" => t!("auto_cut.cancelled").into_owned(),
        _ => t!("auto_cut.error", detail = error).into_owned(),
    }
}

impl VenturiApp {
    pub(crate) fn show_auto_cut(&mut self, ctx: &egui::Context) {
        let Some(mut modal) = self.auto_cut.modal.take() else {
            return;
        };
        if self.timeline_id != Some(modal.timeline) {
            return;
        }
        modal.poll(ctx);
        self.cache_auto_cut_source(&modal);
        if self.session.history.generation() != modal.generation {
            modal.error = Some("timeline_changed".into());
        }
        self.ensure_active_clip_matches_playhead(false);
        self.drive_playback();
        if self.is_timeline_playing() {
            self.auto_cut_skip(&modal);
        }
        self.sync_timeline_audio();
        self.sync_render_ahead();
        self.sync_root_timeline_media();
        if let Some(layers) = self.timeline_video_layers(None) {
            let output = self.viewer_output(&layers);
            self.show_composited(layers, output);
        }
        let mut close = false;
        let mut apply = false;
        let mut seek = None;
        let mut toggle_play = false;
        let screen = ctx.content_rect();
        let response = egui::Modal::new(egui::Id::new("auto_cut"))
            .area(
                egui::Modal::default_area(egui::Id::new("auto_cut"))
                    .anchor(egui::Align2::LEFT_TOP, egui::Vec2::ZERO),
            )
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG_WINDOW)
                    .inner_margin(theme::PANEL_PADDING),
            )
            .show(ctx, |ui| {
                ui.set_min_size(screen.size() - egui::Vec2::splat(theme::PANEL_PADDING * 2.0));
                ui.set_max_size(screen.size() - egui::Vec2::splat(theme::PANEL_PADDING * 2.0));
                egui::Panel::top("auto_cut_header")
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            crate::ui_kit::heading(ui, t!("auto_cut.title"));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    close |= ui.button(t!("auto_cut.close")).clicked();
                                },
                            );
                        });
                        ui.weak(t!("auto_cut.hint"));
                        ui.add_space(theme::SPACE_2);
                    });
                egui::Panel::bottom("auto_cut_footer")
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        ui.separator();
                        if let Some(error) = &modal.error {
                            ui.colored_label(theme::ERROR, error_text(error));
                        }
                        if let Some(job) = &modal.job {
                            let stage = job.progress.stage.load(Ordering::Relaxed);
                            let label = match stage {
                                0 => t!("auto_cut.hashing"),
                                1 => t!("auto_cut.extracting"),
                                _ => t!("auto_cut.transcribing"),
                            };
                            let fraction =
                                job.progress.done.load(Ordering::Relaxed) as f32 / 1000.0;
                            ui.label(label);
                            ui.add(egui::ProgressBar::new(fraction).show_percentage());
                            if stage == 2 {
                                ui.add(
                                    egui::ProgressBar::new(job.progress.whisper.fraction())
                                        .text(t!("auto_cut.current_block")),
                                );
                            }
                        } else if modal.analyzing() {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(t!("auto_cut.analyzing"));
                            });
                        }
                        let cuts = modal.removed_frames();
                        let removed = cuts
                            .iter()
                            .map(|&(s, e)| (e - s) as f64 / modal.fps)
                            .sum::<f64>();
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                egui::RichText::new(t!(
                                    "auto_cut.summary",
                                    before = timecode(modal.duration()),
                                    after = timecode((modal.duration() - removed).max(0.0)),
                                    removed = timecode(removed)
                                ))
                                .strong(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let enabled = modal.error.is_none()
                                        && modal.job.is_none()
                                        && !modal.analyzing()
                                        && !cuts.is_empty();
                                    apply |= ui
                                        .add_enabled(
                                            enabled,
                                            crate::ui_kit::primary_button(t!("auto_cut.delete")),
                                        )
                                        .clicked();
                                    close |= ui.button(t!("common.cancel")).clicked();
                                },
                            );
                        });
                    });
                egui::Panel::top("auto_cut_settings")
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        let mut changed = false;
                        ui.add_enabled_ui(modal.source.is_some() && modal.error.is_none(), |ui| {
                            ui.horizontal_wrapped(|ui| {
                                changed |= ui
                                    .add(
                                        egui::Slider::new(
                                            &mut modal.config.silence_db,
                                            -60.0..=-15.0,
                                        )
                                        .text(t!("auto_cut.threshold"))
                                        .suffix(" dB"),
                                    )
                                    .changed();
                                let mut padding = modal.config.padding * 1000.0;
                                changed |= ui
                                    .add(
                                        egui::Slider::new(&mut padding, 0.0..=500.0)
                                            .step_by(10.0)
                                            .text(t!("auto_cut.padding"))
                                            .suffix(" ms"),
                                    )
                                    .changed();
                                modal.config.padding = padding / 1000.0;
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.checkbox(&mut modal.ripple_all, t!("auto_cut.ripple"))
                                    .on_hover_text(t!("auto_cut.ripple_hint"));
                                ui.checkbox(&mut modal.preview, t!("auto_cut.preview"));
                                for kind in [
                                    SegmentType::Speech,
                                    SegmentType::Silence,
                                    SegmentType::FalseStart,
                                    SegmentType::Filler,
                                ] {
                                    ui.colored_label(type_color(kind), type_label(kind));
                                }
                            });
                        });
                        if changed {
                            modal.request_analysis();
                            if self.is_timeline_playing() {
                                self.toggle_playback();
                            }
                        }
                        ui.add_space(theme::SPACE_2);
                    });
                let available = ui.available_size();
                let left_width = (available.x * 0.61).max(0.0);
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(left_width, available.y),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            let canvas_height =
                                (available.y - theme::CONTROL_HEIGHT * 2.0 - theme::SPACE_5 * 5.0)
                                    .max(theme::CONTROL_HEIGHT);
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(left_width, canvas_height),
                                egui::Sense::hover(),
                            );
                            ui.painter()
                                .rect_filled(rect, theme::RADIUS_PANEL, theme::BG_INSET);
                            let audio_only = self.session.project.timelines[modal.timeline].tracks
                                [modal.target.0]
                                .kind
                                == vv_core::TrackKind::Audio;
                            if !audio_only
                                && let Some((texture, size)) =
                                    self.video_texture_id.zip(self.video_display_size)
                            {
                                let scale = (rect.width() / size.x).min(rect.height() / size.y);
                                let image =
                                    egui::Rect::from_center_size(rect.center(), size * scale);
                                ui.painter().image(
                                    texture,
                                    image,
                                    egui::Rect::from_min_max(
                                        egui::Pos2::ZERO,
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    theme::ON_ACCENT,
                                );
                            } else {
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    t!("auto_cut.audio_preview"),
                                    egui::FontId::proportional(theme::FONT_SECTION),
                                    theme::TEXT_SECONDARY,
                                );
                            }
                            let local = (self.timeline_state.playhead - modal.clip.timeline_start)
                                as f64
                                / modal.fps;
                            ui.horizontal(|ui| {
                                let ready = modal.analysis.is_some()
                                    && !modal.analyzing()
                                    && modal.error.is_none();
                                toggle_play |= ui
                                    .add_enabled(
                                        ready,
                                        egui::Button::new(if self.is_timeline_playing() {
                                            t!("auto_cut.pause")
                                        } else {
                                            t!("auto_cut.play")
                                        }),
                                    )
                                    .clicked();
                                ui.monospace(format!(
                                    "{} / {}",
                                    timecode(local),
                                    timecode(modal.duration())
                                ));
                            });
                            let mut position = local.clamp(0.0, modal.duration());
                            if ui
                                .add(
                                    egui::Slider::new(&mut position, 0.0..=modal.duration())
                                        .show_value(false),
                                )
                                .changed()
                            {
                                seek = Some(position);
                            }
                            if let Some(analysis) = &modal.analysis {
                                if let Some(at) = waveform(ui, analysis, modal.duration(), local) {
                                    seek = Some(at);
                                }
                            } else {
                                ui.weak(t!("auto_cut.waiting"));
                            }
                        },
                    );
                    let width = ui.available_width();
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, available.y),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_width(width);
                            ui.horizontal_wrapped(|ui| {
                                ui.add_enabled_ui(!modal.analyzing(), |ui| {
                                    if ui.button(t!("auto_cut.all_silences")).clicked() {
                                        select_type(&mut modal, SegmentType::Silence);
                                    }
                                    if ui.button(t!("auto_cut.all_restarts")).clicked() {
                                        select_type(&mut modal, SegmentType::FalseStart);
                                    }
                                    if ui.button(t!("auto_cut.invert")).clicked()
                                        && let Some(a) = &mut modal.analysis
                                    {
                                        for s in &mut a.segments {
                                            s.checked = !s.checked;
                                        }
                                    }
                                });
                            });
                            let analyzing = modal.analyzing();
                            if let Some(analysis) = &mut modal.analysis {
                                let row_height = theme::CONTROL_HEIGHT * 2.0;
                                egui::ScrollArea::vertical()
                                    .id_salt("auto_cut_segments")
                                    .auto_shrink([false, false])
                                    .max_height(ui.available_height())
                                    .show_rows(
                                        ui,
                                        row_height,
                                        analysis.segments.len(),
                                        |ui, rows| {
                                            for index in rows {
                                                let s = &mut analysis.segments[index];
                                                let (rect, response) = ui.allocate_exact_size(
                                                    egui::vec2(ui.available_width(), row_height),
                                                    egui::Sense::click(),
                                                );
                                                if response.hovered()
                                                    || modal.active_segment == Some(s.id)
                                                {
                                                    ui.painter().rect_filled(
                                                        rect,
                                                        theme::RADIUS_CONTROL,
                                                        theme::BG_SELECTED,
                                                    );
                                                }
                                                let mut row = ui.new_child(
                                                    egui::UiBuilder::new()
                                                        .id_salt(s.id)
                                                        .max_rect(rect.shrink(theme::SPACE_1))
                                                        .layout(egui::Layout::top_down(
                                                            egui::Align::Min,
                                                        )),
                                                );
                                                row.horizontal(|ui| {
                                                    ui.add_enabled(
                                                        !analyzing,
                                                        egui::Checkbox::without_text(
                                                            &mut s.checked,
                                                        ),
                                                    );
                                                    ui.monospace(timecode(s.start));
                                                    egui::Frame::NONE
                                                        .fill(type_color(s.kind))
                                                        .corner_radius(theme::RADIUS_TAG)
                                                        .show(ui, |ui| {
                                                            ui.label(
                                                                egui::RichText::new(type_label(
                                                                    s.kind,
                                                                ))
                                                                .small()
                                                                .color(theme::TEXT_PRIMARY),
                                                            );
                                                        });
                                                    ui.weak(format!("{:.2} s", s.end - s.start));
                                                });
                                                row.add(
                                                    egui::Label::new(if s.text.is_empty() {
                                                        type_label(s.kind).into_owned()
                                                    } else {
                                                        s.text.clone()
                                                    })
                                                    .truncate(),
                                                )
                                                .on_hover_text(&s.text);
                                                if response.clicked() {
                                                    seek = Some(s.start);
                                                    modal.active_segment = Some(s.id);
                                                }
                                            }
                                        },
                                    );
                            }
                        },
                    );
                });
            });
        close |= response.should_close();
        if !ctx.egui_wants_keyboard_input()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space))
        {
            toggle_play = true;
        }
        if let Some(at) = seek {
            self.timeline_state.playhead =
                modal.clip.timeline_start + (at * modal.fps).round() as FrameIdx;
            self.ensure_active_clip_matches_playhead(true);
        }
        if toggle_play && modal.analysis.is_some() && modal.error.is_none() && !modal.analyzing() {
            if self.timeline_state.playhead >= modal.clip.timeline_end() {
                self.timeline_state.playhead = modal.clip.timeline_start;
            }
            if !self.is_timeline_playing() {
                self.auto_cut_skip(&modal);
            }
            if self.timeline_state.playhead < modal.clip.timeline_end() {
                if self.is_timeline_playing() {
                    self.toggle_playback();
                } else {
                    // Preview playback must never start a voiceover take.
                    let at = self.timeline_state.playhead;
                    self.timeline_audio();
                    self.sync_timeline_audio();
                    self.timeline_audio().seek_frame(at, modal.fps);
                    self.timeline_audio().play();
                    self.last_synced_playhead = self.timeline_state.playhead;
                }
            }
        }
        if self.is_timeline_playing()
            || self
                .render_ahead
                .as_ref()
                .is_some_and(|r| !r.is_caught_up())
        {
            ctx.request_repaint();
        }
        if apply && !close {
            if !modal.source_unchanged() {
                modal.error = Some("source_changed".into());
            } else {
                let cuts = modal.removed_frames();
                match vv_core::auto_cut_edit::AutoCut::plan(
                    &mut self.session.project,
                    modal.timeline,
                    modal.target,
                    &cuts,
                    modal.ripple_all,
                    modal.config.crossfade_seconds,
                ) {
                    Ok(command) => {
                        let selection = command.selection;
                        let start = command.start;
                        if self.is_timeline_playing() {
                            self.toggle_playback();
                        }
                        self.session
                            .history
                            .do_command(&mut self.session.project, Box::new(command));
                        let timeline = &self.session.project.timelines[modal.timeline];
                        let group =
                            selection.and_then(|(t, id)| timeline.clip(t, id)?.linked_group);
                        let mut selected: std::collections::BTreeSet<_> =
                            selection.into_iter().collect();
                        if let Some(group) = group {
                            for (index, track) in timeline.tracks.iter().enumerate() {
                                selected.extend(
                                    track
                                        .clips
                                        .iter()
                                        .filter(|c| c.linked_group == Some(group))
                                        .map(|c| (index, c.id)),
                                );
                            }
                        }
                        self.timeline_state.set_selection(selected, selection);
                        self.timeline_state.playhead = start;
                        self.timeline_state.reveal_playhead = true;
                        self.ensure_active_clip_matches_playhead(true);
                        if self.active_clip.is_none() {
                            self.last_viewer_frame_kind = None;
                        }
                        return;
                    }
                    Err(error) => modal.error = Some(error.into()),
                }
            }
        }
        if close {
            if self.is_timeline_playing() {
                self.toggle_playback();
            }
            self.timeline_state.playhead = modal.original_playhead;
            self.timeline_state.set_selection(
                modal.original_selection.clone(),
                modal.original_selection.first().copied(),
            );
            self.ensure_active_clip_matches_playhead(true);
            return;
        }
        self.auto_cut.modal = Some(modal);
    }

    fn auto_cut_skip(&mut self, modal: &Modal) {
        let start = modal.clip.timeline_start;
        let end = modal.clip.timeline_end();
        let mut at = self.timeline_state.playhead.max(start);
        if modal.preview {
            for (s, e) in modal.removed_frames() {
                if at >= s && at < e {
                    at = e;
                }
            }
        }
        if at >= end {
            if self.is_timeline_playing() {
                self.toggle_playback();
            }
            at = end;
        }
        if at != self.timeline_state.playhead {
            self.timeline_state.playhead = at;
            self.ensure_active_clip_matches_playhead(true);
        }
    }
}

fn select_type(modal: &mut Modal, kind: SegmentType) {
    if let Some(a) = &mut modal.analysis {
        for s in &mut a.segments {
            if s.kind == kind {
                s.checked = true;
            }
        }
    }
}

fn waveform(ui: &mut egui::Ui, analysis: &Analysis, duration: f64, playhead: f64) -> Option<f64> {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::SPACE_5 * 4.0),
        egui::Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, theme::RADIUS_PANEL, theme::BG_INSET);
    let x = |t: f64| rect.left() + (t / duration).clamp(0.0, 1.0) as f32 * rect.width();
    for Segment {
        start,
        end,
        kind,
        checked,
        ..
    } in &analysis.segments
    {
        let region = egui::Rect::from_min_max(
            egui::pos2(x(*start), rect.top()),
            egui::pos2(x(*end), rect.bottom()),
        );
        painter.rect_filled(
            region,
            theme::RADIUS_TAG,
            type_color(*kind).gamma_multiply(if *checked { 0.65 } else { 0.2 }),
        );
        if *checked {
            painter.line_segment(
                [region.left_bottom(), region.right_bottom()],
                egui::Stroke::new(theme::ICON_STROKE, theme::ERROR),
            );
        }
    }
    let columns = rect.width().ceil().max(1.0) as usize;
    for i in 0..columns {
        let from = i * analysis.energy.db.len() / columns;
        let to = ((i + 1) * analysis.energy.db.len() / columns)
            .max(from + 1)
            .min(analysis.energy.db.len());
        let db = analysis
            .energy
            .db
            .get(from..to)
            .unwrap_or_default()
            .iter()
            .copied()
            .fold(-120.0_f32, f32::max);
        let amplitude = 10.0_f32.powf(db / 20.0).sqrt().clamp(0.0, 1.0) * rect.height() * 0.45;
        let center = egui::pos2(rect.left() + i as f32, rect.center().y);
        painter.line_segment(
            [
                center - egui::vec2(0.0, amplitude),
                center + egui::vec2(0.0, amplitude),
            ],
            egui::Stroke::new(1.0, theme::TEXT_SECONDARY),
        );
    }
    painter.line_segment(
        [
            egui::pos2(x(playhead), rect.top()),
            egui::pos2(x(playhead), rect.bottom()),
        ],
        egui::Stroke::new(theme::ICON_STROKE, theme::PLAYHEAD),
    );
    if (response.clicked() || response.dragged())
        && let Some(pos) = response.interact_pointer_pos()
    {
        Some(((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64 * duration)
    } else {
        None
    }
}
