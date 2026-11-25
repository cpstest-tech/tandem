//! CapCut-style tabs of the properties panel, and the controls the engine
//! does not support yet (visible but disabled, «Presto disponibile»).

use super::*;
use crate::properties_panel::{
    BoxedCommand, param_row, push_param_changes, section_header, slider_field,
};
use std::borrow::Cow;
use vv_core::TransformParam as P;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ClipTab {
    #[default]
    Video,
    Audio,
    Speed,
    Animation,
    Adjust,
    /// Only for a clip of the subtitle track: the whole list of lines.
    Subtitles,
}

impl ClipTab {
    const ALL: [ClipTab; 5] = [
        ClipTab::Video,
        ClipTab::Audio,
        ClipTab::Speed,
        ClipTab::Animation,
        ClipTab::Adjust,
    ];

    fn label(self) -> Cow<'static, str> {
        match self {
            ClipTab::Video => t!("props.tab_video"),
            ClipTab::Audio => t!("props.tab_audio"),
            ClipTab::Speed => t!("props.tab_speed"),
            ClipTab::Animation => t!("props.tab_animation"),
            ClipTab::Adjust => t!("props.tab_adjust"),
            ClipTab::Subtitles => t!("props.tab_subtitles"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VideoSubTab {
    Basic,
    RemoveBackground,
    Mask,
    Retouch,
    AutoZoom,
    Reframe,
}

pub(crate) const VIDEO_SUB_TABS: [VideoSubTab; 6] = [
    VideoSubTab::Basic,
    VideoSubTab::RemoveBackground,
    VideoSubTab::Mask,
    VideoSubTab::Retouch,
    VideoSubTab::AutoZoom,
    VideoSubTab::Reframe,
];

/// Opens a Video sub-tab from outside the panel (its index lives in egui memory, see `segmented`).
pub(crate) fn select_video_sub_tab(ctx: &egui::Context, tab: VideoSubTab) {
    let index = VIDEO_SUB_TABS.iter().position(|t| *t == tab).unwrap_or(0);
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(("segmented", "video_subtab")), index));
}

impl VideoSubTab {
    pub(crate) fn label(self) -> Cow<'static, str> {
        match self {
            VideoSubTab::Basic => t!("props.sub_basic"),
            VideoSubTab::RemoveBackground => t!("props.sub_remove_bg"),
            VideoSubTab::Mask => t!("props.sub_mask"),
            VideoSubTab::Retouch => t!("props.sub_retouch"),
            VideoSubTab::AutoZoom => t!("auto_zoom.sub_tab"),
            VideoSubTab::Reframe => t!("reframe.sub_tab"),
        }
    }
}

/// Text tabs, no underline: the active one is cyan.
pub(crate) fn tab_bar(ui: &mut egui::Ui, current: &mut ClipTab, subtitles: bool) {
    let subtitle_tab = subtitles.then_some(ClipTab::Subtitles);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 24.0;
        for tab in subtitle_tab.into_iter().chain(ClipTab::ALL) {
            let color = if *current == tab {
                theme::ACCENT
            } else {
                theme::TEXT_SECONDARY
            };
            let text = egui::RichText::new(tab.label())
                .size(theme::FONT_BODY)
                .color(color);
            if ui
                .add(egui::Label::new(text).sense(egui::Sense::click()))
                .clicked()
            {
                *current = tab;
            }
        }
    });
}

/// Full-width segmented control; the selection lives in egui memory.
pub(crate) fn segmented<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id: &str,
    items: &[(T, Cow<'static, str>)],
) -> T {
    let key = egui::Id::new(("segmented", id));
    let stored: usize = ui.data(|d| d.get_temp(key)).unwrap_or(0);
    let mut index = stored.min(items.len() - 1);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, theme::RADIUS_CONTROL, theme::BG_RAISED);
    let width = rect.width() / items.len() as f32;
    for (i, (_, label)) in items.iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            egui::pos2(rect.left() + width * i as f32, rect.top()),
            egui::vec2(width, rect.height()),
        );
        let response = ui.interact(cell, key.with(i), egui::Sense::click());
        if response.clicked() {
            index = i;
        }
        if i == index {
            ui.painter()
                .rect_filled(cell, theme::RADIUS_CONTROL, theme::BG_SELECTED);
        }
        let color = if i == index {
            theme::ACCENT
        } else {
            theme::TEXT_PRIMARY
        };
        ui.painter().text(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(theme::FONT_SMALL),
            color,
        );
    }
    ui.data_mut(|d| d.insert_temp(key, index));
    items[index].0
}

fn soon(response: egui::Response) {
    response.on_disabled_hover_text(t!("common.coming_soon"));
}

/// Disabled slider row for a control the engine lacks.
pub(crate) fn placeholder_row(ui: &mut egui::Ui, label: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [PLACEHOLDER_LABEL_WIDTH, 18.0],
            egui::Label::new(egui::RichText::new(label).color(theme::TEXT_DISABLED)).truncate(),
        );
        let mut value = 0.0_f32;
        ui.spacing_mut().slider_width = (ui.available_width() - 70.0).clamp(60.0, 240.0);
        soon(ui.add_enabled(
            false,
            egui::Slider::new(&mut value, -100.0..=100.0).show_value(true),
        ));
    });
}

const PLACEHOLDER_LABEL_WIDTH: f32 = 120.0;

/// Folding section whose controls are all [placeholder] sliders.
pub(crate) fn placeholder_section(ui: &mut egui::Ui, title: &str, rows: &[Cow<'static, str>]) {
    ui.add_space(6.0);
    let (open, _) = section_header(ui, title, false);
    if open {
        for row in rows {
            placeholder_row(ui, row);
        }
    }
}

fn placeholder_checkbox_section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    let (open, _) = section_header(ui, title, false);
    if open {
        let mut off = false;
        soon(ui.add_enabled(false, egui::Checkbox::new(&mut off, title)));
    }
}

pub(crate) fn stabilize_section(ui: &mut egui::Ui) {
    placeholder_checkbox_section(ui, &t!("props.stabilize"));
}

type ChromaEdit = Box<dyn Fn(&mut vv_core::ClipFilter)>;

impl VenturiApp {
    /// Automatic cutout (the matting model) and chroma key of the clips.
    pub(crate) fn show_remove_background_tab(
        &mut self,
        ui: &mut egui::Ui,
        targets: &[PanelTarget],
        filters: &[crate::FilterPanelInfo],
        pending: &mut Vec<BoxedCommand>,
    ) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let timeline = &self.session.project.timelines[timeline_id];
        let media_of = |t: &PanelTarget| {
            let clip = timeline.clip(t.track_index, t.clip_id)?;
            let vv_core::ClipSource::Media(id) = clip.source else {
                return None;
            };
            let item = self.session.project.media_pool.get(id)?;
            (item.meta.has_video && item.compound.is_none()).then_some((clip, item))
        };
        let media: Vec<_> = targets
            .iter()
            .filter_map(|t| Some((*t, media_of(t)?)))
            .collect();

        ui.add_space(6.0);
        let (open, _) = section_header(ui, &t!("props.auto_cutout"), false);
        if open {
            match media.first() {
                None => {
                    ui.weak(t!("props.cutout_no_media"));
                }
                Some(&(_, (clip, item))) => {
                    let mut on = clip.effects.remove_background;
                    if ui
                        .checkbox(&mut on, t!("props.cutout_toggle"))
                        .on_hover_text(t!("props.cutout_hint"))
                        .changed()
                    {
                        for (t, _) in &media {
                            pending.push(Box::new(vv_core::set_clip_remove_background(
                                t.timeline,
                                t.track_index,
                                t.clip_id,
                                on,
                            )));
                        }
                    }
                    if on {
                        let hash = item.content_hash;
                        self.cutout_status(ui, hash, item.meta.is_image());
                    }
                }
            }
        }

        ui.add_space(6.0);
        let chroma = filters
            .iter()
            .find(|f| f.kind == vv_core::FilterKind::ChromaKey);
        let (open, _) = section_header(ui, &t!("props.chroma_key"), false);
        if !open {
            return;
        }
        let mut on = chroma.is_some();
        if ui.checkbox(&mut on, t!("filter.chroma_key")).changed() {
            let tl = Some(&self.session.project.timelines[timeline_id]);
            pending.extend(crate::properties_panel::toggle_filter_commands(
                tl,
                targets,
                vv_core::FilterKind::ChromaKey,
                on,
            ));
        }
        let Some(chroma) = chroma else {
            return;
        };
        let mut edit: Option<ChromaEdit> = None;
        let mut color = chroma.color;
        if crate::properties_panel::color_row(ui, &t!("props.key_color"), &mut color).changed {
            edit = Some(Box::new(move |f| f.color = color));
        }
        ui.horizontal(|ui| {
            for (label, rgb) in [
                (t!("props.key_green"), [0.0, 1.0, 0.0]),
                (t!("props.key_blue"), [0.0, 0.28, 1.0]),
            ] {
                if ui.small_button(label).clicked() {
                    let c = vv_core::Rgba {
                        r: rgb[0],
                        g: rgb[1],
                        b: rgb[2],
                        a: 1.0,
                    };
                    edit = Some(Box::new(move |f| f.color = c));
                }
            }
        });
        type Field = fn(&mut vv_core::ClipFilter) -> &mut vv_core::Keyframed<f32>;
        let rows: [(_, f32, Field); 3] = [
            (t!("props.key_tolerance"), chroma.amount, |f| &mut f.amount),
            (t!("props.key_softness"), chroma.grow, |f| &mut f.grow),
            (t!("props.key_spill"), chroma.radius, |f| &mut f.radius),
        ];
        for (label, value, field) in rows {
            let mut value = value;
            let row = crate::properties_panel::param_row(ui, &label, None, |ui| {
                crate::properties_panel::slider_field(ui, &mut value, 0.0..=100.0, 0.5, 1)
            });
            if row.changed {
                edit = Some(Box::new(move |f| field(f).default = value));
            }
        }
        if let Some(edit) = edit {
            let tl = Some(&self.session.project.timelines[timeline_id]);
            for t in targets {
                let Some(effects) = crate::properties_panel::target_effects(tl, t) else {
                    continue;
                };
                let mut new_filters = effects.filters.clone();
                for f in new_filters
                    .iter_mut()
                    .filter(|f| f.kind == vv_core::FilterKind::ChromaKey)
                {
                    edit(f);
                }
                pending.push(crate::properties_panel::set_filters(
                    (t.timeline, t.track_index, t.clip_id),
                    new_filters,
                ));
            }
        }
    }

    fn cutout_status(&mut self, ui: &mut egui::Ui, hash: u64, image: bool) {
        use crate::matte_worker::MatteState;
        if vv_media::matte::mattes_complete(hash) {
            ui.weak(t!("props.cutout_ready"));
            return;
        }
        match self.matte_worker.state(hash) {
            Some(MatteState::Running(progress)) => {
                let label = match progress.stage() {
                    vv_matting::Stage::Download => {
                        let model = if image {
                            &vv_matting::IMAGE_MODEL
                        } else {
                            &vv_matting::VIDEO_MODEL
                        };
                        t!("props.cutout_download", mb = model.size_mb())
                    }
                    _ if image => t!("props.cutout_running_image"),
                    _ => t!("props.cutout_running"),
                };
                ui.label(label);
                // A single image has no steps to count.
                let pulse = image && progress.stage() != vv_matting::Stage::Download;
                let bar = egui::ProgressBar::new(progress.fraction()).animate(pulse);
                ui.add(if pulse { bar } else { bar.show_percentage() });
                if ui.small_button(t!("common.cancel")).clicked() {
                    self.matte_worker.cancel(hash);
                }
            }
            Some(MatteState::Queued) | None => {
                ui.weak(t!("props.cutout_queued"));
            }
            Some(MatteState::Failed(error)) => {
                if error.is_empty() {
                    ui.weak(t!("props.cutout_stopped"));
                } else {
                    ui.colored_label(theme::ERROR, t!("props.cutout_failed", error = error));
                }
                if ui.small_button(t!("props.cutout_retry")).clicked() {
                    self.matte_worker.retry(hash);
                }
            }
        }
    }
}

pub(crate) fn retouch_tab(ui: &mut egui::Ui) {
    placeholder_checkbox_section(ui, &t!("props.enhance"));
}

pub(crate) fn audio_placeholders(ui: &mut egui::Ui) {
    placeholder_checkbox_section(ui, &t!("props.voice_changer"));
}

pub(crate) fn text_placeholders(ui: &mut egui::Ui) {
    placeholder_section(ui, &t!("props.bubble_effects"), &[t!("props.strength")]);
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
    Top,
    Middle,
    Bottom,
}

impl Align {
    const ALL: [Align; 6] = [
        Align::Left,
        Align::Center,
        Align::Right,
        Align::Top,
        Align::Middle,
        Align::Bottom,
    ];

    fn hint(self) -> Cow<'static, str> {
        match self {
            Align::Left => t!("props.align_left"),
            Align::Center => t!("props.align_center"),
            Align::Right => t!("props.align_right"),
            Align::Top => t!("props.align_top"),
            Align::Middle => t!("props.align_middle"),
            Align::Bottom => t!("props.align_bottom"),
        }
    }

    /// Translation (timeline pixels, Y up) that aligns `bounds` = [min x, max x, min y, max y].
    fn shift(self, bounds: [f32; 4], frame: (u32, u32)) -> [f32; 2] {
        let (hw, hh) = (frame.0 as f32 / 2.0, frame.1 as f32 / 2.0);
        match self {
            Align::Left => [-hw - bounds[0], 0.0],
            Align::Right => [hw - bounds[1], 0.0],
            Align::Center => [-(bounds[0] + bounds[1]) / 2.0, 0.0],
            Align::Top => [0.0, hh - bounds[3]],
            Align::Bottom => [0.0, -hh - bounds[2]],
            Align::Middle => [0.0, -(bounds[2] + bounds[3]) / 2.0],
        }
    }

    fn paint(self, painter: &egui::Painter, c: egui::Pos2, color: egui::Color32) {
        let stroke = egui::Stroke::new(theme::ICON_STROKE, color);
        let r = 6.0;
        let (edge, bar_a, bar_b): ([egui::Pos2; 2], [egui::Pos2; 2], [egui::Pos2; 2]);
        match self {
            Align::Left | Align::Center | Align::Right => {
                let x = match self {
                    Align::Left => c.x - r,
                    Align::Center => c.x,
                    _ => c.x + r,
                };
                edge = [egui::pos2(x, c.y - r), egui::pos2(x, c.y + r)];
                let reach = |len: f32| match self {
                    Align::Left => (x, x + len),
                    Align::Center => (x - len / 2.0, x + len / 2.0),
                    _ => (x - len, x),
                };
                let (a0, a1) = reach(9.0);
                let (b0, b1) = reach(5.0);
                bar_a = [egui::pos2(a0, c.y - 2.5), egui::pos2(a1, c.y - 2.5)];
                bar_b = [egui::pos2(b0, c.y + 2.5), egui::pos2(b1, c.y + 2.5)];
            }
            _ => {
                let y = match self {
                    Align::Top => c.y - r,
                    Align::Middle => c.y,
                    _ => c.y + r,
                };
                edge = [egui::pos2(c.x - r, y), egui::pos2(c.x + r, y)];
                let reach = |len: f32| match self {
                    Align::Top => (y, y + len),
                    Align::Middle => (y - len / 2.0, y + len / 2.0),
                    _ => (y - len, y),
                };
                let (a0, a1) = reach(9.0);
                let (b0, b1) = reach(5.0);
                bar_a = [egui::pos2(c.x - 2.5, a0), egui::pos2(c.x - 2.5, a1)];
                bar_b = [egui::pos2(c.x + 2.5, b0), egui::pos2(c.x + 2.5, b1)];
            }
        }
        for seg in [edge, bar_a, bar_b] {
            painter.line_segment(seg, stroke);
        }
    }
}

/// Position change for an alignment, or `None` if nothing moves.
fn aligned(
    which: Align,
    transform: &vv_core::Transform,
    timeline_size: (u32, u32),
    source_size: (u32, u32),
) -> Option<vv_core::Transform> {
    let bounds = crate::viewer_overlay::frame_bounds(timeline_size, source_size, transform);
    let shift = which.shift(bounds, timeline_size);
    let mut new = *transform;
    new.position = [
        transform.position[0] + shift[0],
        transform.position[1] + shift[1],
    ];
    (new.position != transform.position).then_some(new)
}

impl VenturiApp {
    /// Row of six alignment icons; a click moves the clip in one undo step.
    pub(crate) fn show_align_row(
        &self,
        ui: &mut egui::Ui,
        targets: &[PanelTarget],
        info: &ClipPanelInfo,
        pending: &mut Vec<BoxedCommand>,
    ) {
        ui.horizontal(|ui| {
            for which in Align::ALL {
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(28.0, 24.0), egui::Sense::click());
                let color = if response.hovered() {
                    egui::Color32::WHITE
                } else {
                    theme::TEXT_SECONDARY
                };
                which.paint(ui.painter(), rect.center(), color);
                if response.on_hover_text(which.hint()).clicked()
                    && let Some(new) =
                        aligned(which, &info.transform, info.timeline_size, info.source_size)
                {
                    let tl = self
                        .timeline_id
                        .map(|id| &self.session.project.timelines[id]);
                    push_param_changes(
                        pending,
                        tl,
                        targets,
                        &[P::PositionX, P::PositionY],
                        &new,
                        &info.transform,
                    );
                }
            }
        });
    }

    /// The "apply to all subtitles" switch, for a subtitle clip; whether the
    /// look is edited together.
    /// Middle of `clip` in ms, if it is a text clip of the subtitle track.
    pub(crate) fn subtitle_clip_ms(&self, clip: &PanelTarget) -> Option<u32> {
        let timeline = self.session.project.timelines.get(clip.timeline)?;
        let track = timeline
            .tracks
            .get(clip.track_index)
            .filter(|t| t.subtitles)?;
        let c = track.clip(clip.clip_id).filter(|_| clip.is_text)?;
        let middle = c.timeline_start as f64 + c.timeline_len as f64 / 2.0;
        Some((middle / timeline.fps.as_f64().max(1e-9) * 1000.0).round() as u32)
    }

    pub(crate) fn show_subtitle_link(
        &self,
        ui: &mut egui::Ui,
        clip: &PanelTarget,
        pending: &mut Vec<BoxedCommand>,
    ) -> bool {
        let project = &self.session.project;
        let on_subtitles = clip.is_text
            && project.timelines[clip.timeline]
                .tracks
                .get(clip.track_index)
                .is_some_and(|t| t.subtitles);
        let Some(captions) = project
            .captions
            .get(&clip.timeline)
            .filter(|_| on_subtitles)
        else {
            return false;
        };
        let mut linked = captions.linked;
        if ui
            .checkbox(&mut linked, t!("props.subtitles_linked"))
            .on_hover_text(t!("props.subtitles_linked_hint"))
            .changed()
        {
            pending.push(Box::new(vv_core::SetSubtitlesLinked::new(
                clip.timeline,
                linked,
            )));
        }
        linked
    }

    /// Fade in/out of the clips, in seconds; the timeline dots edit the same value.
    pub(crate) fn show_fade_rows(
        &self,
        ui: &mut egui::Ui,
        targets: &[PanelTarget],
        pending: &mut Vec<BoxedCommand>,
    ) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let timeline = &self.session.project.timelines[timeline_id];
        let fps = timeline.fps.as_f64().max(1e-9);
        let Some(clip) = targets
            .first()
            .and_then(|t| timeline.clip(t.track_index, t.clip_id))
        else {
            return;
        };
        let max_secs = (clip.timeline_len as f64 / fps) as f32;
        for (edge, label, frames) in [
            (vv_core::FadeEdge::In, t!("props.fade_in"), clip.fade_in),
            (vv_core::FadeEdge::Out, t!("props.fade_out"), clip.fade_out),
        ] {
            let mut secs = (frames as f64 / fps) as f32;
            let row = param_row(ui, &label, None, |ui| {
                slider_field(ui, &mut secs, 0.0..=max_secs.max(0.1), 0.05, 2)
            });
            if row.reset {
                secs = 0.0;
            }
            if row.changed || row.reset {
                let value = (f64::from(secs) * fps).round() as vv_core::FrameIdx;
                for t in targets {
                    pending.push(Box::new(vv_core::SetClipFade::new(
                        t.timeline,
                        t.track_index,
                        t.clip_id,
                        edge,
                        value,
                    )));
                }
            }
        }
    }

    /// Words of the timeline's subtitles that fall inside the text clip, when
    /// they match the words of its text.
    pub(crate) fn transcript_word_ms(
        &self,
        clip: &PanelTarget,
        title: &vv_core::TitleParams,
    ) -> Option<Vec<[u32; 2]>> {
        let tid = self.timeline_id?;
        let captions = self.session.project.captions.get(&tid)?;
        let timeline = self.session.project.timelines.get(tid)?;
        let c = timeline.tracks.get(clip.track_index)?.clip(clip.clip_id)?;
        let ms =
            |frame: i64| (frame as f64 / timeline.fps.as_f64().max(1.0) * 1000.0).round() as u32;
        let words = vv_core::split_units(&title.display_text(), vv_core::Granularity::Word).len();
        vv_core::captions::clip_word_ms(
            captions,
            ms(c.timeline_start),
            ms(c.timeline_start + c.timeline_len),
            words,
        )
    }

    /// Speed tab of a media clip: ×, resulting duration and pitch; committed
    /// when the gesture ends, so a drag is a single undo step.
    pub(crate) fn show_speed_tab(&mut self, ui: &mut egui::Ui, targets: &[PanelTarget]) {
        let kind = segmented(
            ui,
            "speed_subtab",
            &[
                (false, t!("props.speed_normal")),
                (true, t!("props.speed_curve")),
            ],
        );
        if kind {
            self.show_speed_curve(ui, targets);
            return;
        }
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let timeline = &self.session.project.timelines[timeline_id];
        let fps = timeline.fps.as_f64().max(1e-9);
        let Some(clip) = targets
            .first()
            .and_then(|t| timeline.clip(t.track_index, t.clip_id))
            .filter(|c| matches!(c.source, vv_core::ClipSource::Media(_)))
        else {
            ui.small(t!("props.speed_media_only"));
            return;
        };
        let current = clip.speed().as_percent() / 100.0;
        let (len_frames, pitch) = (clip.timeline_len, clip.pitch_correction);
        let clips: Vec<(usize, ClipId)> = targets
            .iter()
            .filter(|t| {
                timeline
                    .clip(t.track_index, t.clip_id)
                    .is_some_and(|c| matches!(c.source, vv_core::ClipSource::Media(_)))
            })
            .map(|t| (t.track_index, t.clip_id))
            .collect();

        let key = egui::Id::new("speed_tab_value");
        let mut value: f64 = ui.data(|d| d.get_temp(key)).unwrap_or(current);
        let (mut commit, mut reset) = (false, false);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_sized(
                [PLACEHOLDER_LABEL_WIDTH, 18.0],
                egui::Label::new(t!("props.speed_value")),
            );
            ui.spacing_mut().slider_width = (ui.available_width() - 110.0).clamp(60.0, 240.0);
            let slider = ui.add(
                egui::Slider::new(&mut value, 0.1..=100.0)
                    .logarithmic(true)
                    .show_value(false),
            );
            let field = ui.add(
                egui::DragValue::new(&mut value)
                    .range(0.1..=100.0)
                    .speed(0.05)
                    .max_decimals(2)
                    .suffix("×")
                    .update_while_editing(false),
            );
            let dragging = slider.dragged() || field.dragged();
            if slider.changed() || field.changed() {
                ui.data_mut(|d| d.insert_temp(key, value));
            }
            commit = slider.drag_stopped()
                || field.drag_stopped()
                || ((slider.changed() || field.changed()) && !dragging);
            if slider.double_clicked() {
                reset = true;
            }
            if crate::properties_panel::reset_icon_button(ui).clicked() {
                reset = true;
            }
        });
        if reset {
            value = 1.0;
            commit = true;
        }
        let new_secs = len_frames as f64 / fps * current / value.max(0.1);
        ui.horizontal(|ui| {
            ui.add_sized(
                [PLACEHOLDER_LABEL_WIDTH, 18.0],
                egui::Label::new(
                    egui::RichText::new(t!("props.speed_duration")).color(theme::TEXT_SECONDARY),
                ),
            );
            ui.label(timeline_ui::format_timecode(new_secs, fps));
        });
        let mut keep_pitch = pitch;
        let pitch_changed = ui
            .checkbox(&mut keep_pitch, t!("props.keep_pitch"))
            .changed();
        if commit || pitch_changed {
            ui.data_mut(|d| d.remove::<f64>(key));
            let percent = (value * 100.0).clamp(10.0, 10_000.0);
            self.session.apply(Box::new(vv_core::SetClipSpeed::new(
                timeline_id,
                clips,
                vv_core::Rational::from_percent(percent),
                keep_pitch,
                vv_core::SpeedFit::Ripple,
            )));
        }
    }
}
