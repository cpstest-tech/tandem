//! Effects, Filters and Transitions tabs of the Library block: asset
//! folders (`vv_render::assets`) in a grid, with hover previews rendered by
//! the same shaders the timeline uses.

use super::*;
use crate::library_ribbon::{self, hover_buttons};
use crate::pool_view;
use std::collections::HashMap;
use std::sync::Arc;
use vv_core::AssetFx;
use vv_render::assets::{self, Asset, AssetKind};
use vv_render::wgpu;

const PREVIEW: (u32, u32) = (160, 90);
const CELL_TARGET: f32 = 84.0;
const LABEL_HEIGHT: f32 = 16.0;
const LOOP_FPS: f64 = 18.0;
const TRANSITION_FRAMES: usize = 14;
const HOLD_FRAMES: usize = 4;
const LOOP_FRAMES: usize = 2 * (TRANSITION_FRAMES + HOLD_FRAMES);
const FRAMES_PER_UI_FRAME: usize = 6;
const STILLS_PER_UI_FRAME: usize = 4;
/// Sidebar entry for the previous, hand-made list of generators, filters and
/// presets.
const CLASSIC: &str = "@classic";
const FAVORITES: &str = "";

/// What is dragged out of a cell. An effect or filter lands on a clip or on
/// the timeline; a transition only between two adjacent clips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FxDrag {
    pub id: String,
    pub kind: AssetKind,
    /// Length of the element made on the timeline.
    pub millis: u32,
}

pub(crate) fn fx_key(id: &str) -> String {
    format!("fx:{id}")
}

struct Samples {
    device: Arc<wgpu::Device>,
    a: wgpu::Texture,
    b: wgpu::Texture,
}

#[derive(Default)]
pub(crate) struct FxState {
    search: String,
    selected: HashMap<u8, String>,
    samples: Option<Samples>,
    stills: HashMap<String, egui::TextureHandle>,
    loops: HashMap<String, Vec<egui::TextureHandle>>,
    hover_since: Option<(String, f64)>,
}

enum FxAction {
    Favorite(String),
    Add(String),
}

fn kind_slot(kind: AssetKind) -> u8 {
    kind as u8
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t.clamp(0.0, 1.0))
}

/// The two bundled sample pictures, drawn in code: a dusk and a meadow.
pub(crate) fn sample_pixels(variant: u8) -> Vec<u8> {
    let (w, h) = PREVIEW;
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
            let rgb = if variant == 0 {
                let sky = mix([0.10, 0.12, 0.38], [0.98, 0.58, 0.26], v * 1.4);
                let d = ((u - 0.66) * 16.0 / 9.0).hypot(v - 0.52);
                let sun = (1.0 - (d - 0.12) / 0.02).clamp(0.0, 1.0);
                let lit = mix(sky, [1.0, 0.92, 0.6], sun);
                let ridge = 0.74 + 0.07 * (u * 9.0).sin() + 0.03 * (u * 23.0).sin();
                if v > ridge { [0.06, 0.07, 0.11] } else { lit }
            } else {
                let sky = mix([0.28, 0.62, 0.86], [0.85, 0.95, 1.0], v * 1.3);
                let d = ((u - 0.25) * 16.0 / 9.0).hypot(v - 0.28);
                let cloud = (1.0 - (d - 0.09) / 0.05).clamp(0.0, 1.0);
                let lit = mix(sky, [1.0, 1.0, 1.0], cloud * 0.9);
                let ground = 0.6 + 0.04 * (u * 7.0).cos();
                if v > ground {
                    let stripe = ((u * 40.0 + (v - ground) * 30.0).floor() as i32 & 1) as f32;
                    mix([0.16, 0.5, 0.2], [0.3, 0.7, 0.28], stripe)
                } else {
                    lit
                }
            };
            out.extend(rgb.map(|c| (c * 255.0).round() as u8));
            out.push(255);
        }
    }
    out
}

impl Samples {
    fn new(compositor: &vv_render::Compositor) -> Self {
        use wgpu::util::DeviceExt;
        let make = |variant| {
            compositor.device().create_texture_with_data(
                compositor.queue(),
                &wgpu::TextureDescriptor {
                    label: Some("fx preview sample"),
                    size: wgpu::Extent3d {
                        width: PREVIEW.0,
                        height: PREVIEW.1,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                &sample_pixels(variant),
            )
        };
        Self {
            device: Arc::clone(compositor.device()),
            a: make(0),
            b: make(1),
        }
    }
}

/// Which input order, progress and time the frame `index` of an asset's loop
/// shows. Transitions go A to B, hold, then back; effects just run.
fn loop_frame(kind: AssetKind, index: usize) -> (bool, f32, f32) {
    if kind != AssetKind::Transition {
        return (
            false,
            index as f32 / LOOP_FRAMES as f32,
            index as f32 / LOOP_FPS as f32,
        );
    }
    let steps = (TRANSITION_FRAMES - 1) as f32;
    let cycle = TRANSITION_FRAMES + HOLD_FRAMES;
    let swapped = index >= cycle;
    let i = index % cycle;
    let progress = if i < TRANSITION_FRAMES {
        i as f32 / steps
    } else {
        1.0
    };
    (swapped, progress, i as f32 / LOOP_FPS as f32)
}

fn render_frame(
    compositor: &vv_render::Compositor,
    samples: &Samples,
    asset: &Asset,
    (swapped, progress, time): (bool, f32, f32),
) -> Option<egui::ColorImage> {
    let (a, b) = if asset.kind == AssetKind::Transition {
        if swapped {
            (&samples.b, &samples.a)
        } else {
            (&samples.a, &samples.b)
        }
    } else {
        (&samples.a, &samples.a)
    };
    let pass = vv_render::CustomPass {
        key: &asset.id,
        source: &asset.shader,
        params: asset.shader_params(&asset.default_params()),
        curves: asset.curve_values(progress),
        progress,
        time,
        amount: 1.0,
    };
    let out = compositor.render_custom_pass(&pass, a, b)?;
    let layers = [vv_render::Layer::new(
        vv_render::LayerContent::Texture {
            texture: &out,
            source_size: PREVIEW,
        },
        vv_core::Transform::default(),
    )];
    let rgba =
        compositor.render_layers(&layers, vv_render::OutputFrame::exact(PREVIEW.0, PREVIEW.1));
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [PREVIEW.0 as usize, PREVIEW.1 as usize],
        &rgba,
    ))
}

pub(crate) fn category_label(id: &str) -> String {
    match id {
        FAVORITES => t!("ribbon.favorites").into_owned(),
        CLASSIC => t!("fx.cat_classic").into_owned(),
        "basic" => t!("fx.cat_basic").into_owned(),
        "motion" => t!("fx.cat_motion").into_owned(),
        "distort" => t!("fx.cat_distort").into_owned(),
        "style" => t!("fx.cat_style").into_owned(),
        "film" => t!("fx.cat_film").into_owned(),
        "bw" => t!("fx.cat_bw").into_owned(),
        "stylize" => t!("fx.cat_stylize").into_owned(),
        "dynamic" => t!("fx.cat_dynamic").into_owned(),
        other => {
            let mut chars = other.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
    }
}

fn truncated_label(
    painter: &egui::Painter,
    text: &str,
    width: f32,
    color: egui::Color32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        egui::FontId::proportional(theme::FONT_SMALL),
        color,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    painter.layout_job(job)
}

/// Appends the effect or filter to each clip, as one undo step.
pub(crate) fn apply_asset_to_clips(
    project: &mut vv_core::Project,
    history: &mut vv_core::History,
    timeline_id: vv_core::TimelineId,
    targets: &[(usize, vv_core::ClipId)],
    asset: &Asset,
) {
    let group = history.begin_group();
    for &(track, clip) in targets {
        let Some(mut list) = project.timelines[timeline_id]
            .clip(track, clip)
            .map(|c| c.effects.assets.clone())
        else {
            continue;
        };
        list.push(AssetFx::new(asset.id.clone(), asset.default_params()));
        history.do_command(
            project,
            Box::new(vv_core::set_clip_assets(timeline_id, track, clip, list)),
        );
    }
    history.end_group_as(group, vv_core::CommandLabel::Filters);
}

impl VenturiApp {
    pub(crate) fn show_fx_tab(&mut self, ui: &mut egui::Ui, kind: AssetKind) {
        let area = ui.available_rect_before_wrap();
        let library = assets::library();
        let lang = rust_i18n::locale().to_string();
        pool_view::search_field(ui, &mut self.fx.search, "fx");
        ui.add_space(theme::SPACE_1);

        let mut categories: Vec<String> = vec![FAVORITES.to_owned()];
        categories.extend(library.categories(kind).into_iter().map(str::to_owned));
        if kind == AssetKind::Effect {
            categories.push(CLASSIC.to_owned());
        }
        let slot = kind_slot(kind);
        let current = self
            .fx
            .selected
            .get(&slot)
            .filter(|c| categories.contains(c))
            .cloned()
            .unwrap_or_else(|| categories.get(1).cloned().unwrap_or_default());

        let rest = ui.available_rect_before_wrap();
        let side_width = 104.0_f32.min(rest.width() * 0.4);
        let side_rect = rest.with_max_x(rest.left() + side_width);
        let content_rect = rest.with_min_x(side_rect.right() + theme::SPACE_2);
        let mut chosen = current.clone();
        let mut side = ui.new_child(egui::UiBuilder::new().max_rect(side_rect));
        egui::ScrollArea::vertical()
            .id_salt(("fx_sidebar", slot))
            .show(&mut side, |ui| {
                ui.set_width(side_width);
                for category in &categories {
                    let selected = *category == current;
                    let text = egui::RichText::new(category_label(category)).color(if selected {
                        theme::ACCENT
                    } else {
                        theme::TEXT_PRIMARY
                    });
                    if ui
                        .add_sized(
                            [ui.available_width(), theme::CONTROL_HEIGHT],
                            egui::Button::selectable(selected, text),
                        )
                        .clicked()
                    {
                        chosen.clone_from(category);
                    }
                }
            });
        if chosen != current {
            self.fx.selected.insert(slot, chosen.clone());
        }

        let mut content = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
        let mut actions: Vec<FxAction> = Vec::new();
        if chosen == CLASSIC {
            self.show_effects_list(&mut content);
        } else {
            let query = self.fx.search.trim().to_lowercase();
            let favorites = self.settings.favorites.clone();
            let entries: Vec<&Asset> = library
                .of_kind(kind)
                .filter(|a| {
                    if chosen == FAVORITES {
                        library_ribbon::is_favorite(&favorites, &fx_key(&a.id))
                    } else {
                        a.category == chosen
                    }
                })
                .filter(|a| query.is_empty() || a.name.get(&lang).to_lowercase().contains(&query))
                .collect();
            if entries.is_empty() {
                content.add_space(theme::SPACE_2);
                content.weak(if chosen == FAVORITES && query.is_empty() {
                    t!("fx.no_favorites")
                } else {
                    t!("pool.search_no_results")
                });
            } else {
                self.show_fx_grid(&mut content, &entries, &lang, &favorites, &mut actions);
            }
        }
        for action in actions {
            match action {
                FxAction::Favorite(id) => self.toggle_favorite_key(&fx_key(&id)),
                FxAction::Add(id) => self.add_fx_at_playhead(&id),
            }
        }
        ui.allocate_rect(area, egui::Sense::hover());
    }

    fn show_fx_grid(
        &mut self,
        ui: &mut egui::Ui,
        entries: &[&Asset],
        lang: &str,
        favorites: &[String],
        actions: &mut Vec<FxAction>,
    ) {
        let gap = theme::SPACE_2;
        let width = ui.available_width();
        let columns = (((width + gap) / (CELL_TARGET + gap)) as usize).max(1);
        let side = ((width - gap * (columns - 1) as f32) / columns as f32).floor();
        let now = ui.input(|i| i.time);
        let mut hovered: Option<String> = None;
        let mut new_stills = 0;
        let mut built_frames = 0;
        egui::ScrollArea::vertical()
            .id_salt("fx_grid")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
                ui.horizontal_wrapped(|ui| {
                    for asset in entries {
                        let size = egui::vec2(side, side + LABEL_HEIGHT);
                        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                        let id = ui.id().with(("fx_cell", &asset.id));
                        let response = ui.interact(rect, id, egui::Sense::click_and_drag());
                        let image_rect =
                            egui::Rect::from_min_size(rect.min, egui::vec2(side, side));
                        let is_hovered = response.hovered() || response.dragged();
                        if is_hovered {
                            hovered = Some(asset.id.clone());
                        }
                        let texture = self.fx_cell_texture(
                            ui.ctx(),
                            asset,
                            is_hovered,
                            now,
                            &mut new_stills,
                            &mut built_frames,
                        );
                        let painter = ui.painter();
                        painter.rect_filled(image_rect, theme::RADIUS_CONTROL, theme::BG_INSET);
                        if let Some(texture) = texture {
                            let crop = (1.0 - 9.0 / 16.0) / 2.0;
                            painter.image(
                                texture,
                                image_rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(crop, 0.0),
                                    egui::pos2(1.0 - crop, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                        }
                        if is_hovered {
                            painter.rect_stroke(
                                image_rect,
                                theme::RADIUS_CONTROL,
                                egui::Stroke::new(1.5, theme::ACCENT),
                                egui::StrokeKind::Inside,
                            );
                        }
                        let name = asset.name.get(lang);
                        let galley = truncated_label(painter, name, side, theme::TEXT_SECONDARY);
                        painter.galley(
                            egui::pos2(rect.left(), image_rect.bottom() + 1.0),
                            galley,
                            theme::TEXT_SECONDARY,
                        );
                        let drag = FxDrag {
                            id: asset.id.clone(),
                            kind: asset.kind,
                            millis: (asset.default_secs * 1000.0).round() as u32,
                        };
                        response.dnd_set_drag_payload(drag);
                        let response =
                            response.on_hover_text(if asset.kind == AssetKind::Transition {
                                t!("fx.drag_transition")
                            } else {
                                t!("fx.drag_effect")
                            });
                        if response.dragged() {
                            media_pool_ui::show_drag_ghost(ui, id, name);
                        }
                        if ui.rect_contains_pointer(image_rect) && !response.dragged() {
                            let favorite =
                                library_ribbon::is_favorite(favorites, &fx_key(&asset.id));
                            let (star, plus) = hover_buttons(ui, id, image_rect, favorite);
                            if star {
                                actions.push(FxAction::Favorite(asset.id.clone()));
                            }
                            if plus {
                                actions.push(FxAction::Add(asset.id.clone()));
                            }
                        }
                    }
                });
            });
        match hovered {
            Some(id) => {
                match &self.fx.hover_since {
                    Some((current, _)) if *current == id => {}
                    _ => self.fx.hover_since = Some((id, now)),
                }
                ui.ctx().request_repaint();
            }
            None => self.fx.hover_since = None,
        }
        let loading = self.fx.stills.len() < entries.len();
        if new_stills >= STILLS_PER_UI_FRAME || built_frames > 0 || loading {
            ui.ctx().request_repaint();
        }
    }

    pub(crate) fn show_clip_animations(
        &mut self,
        ui: &mut egui::Ui,
        animations: &mut vv_core::clip_anim::ClipAnimations,
    ) -> (bool, bool) {
        let before = animations.clone();
        let slot = crate::properties_tabs::segmented(
            ui,
            "clip_animation_slot",
            &[
                (0, t!("props.anim_in")),
                (1, t!("props.anim_out")),
                (2, t!("props.anim_combo")),
            ],
        );
        let category = match slot {
            0 => "in",
            1 => "out",
            _ => "combo",
        };
        let selected = match slot {
            0 => &animations.in_asset,
            1 => &animations.out_asset,
            _ => &animations.combo_asset,
        }
        .clone();
        let library = assets::library();
        let entries: Vec<_> = library
            .of_kind(AssetKind::Animation)
            .filter(|a| a.category == category)
            .collect();
        let lang = rust_i18n::locale().to_string();
        let gap = theme::SPACE_2;
        let columns = ((ui.available_width() + gap) / (CELL_TARGET + gap))
            .floor()
            .max(1.0);
        let side = ((ui.available_width() - gap * (columns - 1.0)) / columns).floor();
        let now = ui.input(|i| i.time);
        let mut stills = 0;
        let mut frames = 0;
        let mut hovered = None;
        let mut choice = None;
        ui.add_space(gap);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            for asset in std::iter::once(None).chain(entries.iter().copied().map(Some)) {
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(side, side + LABEL_HEIGHT),
                    egui::Sense::click(),
                );
                let image_rect = egui::Rect::from_min_size(rect.min, egui::vec2(side, side));
                let painter = ui.painter().clone();
                painter.rect_filled(image_rect, theme::RADIUS_CONTROL, theme::BG_INSET);
                let label = asset.map_or_else(
                    || t!("props.clip_anim_none").into_owned(),
                    |a| a.name.get(&lang).to_owned(),
                );
                if let Some(asset) = asset {
                    if response.hovered() {
                        hovered = Some(asset.id.clone());
                    }
                    if let Some(texture) = self.fx_cell_texture(
                        ui.ctx(),
                        asset,
                        response.hovered(),
                        now,
                        &mut stills,
                        &mut frames,
                    ) {
                        painter.image(
                            texture,
                            image_rect,
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                } else {
                    painter.text(
                        image_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "∅",
                        egui::FontId::proportional(28.0),
                        theme::TEXT_SECONDARY,
                    );
                }
                if selected.as_deref() == asset.map(|a| a.id.as_str()) || response.hovered() {
                    painter.rect_stroke(
                        image_rect,
                        theme::RADIUS_CONTROL,
                        egui::Stroke::new(1.5, theme::ACCENT),
                        egui::StrokeKind::Inside,
                    );
                }
                let text = truncated_label(&painter, &label, side, theme::TEXT_SECONDARY);
                painter.galley(
                    egui::pos2(rect.left(), image_rect.bottom() + 1.0),
                    text,
                    theme::TEXT_SECONDARY,
                );
                if response.on_hover_text(label).clicked() {
                    choice = Some(asset);
                }
            }
        });
        let restart = matches!(choice, Some(Some(_)));
        if let Some(asset) = choice {
            let id = asset.map(|a| a.id.clone());
            match slot {
                0 => {
                    animations.in_asset = id;
                    animations.entrance = None;
                    if let Some(a) = asset {
                        animations.in_seconds = a.default_secs;
                    }
                }
                1 => {
                    animations.out_asset = id;
                    animations.exit = None;
                    if let Some(a) = asset {
                        animations.out_seconds = a.default_secs;
                    }
                }
                _ => {
                    animations.combo_asset = id;
                    animations.combo_offset_seconds = 0.0;
                    animations.combo_length_seconds = None;
                    animations.pulse = false;
                }
            }
        }
        if let Some(id) = hovered {
            if self
                .fx
                .hover_since
                .as_ref()
                .is_none_or(|(current, _)| *current != id)
            {
                self.fx.hover_since = Some((id, now));
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(33));
        } else {
            self.fx.hover_since = None;
        }
        if stills >= STILLS_PER_UI_FRAME || frames > 0 {
            ui.ctx().request_repaint();
        }
        ui.add_space(gap);
        match slot {
            0 if animations.in_asset.is_some() => {
                ui.add(
                    egui::Slider::new(&mut animations.in_seconds, 0.05..=5.0)
                        .text(t!("props.anim_duration"))
                        .suffix(" s"),
                );
            }
            1 if animations.out_asset.is_some() => {
                ui.add(
                    egui::Slider::new(&mut animations.out_seconds, 0.05..=5.0)
                        .text(t!("props.anim_duration"))
                        .suffix(" s"),
                );
            }
            _ => {}
        }
        (*animations != before, restart)
    }

    /// The picture a cell shows: the running loop while hovered (built a few
    /// frames per UI frame so the first hover does not stall), else a still.
    fn fx_cell_texture(
        &mut self,
        ctx: &egui::Context,
        asset: &Asset,
        hovered: bool,
        now: f64,
        new_stills: &mut usize,
        built_frames: &mut usize,
    ) -> Option<egui::TextureId> {
        let compositor = &self.compositor;
        let fx = &mut self.fx;
        if fx
            .samples
            .as_ref()
            .is_none_or(|s| !Arc::ptr_eq(&s.device, compositor.device()))
        {
            fx.samples = Some(Samples::new(compositor));
            fx.stills.clear();
            fx.loops.clear();
        }
        let samples = fx.samples.as_ref()?;
        if hovered {
            let frames = fx.loops.entry(asset.id.clone()).or_default();
            for _ in 0..FRAMES_PER_UI_FRAME {
                if frames.len() >= LOOP_FRAMES {
                    break;
                }
                let spec = loop_frame(asset.kind, frames.len());
                let Some(image) = render_frame(compositor, samples, asset, spec) else {
                    break;
                };
                frames.push(ctx.load_texture(
                    format!("fx-{}-{}", asset.id, frames.len()),
                    image,
                    egui::TextureOptions::LINEAR,
                ));
                *built_frames += 1;
            }
            if !frames.is_empty() {
                let start = fx.hover_since.as_ref().map_or(now, |(_, t)| *t);
                let index = (((now - start).max(0.0) * LOOP_FPS) as usize) % frames.len();
                return Some(frames[index].id());
            }
        }
        if !fx.stills.contains_key(&asset.id)
            && *new_stills < STILLS_PER_UI_FRAME
            && let Some(image) = render_frame(compositor, samples, asset, (false, 0.5, 0.4))
        {
            let handle = ctx.load_texture(
                format!("fx-still-{}", asset.id),
                image,
                egui::TextureOptions::LINEAR,
            );
            fx.stills.insert(asset.id.clone(), handle);
            *new_stills += 1;
        }
        fx.stills.get(&asset.id).map(egui::TextureHandle::id)
    }

    pub(crate) fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), std::time::Instant::now()));
    }

    pub(crate) fn show_toast(&mut self, ctx: &egui::Context) {
        const SHOWN: std::time::Duration = std::time::Duration::from_millis(3200);
        if let Some(text) = self.timeline_state.toast.take() {
            self.toast(text);
        }
        let Some((text, since)) = &self.toast else {
            return;
        };
        if since.elapsed() > SHOWN {
            self.toast = None;
            return;
        }
        egui::Area::new(egui::Id::new("toast"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -32.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(theme::BG_MENU)
                    .show(ui, |ui| ui.label(text.as_str()));
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }

    /// The «+» of a cell: effects and filters go on the selected video
    /// clips, or become an element on a new track at the playhead; a
    /// transition goes on the selected adjacent pair, or the cut nearest to
    /// the playhead.
    pub(crate) fn add_fx_at_playhead(&mut self, id: &str) {
        let library = assets::library();
        let Some(asset) = library.get(id) else {
            return;
        };
        match asset.kind {
            AssetKind::Animation => {}
            AssetKind::Transition => {
                let Some(pair) = self.transition_pair_for_add() else {
                    self.toast(t!("fx.no_adjacent_pair"));
                    return;
                };
                self.apply_asset_transition_to(pair, asset);
            }
            AssetKind::Effect | AssetKind::Filter => {
                let targets = self.selected_video_clips();
                if targets.is_empty() {
                    let start = self.timeline_state.playhead;
                    self.add_fx_element_at(
                        id,
                        (asset.default_secs * 1000.0) as u32,
                        start,
                        timeline_ui::MediaDropTarget::NewVideoTrack,
                    );
                } else {
                    self.apply_fx_to_clips(&targets, asset);
                }
            }
        }
    }

    fn selected_video_clips(&self) -> Vec<(usize, vv_core::ClipId)> {
        let Some(timeline) = self
            .timeline_id
            .map(|id| &self.session.project.timelines[id])
        else {
            return Vec::new();
        };
        self.timeline_state
            .selected
            .iter()
            .copied()
            .filter(|&(track, id)| {
                timeline.tracks.get(track).is_some_and(|t| {
                    t.kind == vv_core::TrackKind::Video && !t.locked && t.clip(id).is_some()
                })
            })
            .collect()
    }

    pub(crate) fn apply_fx_to_clips(
        &mut self,
        targets: &[(usize, vv_core::ClipId)],
        asset: &Asset,
    ) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        apply_asset_to_clips(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            targets,
            asset,
        );
    }

    /// An effect element: an adjustment clip carrying the effect, which
    /// processes every track below for its length. It goes on the effects
    /// track under the pointer when it has room, else on another one, else on
    /// a new one on top; then it is selected so Properties shows its
    /// parameters.
    pub(crate) fn add_fx_element_at(
        &mut self,
        id: &str,
        millis: u32,
        start: FrameIdx,
        target: timeline_ui::MediaDropTarget,
    ) {
        let library = assets::library();
        let Some(asset) = library.get(id) else {
            return;
        };
        let group = self.session.history.begin_group();
        let timeline_id = self.ensure_timeline();
        let fps = self.session.project.timelines[timeline_id].fps.as_f64();
        let len = ((fps * f64::from(millis) / 1000.0).round() as FrameIdx).max(1);
        let preferred = match target {
            timeline_ui::MediaDropTarget::Track(track) => Some(track),
            _ => None,
        };
        let element = vv_core::fx::insert_fx_element(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            preferred,
            start,
            len,
            vec![AssetFx::new(asset.id.clone(), asset.default_params())],
        );
        self.session
            .history
            .end_group_as(group, vv_core::CommandLabel::InsertClips);
        self.timeline_state.selected = std::iter::once(element).collect();
    }

    fn transition_pair_for_add(&self) -> Option<(usize, vv_core::ClipId, vv_core::ClipId)> {
        let timeline = &self.session.project.timelines[self.timeline_id?];
        let pairs = vv_core::fx::adjacent_pairs(timeline);
        let selected = &self.timeline_state.selected;
        if selected.is_empty() {
            let playhead = self.timeline_state.playhead;
            return pairs.into_iter().min_by_key(|&(track, left, _)| {
                timeline.tracks[track]
                    .clip(left)
                    .map_or(FrameIdx::MAX, |c| (c.timeline_end() - playhead).abs())
            });
        }
        let has = |track, id| selected.contains(&(track, id));
        pairs
            .iter()
            .find(|&&(t, l, r)| has(t, l) && has(t, r))
            .or_else(|| pairs.iter().find(|&&(t, l, _)| has(t, l)))
            .or_else(|| pairs.iter().find(|&&(t, _, r)| has(t, r)))
            .copied()
    }

    pub(crate) fn apply_asset_transition_to(
        &mut self,
        (track, left, right): (usize, vv_core::ClipId, vv_core::ClipId),
        asset: &Asset,
    ) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let applied = vv_core::fx::apply_asset_transition(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            track,
            (left, right),
            AssetFx::new(asset.id.clone(), asset.default_params()),
            asset.default_secs,
        );
        if applied {
            timeline_ui::select_crossing(&mut self.timeline_state, track, left);
        } else {
            self.toast(t!("fx.no_adjacent_pair"));
        }
    }

    pub(crate) fn is_effect_element(&self, target: &PanelTarget) -> bool {
        self.session
            .project
            .timelines
            .get(target.timeline)
            .and_then(|t| t.clip(target.track_index, target.clip_id))
            .is_some_and(vv_core::Clip::is_effect_element)
    }

    /// Rows of the asset effects and filters of the first target clip: switch,
    /// parameters (double click resets), remove. One slider drag is one undo
    /// step: the app merges the commands while the pointer is down.
    pub(crate) fn show_asset_effects_section(
        &mut self,
        ui: &mut egui::Ui,
        target: &PanelTarget,
        pending: &mut Vec<crate::properties_panel::BoxedCommand>,
    ) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let Some(before) = self.session.project.timelines[timeline_id]
            .clip(target.track_index, target.clip_id)
            .map(|c| c.effects.assets.clone())
        else {
            return;
        };
        if before.is_empty() {
            return;
        }
        ui.add_space(crate::theme::SPACE_2);
        let (open, reset_all) =
            crate::properties_panel::section_header(ui, &t!("fx.props_title"), true);
        let library = assets::library();
        let lang = rust_i18n::locale().to_string();
        let mut after = before.clone();
        if reset_all {
            for fx in &mut after {
                if let Some(asset) = library.get(&fx.id) {
                    fx.params = asset.default_params();
                }
            }
        }
        let mut remove = None;
        for (index, fx) in after.iter_mut().enumerate().filter(|_| open) {
            let asset = library.get(&fx.id);
            ui.horizontal(|ui| {
                ui.checkbox(&mut fx.enabled, "");
                match asset {
                    Some(asset) => ui.strong(asset.name.get(&lang)),
                    None => ui.weak(t!("fx.unknown")),
                };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("×")
                        .on_hover_text(t!("fx.remove"))
                        .clicked()
                    {
                        remove = Some(index);
                    }
                });
            });
            let Some(asset) = asset else {
                continue;
            };
            fx.params.resize(asset.params.len(), 0.0);
            for (value, param) in fx.params.iter_mut().zip(&asset.params) {
                ui.horizontal(|ui| {
                    ui.label(param.name.get(&lang));
                    let response =
                        ui.add(egui::Slider::new(value, param.min..=param.max).max_decimals(3));
                    let reset = crate::properties_panel::reset_icon_button(ui).clicked();
                    if reset || response.double_clicked() {
                        *value = param.default;
                    }
                });
            }
            ui.add_space(crate::theme::SPACE_1);
        }
        if let Some(index) = remove {
            after.remove(index);
        }
        if after != before {
            pending.push(Box::new(vv_core::set_clip_assets(
                target.timeline,
                target.track_index,
                target.clip_id,
                after,
            )));
        }
    }

    /// Parameters of the asset transition in `transition`, for the transition
    /// panel. `true` if any changed.
    pub(crate) fn show_transition_params(
        ui: &mut egui::Ui,
        transition: &mut vv_core::Transition,
    ) -> bool {
        let Some(fx) = transition.asset.as_mut() else {
            return false;
        };
        let library = assets::library();
        let Some(asset) = library.get(&fx.id) else {
            return false;
        };
        let lang = rust_i18n::locale().to_string();
        let before = fx.params.clone();
        fx.params.resize(asset.params.len(), 0.0);
        for (value, param) in fx.params.iter_mut().zip(&asset.params) {
            ui.horizontal(|ui| {
                ui.label(param.name.get(&lang));
                let response =
                    ui.add(egui::Slider::new(value, param.min..=param.max).max_decimals(3));
                if response.double_clicked() {
                    *value = param.default;
                }
            });
        }
        fx.params != before
    }

    /// Alt+G: the selected layers (video, text, effects, with their linked
    /// audio) become one compound clip.
    pub(crate) fn make_compound_from_selection(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let selected: Vec<_> = self.timeline_state.selected.iter().copied().collect();
        if selected.is_empty() {
            self.toast(t!("fx.compound_needs_selection"));
            return;
        }
        let clips = timeline_ui::with_linked_clips(&self.session.project, timeline_id, selected);
        timeline_ui::make_compound_clip(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            clips,
        );
        self.timeline_state.clear_selection();
    }

    /// Alt+Shift+G.
    pub(crate) fn ungroup_selected_compound(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let Some(&(track, clip)) = self.timeline_state.selected.iter().next() else {
            self.toast(t!("fx.not_compound"));
            return;
        };
        match vv_core::fx::ungroup_compound_clip(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            track,
            clip,
        ) {
            Ok(created) => {
                self.timeline_state.clear_selection();
                self.timeline_state.selected.extend(created);
            }
            Err(vv_core::fx::UngroupError::NotCompound) => self.toast(t!("fx.not_compound")),
            Err(vv_core::fx::UngroupError::CarriesEffects) => {
                self.toast(t!("fx.ungroup_has_effects"));
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/fx_library.rs"]
mod tests;
