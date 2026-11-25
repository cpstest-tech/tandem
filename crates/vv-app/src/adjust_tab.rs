//! The "Adjust" tab: basic sliders, HSL, RGB curves and LUT. All of them are
//! params of the clip's color correction, created on the first edit.

use std::path::PathBuf;

use egui::{Color32, Stroke, pos2, vec2};
use vv_core::{CurveChannel, GradeParam, HslBand};

use crate::grade_panel::{GradeInfo, GradeSectionResponse, grade_commands, scalar_row};
use crate::properties_panel::{BoxedCommand, section_header, set_filters, target_effects};
use crate::{PanelTarget, VenturiApp, theme};

const CURVE_HEIGHT: f32 = 150.0;

fn basic_label(param: GradeParam) -> std::borrow::Cow<'static, str> {
    match param {
        GradeParam::Temperature => t!("props.temp"),
        GradeParam::Tint => t!("props.tint"),
        GradeParam::Brightness => t!("props.brightness"),
        GradeParam::Contrast => t!("props.contrast"),
        GradeParam::Highlights => t!("props.highlight"),
        GradeParam::Shadows => t!("props.shadows"),
        GradeParam::Vignette => t!("props.vignette"),
        GradeParam::Sharpen => t!("props.sharpen"),
        _ => "".into(),
    }
}

pub(crate) fn band_label(band: HslBand) -> std::borrow::Cow<'static, str> {
    match band {
        HslBand::Red => t!("props.hsl_red"),
        HslBand::Orange => t!("props.hsl_orange"),
        HslBand::Yellow => t!("props.hsl_yellow"),
        HslBand::Green => t!("props.hsl_green"),
        HslBand::Cyan => t!("props.hsl_cyan"),
        HslBand::Blue => t!("props.hsl_blue"),
        HslBand::Purple => t!("props.hsl_purple"),
        HslBand::Magenta => t!("props.hsl_magenta"),
    }
}

pub(crate) fn channel_label(channel: CurveChannel) -> std::borrow::Cow<'static, str> {
    match channel {
        CurveChannel::Master => t!("props.curve_master"),
        CurveChannel::Red => t!("props.hsl_red"),
        CurveChannel::Green => t!("props.hsl_green"),
        CurveChannel::Blue => t!("props.hsl_blue"),
    }
}

fn band_color(band: HslBand) -> Color32 {
    let hsva = egui::ecolor::Hsva::new(band.hue(), 0.85, 0.9, 1.0);
    Color32::from(hsva)
}

fn channel_color(channel: CurveChannel) -> Color32 {
    match channel {
        CurveChannel::Master => theme::TEXT_PRIMARY,
        CurveChannel::Red => Color32::from_rgb(230, 80, 80),
        CurveChannel::Green => Color32::from_rgb(80, 200, 100),
        CurveChannel::Blue => Color32::from_rgb(90, 140, 240),
    }
}

/// Five handles at fixed inputs, dragged up and down.
fn curve_graph(
    ui: &mut egui::Ui,
    info: &GradeInfo,
    channel: CurveChannel,
    response: &mut GradeSectionResponse,
) {
    let (rect, area) = ui.allocate_exact_size(
        vec2(ui.available_width().min(260.0), CURVE_HEIGHT),
        egui::Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, theme::RADIUS_CONTROL, theme::BG_INSET);
    for i in 1..4 {
        let f = i as f32 / 4.0;
        let stroke = Stroke::new(1.0, theme::BORDER_SUBTLE);
        painter.vline(rect.left() + f * rect.width(), rect.y_range(), stroke);
        painter.hline(rect.x_range(), rect.bottom() - f * rect.height(), stroke);
    }
    painter.line_segment(
        [rect.left_bottom(), rect.right_top()],
        Stroke::new(1.0, theme::TEXT_DISABLED),
    );
    let params = channel.params();
    let mut knots = params.map(|p| info.value.get(p));
    let drag_id = ui.id().with(("curve_drag", channel as u8));
    let mut dragged: Option<usize> = ui.data(|d| d.get_temp(drag_id));
    let to_screen = |i: usize, v: f32| {
        pos2(
            rect.left() + i as f32 / 4.0 * rect.width(),
            rect.bottom() - v * rect.height(),
        )
    };
    if area.drag_started()
        && let Some(pos) = area.interact_pointer_pos()
    {
        dragged = Some((((pos.x - rect.left()) / rect.width() * 4.0).round() as usize).min(4));
    }
    if let (Some(i), Some(pos)) = (dragged, area.interact_pointer_pos())
        && area.dragged()
    {
        knots[i] = ((rect.bottom() - pos.y) / rect.height()).clamp(0.0, 1.0);
        response.set(params[i], knots[i]);
    }
    if area.drag_stopped() {
        dragged = None;
    }
    if area.double_clicked() {
        response.reset.extend(params);
        knots = params.map(GradeParam::neutral);
    }
    ui.data_mut(|d| match dragged {
        Some(i) => {
            d.insert_temp(drag_id, i);
        }
        None => d.remove::<usize>(drag_id),
    });
    let line: Vec<_> = (0..=64)
        .map(|s| {
            let x = s as f32 / 64.0;
            pos2(
                rect.left() + x * rect.width(),
                rect.bottom() - vv_core::curve_at(knots, x) * rect.height(),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        line,
        Stroke::new(2.0, channel_color(channel)),
    ));
    for (i, v) in knots.iter().enumerate() {
        painter.circle(
            to_screen(i, *v),
            4.5,
            channel_color(channel),
            Stroke::new(1.0, Color32::BLACK),
        );
    }
    area.on_hover_text(t!("props.curves_hint"));
}

/// The color correction of every target set to `edit`, adding one where
/// missing: what any change on this tab commits.
fn commands_for(
    tl: Option<&vv_core::Timeline>,
    targets: &[PanelTarget],
    section: &GradeSectionResponse,
    lut: Option<Option<PathBuf>>,
) -> Vec<BoxedCommand> {
    let mut commands = Vec::new();
    let mut with_grade = Vec::new();
    for t in targets {
        let Some(effects) = target_effects(tl, t) else {
            continue;
        };
        let clip_ref = (t.timeline, t.track_index, t.clip_id);
        let mut filters = effects.filters.clone();
        let pos = match filters.iter().position(|f| f.kind.has_grade()) {
            Some(pos) => {
                with_grade.push(*t);
                pos
            }
            None => {
                let mut filter = vv_core::ClipFilter::new(vv_core::FilterKind::ColorCorrection);
                for (edit, _) in &section.keyframes {
                    if let crate::properties_panel::KeyframeEdit::Set(
                        vv_core::KeyframeValue::Grade(param, v),
                    ) = edit
                    {
                        filter.grade.track_mut(*param).default = *v;
                    }
                }
                filters.push(filter);
                filters.len() - 1
            }
        };
        if let Some(lut) = &lut {
            filters[pos].lut = lut.clone();
        }
        if filters != effects.filters {
            commands.push(set_filters(clip_ref, filters));
        }
    }
    // Set after the LUT change: the keyframed edits on clips already graded.
    commands.extend(grade_commands(tl, &with_grade, section));
    commands
}

impl VenturiApp {
    /// «Retouch»: a one-click enhance of the playhead frame, written to the
    /// Adjust tab's sliders.
    pub(crate) fn show_retouch_tab(
        &mut self,
        ui: &mut egui::Ui,
        targets: &[PanelTarget],
        pending: &mut Vec<BoxedCommand>,
    ) {
        let Some(primary) = targets.first().copied() else {
            return;
        };
        ui.add_space(6.0);
        let (open, _) = section_header(ui, &t!("props.enhance"), false);
        if !open {
            return;
        }
        ui.small(t!("props.enhance_hint"));
        if !ui.button(t!("props.enhance_apply")).clicked() {
            return;
        }
        let grade = {
            let tl = &self.session.project.timelines[primary.timeline];
            tl.clip(primary.track_index, primary.clip_id)
                .and_then(|c| c.effects.filters.iter().find(|f| f.kind.has_grade()))
                .map_or(vv_core::GradeValue::NEUTRAL, |f| {
                    f.grade.value_at(primary.source_frame)
                })
        };
        let Some(pixels) = self.isolated_pixels(primary) else {
            return;
        };
        let pixels = pixels.as_chunks::<4>().0;
        let step = (pixels.len() / 250_000).max(1);
        let enhanced = vv_core::auto_enhance(
            &grade,
            pixels
                .iter()
                .step_by(step)
                .map(|p| [p[0], p[1], p[2]].map(|c| c as f32 / 255.0)),
        );
        let mut section = GradeSectionResponse::default();
        for param in GradeParam::ALL {
            if enhanced.get(param) != grade.get(param) {
                section.set(param, enhanced.get(param));
            }
        }
        let tl = self
            .timeline_id
            .map(|id| &self.session.project.timelines[id]);
        pending.extend(commands_for(tl, targets, &section, None));
    }

    pub(crate) fn show_adjust_tab(
        &mut self,
        ui: &mut egui::Ui,
        targets: &[PanelTarget],
        pending: &mut Vec<BoxedCommand>,
    ) {
        self.poll_lut_pick();
        let Some(primary) = targets.first().copied() else {
            return;
        };
        let tl = self
            .timeline_id
            .map(|id| &self.session.project.timelines[id]);
        let Some(clip) = tl.and_then(|tl| tl.clip(primary.track_index, primary.clip_id)) else {
            return;
        };
        let in_clip = |f: &vv_core::FrameIdx| {
            let at = clip.timeline_frame_at(*f);
            clip.contains(at)
        };
        let filter = clip.effects.filters.iter().find(|f| f.kind.has_grade());
        let default_grade = vv_core::GradeTracks::default();
        let info = GradeInfo::of(
            filter.map_or(&default_grade, |f| &f.grade),
            primary.source_frame,
            in_clip,
        );
        let current_lut = filter.and_then(|f| f.lut.clone());
        let mut section = GradeSectionResponse::default();
        let mut lut_change: Option<Option<PathBuf>> = self.adjust.picked_lut.take().map(Some);
        let mut pick = false;

        ui.add_space(6.0);
        let (open, _) = section_header(ui, &t!("props.adj_basic"), false);
        if open {
            for param in vv_core::BASIC_ADJUSTMENTS {
                scalar_row(
                    ui,
                    &mut section,
                    &info.value,
                    param,
                    info.keys[param.index()],
                    &basic_label(param),
                );
            }
        }

        ui.add_space(6.0);
        let (open, _) = section_header(ui, &t!("props.hsl"), false);
        if open {
            ui.horizontal_wrapped(|ui| {
                for band in HslBand::ALL {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::click());
                    ui.painter()
                        .circle_filled(rect.center(), 9.0, band_color(band));
                    if self.adjust.band == band {
                        ui.painter().circle_stroke(
                            rect.center(),
                            10.5,
                            Stroke::new(1.5, theme::TEXT_PRIMARY),
                        );
                    }
                    if response.on_hover_text(band_label(band)).clicked() {
                        self.adjust.band = band;
                    }
                }
            });
            let labels = [
                t!("props.hue"),
                t!("props.saturation"),
                t!("props.brightness"),
            ];
            for (param, label) in self.adjust.band.params().into_iter().zip(labels) {
                scalar_row(
                    ui,
                    &mut section,
                    &info.value,
                    param,
                    info.keys[param.index()],
                    &label,
                );
            }
        }

        ui.add_space(6.0);
        let (open, _) = section_header(ui, &t!("props.curves_rgb"), false);
        if open {
            ui.horizontal(|ui| {
                for channel in CurveChannel::ALL {
                    if ui
                        .selectable_label(self.adjust.channel == channel, channel_label(channel))
                        .clicked()
                    {
                        self.adjust.channel = channel;
                    }
                }
            });
            curve_graph(ui, &info, self.adjust.channel, &mut section);
        }

        ui.add_space(6.0);
        let (open, _) = section_header(ui, &t!("props.lut"), false);
        if open {
            ui.horizontal(|ui| {
                if ui.button(t!("props.lut_import")).clicked() {
                    pick = true;
                }
                if current_lut.is_some() && ui.button(t!("props.lut_remove")).clicked() {
                    lut_change = Some(None);
                }
            });
            if let Some(path) = &current_lut {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                ui.small(name);
                scalar_row(
                    ui,
                    &mut section,
                    &info.value,
                    GradeParam::LutAmount,
                    info.keys[GradeParam::LutAmount.index()],
                    &t!("props.lut_amount"),
                );
            }
            if let Some(error) = &self.adjust.lut_error {
                ui.colored_label(theme::ERROR, error);
            }
        }

        // Both come from the same stale effects: a LUT change goes alone.
        if lut_change.is_some() {
            let none = GradeSectionResponse::default();
            pending.extend(commands_for(tl, targets, &none, lut_change));
        } else if !section.is_empty() {
            pending.extend(commands_for(tl, targets, &section, None));
        }
        if pick {
            self.pick_lut();
        }
    }

    fn pick_lut(&mut self) {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(
                rfd::FileDialog::new()
                    .add_filter("LUT", &["cube", "CUBE"])
                    .pick_file(),
            );
        });
        self.adjust.lut_pick = Some(rx);
    }

    fn poll_lut_pick(&mut self) {
        let Some(rx) = &self.adjust.lut_pick else {
            return;
        };
        let picked = match rx.try_recv() {
            Ok(picked) => picked,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
        };
        self.adjust.lut_pick = None;
        let Some(path) = picked else {
            return;
        };
        match vv_core::lut::validate(&path) {
            Ok(()) => {
                self.adjust.lut_error = None;
                self.adjust.picked_lut = Some(path);
            }
            Err(e) => self.adjust.lut_error = Some(format!("{}: {e}", t!("props.lut_invalid"))),
        }
    }
}

/// What the tab remembers between frames.
pub(crate) struct AdjustState {
    pub(crate) band: HslBand,
    pub(crate) channel: CurveChannel,
    lut_pick: Option<std::sync::mpsc::Receiver<Option<PathBuf>>>,
    picked_lut: Option<PathBuf>,
    lut_error: Option<String>,
}

impl Default for AdjustState {
    fn default() -> Self {
        Self {
            band: HslBand::Red,
            channel: CurveChannel::Master,
            lut_pick: None,
            picked_lut: None,
            lut_error: None,
        }
    }
}
