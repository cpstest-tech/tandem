//! Tabs of the Library block: Media, Audio, Text and Sticker. Each has
//! categories, the first always the user's Favorites. Media belong to the
//! project; audio and stickers live in the user library (categories are
//! reserved folders of it); favorites and text templates are in the settings.

use super::*;
use crate::media_pool::PoolState;
use crate::pool_view::{
    self, HoverActions, MenuChoice, PoolEntry, PoolEvent, PoolMenus, PoolView, Thumb,
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::sync::Arc;
use vv_core::text_template::{TemplateCategory as TextCategory, TemplateClip};

pub(crate) const STICKER_SECS: f64 = 3.0;
pub(crate) const YOURS_STICKER_FOLDER: &str = "ribbon-sticker-yours";
const TEXT_SECS: f64 = 3.0;
const TEXT_CARD: egui::Vec2 = egui::vec2(160.0, 112.0);
const TEXT_PREVIEW: egui::Vec2 = egui::vec2(160.0, 90.0);
const PREVIEW_TEXTURE_WIDTH: u32 = 480;
pub(crate) const HOVER_BUTTON: f32 = 18.0;
const DEFAULT_TEXT_ID: &str = "default";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum RibbonTab {
    #[default]
    Media,
    Audio,
    Text,
    Sticker,
    Effects,
    Filters,
    Transitions,
    Subtitles,
    Transcript,
    Styles,
    AiClips,
    AiSequence,
    Curves,
    Mixer,
    Color,
}

const SIDEBAR_WIDTH: f32 = 112.0;

impl RibbonTab {
    const ALL: [RibbonTab; 15] = [
        Self::Media,
        Self::Audio,
        Self::Text,
        Self::Sticker,
        Self::Effects,
        Self::Filters,
        Self::Transitions,
        Self::Subtitles,
        Self::Transcript,
        Self::Styles,
        Self::AiClips,
        Self::AiSequence,
        Self::Curves,
        Self::Mixer,
        Self::Color,
    ];

    /// Opens an editor over the library instead of a list.
    pub(crate) fn is_editor(self) -> bool {
        matches!(self, Self::Curves | Self::Mixer | Self::Color)
    }

    fn label(self) -> Cow<'static, str> {
        match self {
            Self::Media => t!("ribbon.tab_media"),
            Self::Audio => t!("ribbon.tab_audio"),
            Self::Text => t!("ribbon.tab_text"),
            Self::Sticker => t!("ribbon.tab_sticker"),
            Self::Effects => t!("toolbar.effects"),
            Self::Filters => t!("fx.tab_filters"),
            Self::Transitions => t!("fx.tab_transitions"),
            Self::Subtitles => t!("ribbon.tab_subtitles"),
            Self::Transcript => t!("transcript.tab"),
            Self::Styles => t!("style_pack.tab"),
            Self::AiClips => "AI Clips".into(),
            Self::AiSequence => t!("ai_sequence.tab"),
            Self::Curves => t!("toolbar.keyframe_editor"),
            Self::Mixer => t!("toolbar.mixer"),
            Self::Color => t!("toolbar.color"),
        }
    }

    /// Favorites first.
    pub(crate) fn categories(self) -> Vec<Category> {
        let mut categories = vec![Category::Favorites];
        match self {
            Self::Media => categories.push(Category::Project),
            Self::Audio => categories.extend([Category::Music, Category::Sfx]),
            Self::Text => {
                categories.push(Category::AddText);
                categories.extend(TextCategory::ALL.map(Category::Template));
                categories.push(Category::Yours);
            }
            Self::Sticker => {
                categories.extend(StickerCategory::ALL.map(Category::Sticker));
            }
            Self::Effects
            | Self::Filters
            | Self::Transitions
            | Self::Subtitles
            | Self::Transcript
            | Self::Styles
            | Self::AiClips
            | Self::AiSequence
            | Self::Curves
            | Self::Mixer
            | Self::Color => {}
        }
        categories
    }

    /// Library folders holding what this tab shows.
    fn folders(self) -> Vec<String> {
        self.categories()
            .into_iter()
            .filter_map(Category::folder_id)
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum StickerCategory {
    Shapes,
    Emoji,
    Arrows,
    Social,
    /// Everything else of the icon sets.
    Other,
    /// The user's own images.
    Yours,
}

impl StickerCategory {
    pub(crate) const ALL: [StickerCategory; 6] = [
        Self::Shapes,
        Self::Emoji,
        Self::Arrows,
        Self::Social,
        Self::Other,
        Self::Yours,
    ];

    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::Shapes => "shapes",
            Self::Emoji => "emoji",
            Self::Arrows => "arrows",
            Self::Social => "social",
            Self::Other => "other",
            Self::Yours => "yours",
        }
    }

    fn label(self) -> Cow<'static, str> {
        match self {
            Self::Shapes => t!("ribbon.sticker_shapes"),
            Self::Emoji => t!("ribbon.sticker_emoji"),
            Self::Arrows => t!("ribbon.sticker_arrows"),
            Self::Social => t!("ribbon.sticker_social"),
            Self::Other => t!("ribbon.sticker_other"),
            Self::Yours => t!("ribbon.sticker_yours"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Category {
    Favorites,
    Project,
    Music,
    Sfx,
    AddText,
    Template(TextCategory),
    Yours,
    Sticker(StickerCategory),
}

impl Category {
    fn label(self) -> Cow<'static, str> {
        match self {
            Self::Favorites => t!("ribbon.favorites"),
            Self::Project => t!("ribbon.project_media"),
            Self::Music => t!("ribbon.audio_music"),
            Self::Sfx => t!("ribbon.audio_sfx"),
            Self::AddText => t!("ribbon.text_add"),
            Self::Template(category) => t!(format!("ribbon.text_cat_{}", category.id())),
            Self::Yours => t!("ribbon.text_yours"),
            Self::Sticker(category) => category.label(),
        }
    }

    /// The library folder holding the assets of the category.
    pub(crate) fn folder_id(self) -> Option<String> {
        match self {
            Self::Music => Some("ribbon-audio-music".to_owned()),
            Self::Sfx => Some("ribbon-audio-sfx".to_owned()),
            Self::Sticker(StickerCategory::Yours) => Some(YOURS_STICKER_FOLDER.to_owned()),
            _ => None,
        }
    }

    fn is_sticker(self) -> bool {
        matches!(self, Self::Sticker(_))
    }
}

/// Key of a favorite: it stays valid across projects.
pub(crate) fn media_key(path: &str) -> String {
    format!("media:{path}")
}

pub(crate) fn asset_key(id: &str) -> String {
    format!("asset:{id}")
}

pub(crate) fn text_key(id: &str) -> String {
    format!("text:{id}")
}

pub(crate) fn is_favorite(favorites: &[String], key: &str) -> bool {
    favorites.iter().any(|f| f == key)
}

pub(crate) fn toggle_favorite(favorites: &mut Vec<String>, key: &str) {
    if is_favorite(favorites, key) {
        favorites.retain(|f| f != key);
    } else {
        favorites.push(key.to_owned());
    }
}

/// A text style saved by the user from the text properties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextTemplate {
    pub id: String,
    pub name: String,
    pub title: vv_core::TitleParams,
}

/// Layers, frame size and texture of a card at rest.
type StaticPreview = (Vec<vv_core::TitleParams>, (u32, u32), egui::TextureHandle);
type SizedClips = ((u32, u32), Arc<Vec<TemplateClip>>);

#[derive(Default)]
pub(crate) struct RibbonState {
    pub tab: RibbonTab,
    selected: HashMap<RibbonTab, Category>,
    /// For the audio and sticker views (asset ids) and the favorite media
    /// (paths).
    pool: PoolState<String, String>,
    /// Whether each asset misses a file, checked once.
    offline: HashMap<String, bool>,
    /// What the favorite media outside this project hold, probed once.
    media_meta: HashMap<PathBuf, Option<vv_core::MediaMeta>>,
    /// The whole ribbon, as last drawn: where dropped files go.
    pub area: Option<egui::Rect>,
    /// A click on a sound starts its preview once the viewer has it.
    pub autoplay: bool,
    /// The template whose card is being dragged to the timeline.
    dragged_template: Option<String>,
    text_previews: HashMap<String, StaticPreview>,
    text_clips: HashMap<String, SizedClips>,
    text_search: String,
    /// The text card under the pointer and when the pointer arrived.
    text_hover: Option<(String, f64)>,
    text_hover_texture: Option<egui::TextureHandle>,
    /// Curves, Mixer or Color, drawn over the tab content; `tab` keeps the
    /// tab to go back to.
    pub editor: Option<RibbonTab>,
    pub editor_commands: Vec<crate::properties_panel::BoxedCommand>,
    pub editor_playhead: Option<vv_core::FrameIdx>,
    /// Scroll offset and its maximum of the tab strip, as last drawn.
    tab_strip: (f32, f32),
}

impl RibbonState {
    pub(crate) fn category(&self) -> Category {
        self.selected.get(&self.tab).copied().unwrap_or_else(|| {
            self.tab
                .categories()
                .get(1)
                .copied()
                .unwrap_or(Category::Favorites)
        })
    }

    pub(crate) fn show(&mut self, tab: RibbonTab, category: Category) {
        self.tab = tab;
        self.selected.insert(tab, category);
        self.editor = None;
    }

    /// The template of the text drag that just landed, if any.
    fn take_dragged_template(&mut self) -> Option<String> {
        self.dragged_template.take()
    }
}

struct AssetMenus;

enum RibbonAction {
    Remove,
}

impl PoolMenus<String, String> for AssetMenus {
    type Action = RibbonAction;

    fn item_menu(
        &self,
        ui: &mut egui::Ui,
        _entry: &PoolEntry<String, String>,
        _selected: usize,
        out: &mut Vec<MenuChoice<String, RibbonAction>>,
    ) {
        if ui.button(t!("ribbon.remove")).clicked() {
            out.push(MenuChoice::Action(RibbonAction::Remove));
            ui.close();
        }
    }
}

struct PathMenus;

impl PoolMenus<String, String> for PathMenus {
    type Action = ();

    fn item_menu(
        &self,
        _ui: &mut egui::Ui,
        _entry: &PoolEntry<String, String>,
        _selected: usize,
        _out: &mut Vec<MenuChoice<String, ()>>,
    ) {
    }
}

enum TextAction {
    Toggle(String),
    Insert(Option<String>),
    AsSubtitles(String),
    Delete(String),
}

enum CardSource {
    Bundled(&'static vv_core::TextTemplateDef),
    /// The default text or a template saved by the user: one layer.
    Plain(Box<vv_core::TitleParams>),
}

struct TextCard {
    id: String,
    name: String,
    user: bool,
    source: CardSource,
}

impl RibbonState {
    /// Layers and duration of a card, cached per frame size.
    fn card_clips(&mut self, card: &TextCard, size: (u32, u32)) -> (Arc<Vec<TemplateClip>>, f32) {
        match &card.source {
            CardSource::Plain(title) => (
                Arc::new(vec![TemplateClip {
                    title: (**title).clone(),
                    start_secs: 0.0,
                    length_secs: TEXT_SECS as f32,
                }]),
                TEXT_SECS as f32,
            ),
            CardSource::Bundled(def) => {
                let entry = self
                    .text_clips
                    .entry(card.id.clone())
                    .or_insert_with(|| (size, Arc::new(def.clips(size))));
                if entry.0 != size {
                    *entry = (size, Arc::new(def.clips(size)));
                }
                (entry.1.clone(), def.duration)
            }
        }
    }
}

/// The layers `time` seconds into a looping preview of a template lasting
/// `duration`: In, hold, Out, then a short pause.
fn hover_titles(clips: &[TemplateClip], duration: f32, time: f32) -> Vec<vv_core::TitleParams> {
    let t = time.rem_euclid(duration + 0.6).min(duration);
    clips
        .iter()
        .filter_map(|clip| {
            let local = t - clip.start_secs;
            if local < 0.0 {
                return None;
            }
            let mut title = clip.title.clone();
            title.anim_state = title.anim.state_at(
                &title.display_text(),
                local.min(clip.length_secs),
                clip.length_secs,
            );
            Some(title)
        })
        .collect()
}

/// The title as the compositor draws it, over a dark frame of the timeline's
/// proportions.
pub(crate) fn title_preview_image(
    title: &vv_core::TitleParams,
    timeline_size: (u32, u32),
) -> egui::ColorImage {
    layers_preview_image(std::slice::from_ref(title), timeline_size)
}

/// Several titles stacked, the first at the bottom.
pub(crate) fn layers_preview_image(
    titles: &[vv_core::TitleParams],
    timeline_size: (u32, u32),
) -> egui::ColorImage {
    let width = PREVIEW_TEXTURE_WIDTH;
    let height = (width as f32 * TEXT_PREVIEW.y / TEXT_PREVIEW.x).round() as u32;
    preview_image_in(titles, timeline_size, (width, height))
}

/// `layers_preview_image` on a texture of `size`.
pub(crate) fn preview_image_in(
    titles: &[vv_core::TitleParams],
    timeline_size: (u32, u32),
    (width, height): (u32, u32),
) -> egui::ColorImage {
    // The frame keeps the timeline's aspect, centred in the card: text
    // anchored to a frame edge would fall outside a 16:9 one.
    let scale = (width as f32 / timeline_size.0.max(1) as f32)
        .min(height as f32 / timeline_size.1.max(1) as f32);
    let frame = (
        ((timeline_size.0 as f32 * scale).round() as u32).clamp(1, width),
        ((timeline_size.1 as f32 * scale).round() as u32).clamp(1, height),
    );
    let (left, top) = (
        (width - frame.0) as usize / 2,
        (height - frame.1) as usize / 2,
    );
    let mut pixels = vec![[18.0f32, 18.0, 20.0]; (width * height) as usize];
    for row in top..top + frame.1 as usize {
        pixels[row * width as usize + left..][..frame.0 as usize].fill([34.0, 34.0, 38.0]);
    }
    for title in titles {
        let title = title.at(0);
        let render = vv_render::text::render_title(&title, timeline_size, frame);
        for (placed, color) in &render.masks() {
            let rgb = [color.r, color.g, color.b].map(|c| c.clamp(0.0, 1.0) * 255.0);
            let mask = &placed.mask;
            let (x0, y0) = (placed.x as usize + left, placed.y as usize + top);
            let cols = (mask.width as usize).min((width as usize).saturating_sub(x0));
            for row in 0..(mask.height as usize).min((height as usize).saturating_sub(y0)) {
                let src = &mask.data[row * mask.width as usize..][..cols];
                let dst = &mut pixels[(y0 + row) * width as usize + x0..][..cols];
                for (pixel, &coverage) in dst.iter_mut().zip(src) {
                    let alpha = f32::from(coverage) / 255.0 * color.a.clamp(0.0, 1.0);
                    for (dst, src) in pixel.iter_mut().zip(rgb) {
                        *dst += (src - *dst) * alpha;
                    }
                }
            }
        }
    }
    let rgba = pixels
        .iter()
        .flat_map(|p| [p[0] as u8, p[1] as u8, p[2] as u8, 255])
        .collect::<Vec<_>>();
    egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], &rgba)
}

const TAB_WIDTH: f32 = 54.0;
const TAB_HEIGHT: f32 = 40.0;
const TAB_STRIP_STEP: f32 = 3.0 * TAB_WIDTH;

/// «‹» or «›» beside the tab strip; dimmed at the end of the scroll.
fn tab_strip_arrow(ui: &mut egui::Ui, left: bool, enabled: bool) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(theme::ICON_SIZE, TAB_HEIGHT), sense);
    let color = if !enabled {
        theme::TEXT_DISABLED
    } else if response.hovered() {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_SECONDARY
    };
    if enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, theme::RADIUS_CONTROL, theme::BG_RAISED_HOVER);
    }
    let c = rect.center();
    let dx = if left { 3.0 } else { -3.0 };
    ui.painter().add(egui::Shape::line(
        vec![
            c + egui::vec2(dx, -6.0),
            c + egui::vec2(-dx, 0.0),
            c + egui::vec2(dx, 6.0),
        ],
        egui::Stroke::new(theme::ICON_STROKE, color),
    ));
    response
}

fn ribbon_tab_button(ui: &mut egui::Ui, tab: RibbonTab, selected: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(TAB_WIDTH, TAB_HEIGHT), egui::Sense::click());
    let color = if selected {
        theme::ACCENT
    } else if response.hovered() {
        egui::Color32::WHITE
    } else {
        theme::TEXT_SECONDARY
    };
    let center = egui::pos2(rect.center().x, rect.top() + 12.0);
    paint_tab_icon(ui.painter(), tab, center, color);
    ui.painter().text(
        egui::pos2(rect.center().x, rect.bottom() - 6.0),
        egui::Align2::CENTER_CENTER,
        tab.label(),
        egui::FontId::proportional(theme::FONT_SMALL),
        color,
    );
    response
}

/// 20 px line icons in the style of Lucide, drawn by hand.
fn paint_tab_icon(painter: &egui::Painter, tab: RibbonTab, c: egui::Pos2, color: egui::Color32) {
    let stroke = egui::Stroke::new(theme::ICON_STROKE, color);
    let v = |x: f32, y: f32| c + egui::vec2(x, y);
    let line = |a: egui::Pos2, b: egui::Pos2| painter.line_segment([a, b], stroke);
    let square = egui::Rect::from_center_size(c, egui::Vec2::splat(15.0));
    match tab {
        RibbonTab::Media => {
            painter.rect_stroke(square, 2.0, stroke, egui::StrokeKind::Middle);
            painter.add(egui::Shape::closed_line(
                vec![v(-2.0, -3.5), v(3.5, 0.0), v(-2.0, 3.5)],
                stroke,
            ));
        }
        RibbonTab::Audio => {
            painter.circle_stroke(v(-3.0, 4.5), 2.8, stroke);
            line(v(-0.2, 4.5), v(-0.2, -6.0));
            painter.add(egui::Shape::line(
                vec![v(-0.2, -6.0), v(2.5, -5.0), v(5.0, -2.5)],
                stroke,
            ));
        }
        RibbonTab::Text => {
            line(v(-5.5, -5.5), v(5.5, -5.5));
            line(v(0.0, -5.5), v(0.0, 6.0));
            line(v(-2.0, 6.0), v(2.0, 6.0));
        }
        RibbonTab::Sticker => {
            painter.circle_stroke(c, 7.5, stroke);
            painter.circle_filled(v(-2.6, -2.0), 0.9, color);
            painter.circle_filled(v(2.6, -2.0), 0.9, color);
            painter.add(egui::Shape::line(
                vec![v(-3.5, 2.0), v(-1.5, 3.8), v(1.5, 3.8), v(3.5, 2.0)],
                stroke,
            ));
        }
        RibbonTab::Effects => {
            painter.add(egui::Shape::closed_line(
                vec![
                    v(0.0, -7.5),
                    v(1.8, -1.8),
                    v(7.5, 0.0),
                    v(1.8, 1.8),
                    v(0.0, 7.5),
                    v(-1.8, 1.8),
                    v(-7.5, 0.0),
                    v(-1.8, -1.8),
                ],
                stroke,
            ));
        }
        RibbonTab::Filters => {
            painter.circle_stroke(v(-2.5, -1.5), 5.0, stroke);
            painter.circle_stroke(v(2.5, -1.5), 5.0, stroke);
            painter.circle_stroke(v(0.0, 2.5), 5.0, stroke);
        }
        RibbonTab::Transitions => {
            let a = egui::Rect::from_center_size(v(-3.5, 0.0), egui::vec2(8.0, 12.0));
            let b = egui::Rect::from_center_size(v(3.5, 0.0), egui::vec2(8.0, 12.0));
            painter.rect_stroke(a, 1.5, stroke, egui::StrokeKind::Inside);
            painter.rect_stroke(b, 1.5, stroke, egui::StrokeKind::Inside);
        }
        RibbonTab::Subtitles => {
            let body = egui::Rect::from_center_size(c, egui::vec2(15.0, 11.0));
            painter.rect_stroke(body, 2.0, stroke, egui::StrokeKind::Inside);
            line(v(-4.5, 1.5), v(-1.0, 1.5));
            line(v(1.0, 1.5), v(4.5, 1.5));
            line(v(-4.5, -1.5), v(2.5, -1.5));
        }
        RibbonTab::Transcript => {
            for (y, w) in [(-5.0, 14.0), (-1.5, 10.0), (2.0, 14.0), (5.5, 8.0)] {
                line(v(-7.0, y), v(-7.0 + w, y));
            }
        }
        RibbonTab::Styles => {
            painter.circle_stroke(v(-3.0, -3.0), 4.0, stroke);
            painter.rect_stroke(
                egui::Rect::from_center_size(v(3.0, 3.0), egui::vec2(8.0, 8.0)),
                1.5,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        RibbonTab::AiClips => {
            painter.rect_stroke(square, 2.0, stroke, egui::StrokeKind::Middle);
            line(v(-4.0, -4.0), v(4.0, 0.0));
            line(v(4.0, 0.0), v(-4.0, 4.0));
        }
        RibbonTab::AiSequence => {
            painter.rect_stroke(square, 2.0, stroke, egui::StrokeKind::Middle);
            line(v(-4.0, 4.0), v(-4.0, 0.0));
            line(v(0.0, 4.0), v(0.0, -4.0));
            line(v(4.0, 4.0), v(4.0, -1.5));
        }
        RibbonTab::Curves => {
            let points = (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    let eased = t * t * (3.0 - 2.0 * t);
                    v(-7.0 + 14.0 * t, 6.0 - 12.0 * eased)
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
            painter.circle_filled(v(-7.0, 6.0), 1.4, color);
            painter.circle_filled(v(7.0, -6.0), 1.4, color);
        }
        RibbonTab::Mixer => {
            for (x, knob) in [(-5.0, -2.0), (0.0, 3.0), (5.0, -4.0)] {
                line(v(x, -7.0), v(x, 7.0));
                painter.rect_filled(
                    egui::Rect::from_center_size(v(x, knob), egui::vec2(5.0, 3.0)),
                    1.0,
                    color,
                );
            }
        }
        RibbonTab::Color => {
            painter.circle_stroke(c, 7.5, stroke);
            for (x, y) in [(-3.0, -2.5), (2.0, -3.5), (3.8, 1.5), (-1.0, 3.0)] {
                painter.circle_filled(v(x, y), 1.1, color);
            }
        }
    }
}

pub(crate) fn paint_star(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    filled: bool,
    color: egui::Color32,
) {
    let points: Vec<egui::Pos2> = (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { radius } else { radius * 0.45 };
            let angle = std::f32::consts::TAU * i as f32 / 10.0 - std::f32::consts::FRAC_PI_2;
            center + egui::vec2(angle.cos(), angle.sin()) * r
        })
        .collect();
    if filled {
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(center, color);
        for point in &points {
            mesh.colored_vertex(*point, color);
        }
        for i in 0..10u32 {
            mesh.add_triangle(0, 1 + i, 1 + (i + 1) % 10);
        }
        painter.add(egui::Shape::mesh(mesh));
    }
    painter.add(egui::Shape::closed_line(
        points,
        egui::Stroke::new(1.2, color),
    ));
}

pub(crate) fn paint_plus(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(1.8, color);
    painter.line_segment(
        [
            center - egui::vec2(radius, 0.0),
            center + egui::vec2(radius, 0.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            center - egui::vec2(0.0, radius),
            center + egui::vec2(0.0, radius),
        ],
        stroke,
    );
}

/// The star and the "+" shown on an item while the pointer is on `rect`:
/// `(star clicked, plus clicked)`.
pub(crate) fn hover_buttons(
    ui: &mut egui::Ui,
    id: egui::Id,
    rect: egui::Rect,
    favorite: bool,
) -> (bool, bool) {
    let size = egui::vec2(HOVER_BUTTON, HOVER_BUTTON);
    let plus_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - size.x - 2.0, rect.top() + 2.0),
        size,
    );
    let star_rect = plus_rect.translate(egui::vec2(-size.x - 2.0, 0.0));
    let star = ui
        .interact(star_rect, id.with("star"), egui::Sense::click())
        .on_hover_text(if favorite {
            t!("ribbon.unfavorite")
        } else {
            t!("ribbon.favorite")
        });
    let plus = ui
        .interact(plus_rect, id.with("plus"), egui::Sense::click())
        .on_hover_text(t!("ribbon.add_to_timeline"));
    let background = |response: &egui::Response| {
        egui::Color32::from_black_alpha(if response.hovered() { 210 } else { 150 })
    };
    let painter = ui.painter();
    painter.rect_filled(star_rect, 3.0, background(&star));
    painter.rect_filled(plus_rect, 3.0, background(&plus));
    let gold = crate::theme::FAVORITE;
    paint_star(
        painter,
        star_rect.center(),
        size.x * 0.32,
        favorite,
        if favorite { gold } else { egui::Color32::WHITE },
    );
    paint_plus(
        painter,
        plus_rect.center(),
        size.x * 0.28,
        egui::Color32::WHITE,
    );
    (star.clicked(), plus.clicked())
}

/// Dashed box with a "+": `true` when it is clicked.
pub(crate) fn empty_state(ui: &mut egui::Ui, text: &str) -> bool {
    let size = egui::vec2(
        ui.available_width(),
        ui.available_height().clamp(90.0, 180.0),
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let color = if response.hovered() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let outline = rect.shrink(4.0);
    let corners = [
        outline.left_top(),
        outline.right_top(),
        outline.right_bottom(),
        outline.left_bottom(),
        outline.left_top(),
    ];
    let painter = ui.painter();
    painter.extend(egui::Shape::dashed_line(
        &corners,
        egui::Stroke::new(1.5, color),
        6.0,
        4.0,
    ));
    paint_plus(
        painter,
        egui::pos2(rect.center().x, rect.center().y - 18.0),
        12.0,
        theme::ACCENT,
    );
    let galley = painter.layout(
        text.to_owned(),
        egui::FontId::proportional(crate::theme::FONT_SECTION),
        color,
        (rect.width() - 24.0).max(40.0),
    );
    let pos = egui::pos2(
        rect.center().x - galley.size().x / 2.0,
        rect.center().y + 2.0,
    );
    painter.galley(pos, galley, color);
    response.clicked()
}

impl VenturiApp {
    /// The ribbon: tabs, categories and the content of the chosen one.
    pub(crate) fn show_ribbon(&mut self, ui: &mut egui::Ui, preview_action: &mut Option<Browsed>) {
        let area = ui.available_rect_before_wrap();
        self.library_ribbon.area = Some(area);
        if !egui::DragAndDrop::has_any_payload(ui.ctx()) {
            self.library_ribbon.dragged_template = None;
        }
        self.show_ribbon_tabs(ui);
        if let Some(editor) = self.library_ribbon.editor {
            self.show_ribbon_editor(ui, editor);
            ui.allocate_rect(area, egui::Sense::hover());
            return;
        }
        let tab = self.library_ribbon.tab;
        match tab {
            RibbonTab::Effects | RibbonTab::Filters | RibbonTab::Transitions => {
                let kind = match tab {
                    RibbonTab::Effects => vv_render::assets::AssetKind::Effect,
                    RibbonTab::Filters => vv_render::assets::AssetKind::Filter,
                    _ => vv_render::assets::AssetKind::Transition,
                };
                self.show_fx_tab(ui, kind);
                ui.allocate_rect(area, egui::Sense::hover());
                return;
            }
            RibbonTab::Subtitles => {
                self.show_subtitles_tab(ui);
                ui.allocate_rect(area, egui::Sense::hover());
                return;
            }
            RibbonTab::Transcript => {
                self.show_transcript_tab(ui);
                ui.allocate_rect(area, egui::Sense::hover());
                return;
            }
            RibbonTab::Styles => {
                self.show_style_packs_tab(ui);
                ui.allocate_rect(area, egui::Sense::hover());
                return;
            }
            RibbonTab::AiClips => {
                self.show_ai_clips_tab(ui);
                ui.allocate_rect(area, egui::Sense::hover());
                return;
            }
            RibbonTab::AiSequence => {
                self.show_ai_sequence_tab(ui);
                ui.allocate_rect(area, egui::Sense::hover());
                return;
            }
            _ => {}
        }
        let side_width = SIDEBAR_WIDTH.min(area.width() * 0.4);
        let rest = ui.available_rect_before_wrap();
        let side_rect = rest.with_max_x(rest.left() + side_width);
        let content_rect = rest.with_min_x(side_rect.right() + theme::SPACE_2);
        let mut side_ui = ui.new_child(egui::UiBuilder::new().max_rect(side_rect));
        let category = self.show_category_sidebar(&mut side_ui);
        let mut content_ui = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
        // The pool's background interaction spans the clip rect: inherited
        // unclipped, it would swallow clicks on the sidebar rows.
        content_ui.shrink_clip_rect(content_rect);
        let ui = &mut content_ui;
        if let Some(destination) = tab.download_destination(category) {
            self.show_download_bar(ui, destination);
        }
        match (tab, category) {
            (RibbonTab::Media, Category::Project) => self.show_media_pool(ui, preview_action),
            (RibbonTab::Media, _) => self.show_media_favorites(ui, preview_action),
            (RibbonTab::Text, category) => self.show_text_tab(ui, category),
            (RibbonTab::Sticker, category) => self.show_sticker_tab(ui, category),
            (_, category) => self.show_asset_category(ui, tab, category, preview_action),
        }
        ui.allocate_rect(area, egui::Sense::hover());
    }

    /// One row of tabs: when the block is too narrow it scrolls, with an
    /// arrow on each side.
    fn show_ribbon_tabs(&mut self, ui: &mut egui::Ui) {
        let importable = self.ribbon_import_visible() && self.library_ribbon.editor.is_none();
        let mut import = false;
        let mut clicked = None;
        let (offset, max_offset) = self.library_ribbon.tab_strip;
        let overflows = max_offset > 0.5;
        let mut scroll_to = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme::SPACE_1;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if importable {
                    import = ui.button(t!("ribbon.import")).clicked();
                }
                if overflows && tab_strip_arrow(ui, false, offset < max_offset - 0.5).clicked() {
                    scroll_to = Some(offset + TAB_STRIP_STEP);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if overflows && tab_strip_arrow(ui, true, offset > 0.5).clicked() {
                        scroll_to = Some(offset - TAB_STRIP_STEP);
                    }
                    let mut area = egui::ScrollArea::horizontal()
                        .id_salt("ribbon_tab_strip")
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                        );
                    if let Some(x) = scroll_to {
                        area = area.horizontal_scroll_offset(x.clamp(0.0, max_offset));
                    }
                    let output = area.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = theme::SPACE_1;
                            for tab in RibbonTab::ALL {
                                let selected = match self.library_ribbon.editor {
                                    Some(editor) => editor == tab,
                                    None => self.library_ribbon.tab == tab,
                                };
                                if ribbon_tab_button(ui, tab, selected).clicked() && !selected {
                                    clicked = Some(tab);
                                }
                            }
                        });
                    });
                    let max = (output.content_size.x - output.inner_rect.width()).max(0.0);
                    self.library_ribbon.tab_strip = (output.state.offset.x.min(max), max);
                });
            });
        });
        if let Some(tab) = clicked {
            self.select_ribbon_tab(tab);
        }
        if import {
            self.import_media_dialog();
        }
    }

    pub(crate) fn select_ribbon_tab(&mut self, tab: RibbonTab) {
        if tab.is_editor() {
            self.open_ribbon_editor(tab);
            return;
        }
        self.library_ribbon.editor = None;
        if self.library_ribbon.tab != tab {
            self.library_ribbon.tab = tab;
            self.library_ribbon.pool.clear();
            self.media_pool_state.clear();
            self.library_ui.pool.clear();
        }
    }

    fn ribbon_import_visible(&self) -> bool {
        let ribbon = &self.library_ribbon;
        matches!(
            ribbon.tab,
            RibbonTab::Media | RibbonTab::Audio | RibbonTab::Sticker
        ) && ribbon.category() != Category::Favorites
    }

    fn show_category_sidebar(&mut self, ui: &mut egui::Ui) -> Category {
        let tab = self.library_ribbon.tab;
        let current = self.library_ribbon.category();
        let mut chosen = current;
        let categories = tab.categories();
        let group = match tab {
            RibbonTab::Media => Some(t!("ribbon.group_media")),
            RibbonTab::Sticker => Some(t!("ribbon.group_stickers")),
            _ => None,
        };
        let mut row = |ui: &mut egui::Ui, category: Category| {
            let selected = current == category;
            let text = egui::RichText::new(category.label()).color(if selected {
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
                chosen = category;
            }
        };
        egui::ScrollArea::vertical()
            .id_salt("ribbon_sidebar")
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                row(ui, categories[0]);
                match group {
                    Some(title) => {
                        egui::CollapsingHeader::new(title)
                            .id_salt(("ribbon_group", tab as u8))
                            .default_open(true)
                            .show(ui, |ui| {
                                for &category in &categories[1..] {
                                    row(ui, category);
                                }
                            });
                    }
                    None => {
                        for &category in &categories[1..] {
                            row(ui, category);
                        }
                    }
                }
            });
        if chosen != current {
            self.library_ribbon.selected.insert(tab, chosen);
            self.library_ribbon.pool.clear();
        }
        chosen
    }

    /// Audio category that files imported now would join: the one shown, or
    /// the first of the tab while on Favorites. Stickers always go to
    /// Yours. `None` on the other tabs, or with the Library block closed.
    pub(crate) fn ribbon_import_category(&self) -> Option<Category> {
        let ribbon = &self.library_ribbon;
        if !self.block_shown(Block::Library)
            || !matches!(ribbon.tab, RibbonTab::Audio | RibbonTab::Sticker)
        {
            return None;
        }
        if ribbon.tab == RibbonTab::Sticker {
            return Some(Category::Sticker(StickerCategory::Yours));
        }
        let category = ribbon.category();
        if category.folder_id().is_some() {
            Some(category)
        } else {
            ribbon.tab.categories().get(1).copied()
        }
    }

    pub(crate) fn import_to_category_dialog(&mut self, category: Category) {
        let extensions: Vec<&str> = if category.is_sticker() {
            vec!["png", "webp", "svg", "gif", "webm"]
        } else {
            vv_media::AUDIO_EXTENSIONS.to_vec()
        };
        self.spawn_dialog(
            project_io::DialogKind::ImportToCategory(category),
            move |dlg| {
                project_io::DialogOutcome::Files(
                    dlg.add_filter("files", &project_io::case_insensitive(&extensions))
                        .pick_files(),
                )
            },
        );
    }

    /// Copies the files into the user library, in the category's folder.
    pub(crate) fn import_into_category(&mut self, paths: Vec<PathBuf>, category: Category) {
        let Some(folder) = category.folder_id() else {
            return;
        };
        let accepted: &[&str] = if category.is_sticker() {
            &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webm"]
        } else {
            vv_media::AUDIO_EXTENSIONS
        };
        let has_extension = |path: &Path, list: &[&str]| {
            path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| list.iter().any(|x| x.eq_ignore_ascii_case(e)))
        };
        let mut warnings = Vec::new();
        let mut files = Vec::new();
        for path in paths {
            if category.is_sticker() && !path.is_dir() && crate::stickers::needs_conversion(&path) {
                self.start_sticker_conversion(path);
            } else if path.is_dir() || has_extension(&path, accepted) {
                files.push(path);
            } else {
                warnings.push(
                    t!(
                        "ribbon.import_unsupported",
                        name = vv_session::file_label(&path)
                    )
                    .into_owned(),
                );
            }
        }
        self.import_warnings = warnings;
        if files.is_empty() {
            return;
        }
        if let Err(e) = self.library.ensure_folder(&folder, &category.label()) {
            self.project_error = Some(t!("library.save_failed", error = e).into_owned());
            return;
        }
        let tree = vv_session::MediaTree::scan(&files);
        let origin = self.clipboard_origin();
        if let Err(e) = self.library.import_tree(tree, Some(folder), origin) {
            self.project_error = Some(t!("library.save_failed", error = e).into_owned());
        }
    }

    pub(crate) fn toggle_favorite_key(&mut self, key: &str) {
        toggle_favorite(&mut self.settings.favorites, key);
        self.persist_settings();
    }

    /// The pool item of a file, imported if the project lacks it.
    fn pool_media_for_path(&mut self, path: &Path) -> Option<MediaId> {
        if let Some(id) = self.session.media_with_path(path) {
            return Some(id);
        }
        match self.add_media_to_pool(path.to_path_buf()) {
            Ok(id) => Some(id),
            Err(e) => {
                self.import_warnings = vec![e];
                None
            }
        }
    }

    /// Stickers and photos get their default length instead of the pool's.
    fn drag_set(&self, media: &[MediaId], image_secs: Option<f64>) -> timeline_ui::MediaDragSet {
        let items = media
            .iter()
            .filter_map(|&id| {
                let meta = &self.session.project.media_pool.get(id)?.meta;
                Some(match image_secs {
                    Some(secs) if meta.is_image() => {
                        timeline_ui::MediaDrag::image_secs(id, meta, secs)
                    }
                    Some(secs) if meta.has_video => {
                        timeline_ui::MediaDrag::capped_secs(id, meta, secs)
                    }
                    _ => timeline_ui::MediaDrag::whole(id, meta),
                })
            })
            .collect();
        timeline_ui::MediaDragSet { items }
    }

    /// Where an overlay (sticker, text) goes at `start`: a video track free
    /// there, else a new one; the Main Track only when nothing is on the
    /// timeline.
    fn overlay_target(&self, start: FrameIdx, len: FrameIdx) -> timeline_ui::MediaDropTarget {
        let Some(timeline) = self
            .timeline_id
            .and_then(|id| self.session.project.timelines.get(id))
        else {
            return timeline_ui::MediaDropTarget::Default;
        };
        if timeline.tracks.iter().all(|t| t.clips.is_empty()) {
            return timeline_ui::MediaDropTarget::Default;
        }
        let end = start + len;
        let free = timeline
            .tracks_of_kind(TrackKind::Video)
            .filter(|(_, track)| !track.locked && !track.main)
            .find(|(_, track)| {
                !track
                    .clips
                    .iter()
                    .any(|c| c.timeline_start < end && c.timeline_start + c.timeline_len > start)
            })
            .map(|(index, _)| index);
        match free {
            Some(index) => timeline_ui::MediaDropTarget::Track(index),
            None => timeline_ui::MediaDropTarget::NewVideoTrack,
        }
    }

    fn timeline_fps(&self) -> f64 {
        self.timeline_id
            .and_then(|id| self.session.project.timelines.get(id))
            .map_or(30.0, |t| t.fps.as_f64())
    }

    /// The "+" of an item: inserts at the playhead.
    fn insert_at_playhead(&mut self, media: &[MediaId], overlay: bool) {
        let start = self.timeline_state.playhead.max(0);
        let set = self.drag_set(media, overlay.then_some(STICKER_SECS));
        let target = if overlay {
            let len = (self.timeline_fps() * STICKER_SECS).round() as FrameIdx;
            self.overlay_target(start, len)
        } else {
            timeline_ui::MediaDropTarget::Default
        };
        self.add_media_set_to_timeline_at(&set, start, target);
    }

    pub(crate) fn insert_assets_at_playhead(&mut self, assets: &[String], sticker: bool) {
        let media: Vec<MediaId> = assets
            .iter()
            .flat_map(|id| self.import_library_asset(id))
            .collect();
        self.insert_at_playhead(&media, sticker);
    }

    /// The "+" of the pool's media.
    pub(crate) fn insert_pool_media_at_playhead(&mut self, media: MediaId) {
        self.insert_at_playhead(&[media], false);
    }

    /// Text at the playhead, with the template `template` if given.
    pub(crate) fn insert_text_at_playhead(&mut self, template: Option<&str>) {
        let start = self.timeline_state.playhead.max(0);
        let resolution = self.timeline_resolution();
        let (clips, _) = self.text_template_clips(template, resolution);
        let longest = clips
            .iter()
            .map(|c| c.start_secs + c.length_secs)
            .fold(0.0, f32::max);
        let len = (self.timeline_fps() * f64::from(longest)).round() as FrameIdx;
        let target = self.overlay_target(start, len);
        self.insert_text_template(template, start, target);
    }

    fn timeline_resolution(&self) -> (u32, u32) {
        self.timeline_id
            .and_then(|id| self.session.project.timelines.get(id))
            .map_or((1920, 1080), |t| t.resolution)
    }

    /// The clips of template `id` (bundled or the user's; the plain default
    /// text without it) for a frame of `resolution`, and its duration.
    fn text_template_clips(
        &self,
        id: Option<&str>,
        resolution: (u32, u32),
    ) -> (Vec<TemplateClip>, f32) {
        let plain = |title: vv_core::TitleParams| {
            (
                vec![TemplateClip {
                    title,
                    start_secs: 0.0,
                    length_secs: TEXT_SECS as f32,
                }],
                TEXT_SECS as f32,
            )
        };
        let Some(id) = id else {
            return plain(vv_core::TitleParams::default());
        };
        if let Some(def) = crate::text_templates::find(id) {
            return (def.clips(resolution), def.duration);
        }
        match self.settings.text_templates.iter().find(|t| t.id == id) {
            Some(user) => plain(user.title.clone()),
            None => plain(vv_core::TitleParams::default()),
        }
    }

    /// Puts the layers of a template on the timeline at `start`, the first on
    /// `target` and each next one on a new track above, in one undo step.
    pub(crate) fn insert_text_template(
        &mut self,
        template: Option<&str>,
        start: FrameIdx,
        target: timeline_ui::MediaDropTarget,
    ) {
        self.insert_text_template_with(template, start, target, None);
    }

    /// The template's duration in seconds, as `insert_text_template_with` uses it.
    pub(crate) fn text_template_secs(&self, template: &str, resolution: (u32, u32)) -> f32 {
        self.text_template_clips(Some(template), resolution).1
    }

    /// `insert_text_template` with `text` in place of the first layer's.
    pub(crate) fn insert_text_template_with(
        &mut self,
        template: Option<&str>,
        start: FrameIdx,
        target: timeline_ui::MediaDropTarget,
        text: Option<&str>,
    ) {
        let group = self.session.history.begin_group();
        let timeline_id = self.ensure_timeline();
        let timeline = &self.session.project.timelines[timeline_id];
        let (fps, resolution) = (timeline.fps, timeline.resolution);
        let (clips, _) = self.text_template_clips(template, resolution);
        for (n, mut clip) in clips.into_iter().enumerate() {
            if n == 0
                && let Some(text) = text.filter(|t| !t.trim().is_empty())
            {
                clip.title.content = vv_core::Keyframed::constant(text.to_owned());
            }
            crate::text_templates::fit_title_to_frame(&mut clip.title, resolution.0);
            let target = if n == 0 {
                target
            } else {
                timeline_ui::MediaDropTarget::NewVideoTrack
            };
            let Some(tracks) = self.resolve_drop_tracks(timeline_id, target, true, false) else {
                continue;
            };
            let frames = |secs: f32| (fps.as_f64() * f64::from(secs)).round() as FrameIdx;
            vv_core::edit::insert_title_len(
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
                clip.title,
                tracks.video.expect("text: video track always resolved"),
                start + frames(clip.start_secs),
                frames(clip.length_secs).max(1),
            );
        }
        self.session
            .history
            .end_group_as(group, vv_core::CommandLabel::InsertClips);
    }

    /// The style of a text clip becomes a template of the user's.
    pub(crate) fn save_text_template(&mut self, title: vv_core::TitleParams) {
        let number = self.settings.text_templates.len() + 1;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        self.settings.text_templates.push(TextTemplate {
            id: format!("{nanos:x}"),
            name: t!("ribbon.template_name", number = number).into_owned(),
            title,
        });
        self.persist_settings();
    }

    /// The cards of `category`; a search covers every template.
    fn text_cards(&self, category: Category) -> Vec<TextCard> {
        let query = self.library_ribbon.text_search.trim();
        let favorites = &self.settings.favorites;
        let bundled = |def: &'static vv_core::TextTemplateDef| TextCard {
            id: def.id.clone(),
            name: def.name.clone(),
            user: false,
            source: CardSource::Bundled(def),
        };
        let users = self.settings.text_templates.iter().map(|t| TextCard {
            id: t.id.clone(),
            name: t.name.clone(),
            user: true,
            source: CardSource::Plain(Box::new(t.title.clone())),
        });
        let default_card = TextCard {
            id: DEFAULT_TEXT_ID.to_owned(),
            name: t!("ribbon.text_default").into_owned(),
            user: false,
            source: CardSource::Plain(Box::default()),
        };
        if !query.is_empty() {
            let label = |c: TextCategory| t!(format!("ribbon.text_cat_{}", c.id())).into_owned();
            let lowered = query.to_lowercase();
            return crate::text_templates::all()
                .iter()
                .filter(|d| crate::text_templates::matches(d, &label(d.category), query))
                .map(bundled)
                .chain(users.filter(|c| c.name.to_lowercase().contains(&lowered)))
                .collect();
        }
        match category {
            Category::AddText => std::iter::once(default_card)
                .chain(crate::text_templates::all().iter().map(bundled))
                .collect(),
            Category::Template(c) => crate::text_templates::in_category(c).map(bundled).collect(),
            Category::Yours => users.collect(),
            _ => std::iter::once(default_card)
                .chain(crate::text_templates::all().iter().map(bundled))
                .chain(users)
                .filter(|c| is_favorite(favorites, &text_key(&c.id)))
                .collect(),
        }
    }

    fn show_text_tab(&mut self, ui: &mut egui::Ui, category: Category) {
        ui.add(
            egui::TextEdit::singleline(&mut self.library_ribbon.text_search)
                .hint_text(t!("ribbon.text_search"))
                .desired_width(ui.available_width()),
        );
        ui.add_space(theme::SPACE_1);
        let cards = self.text_cards(category);
        if cards.is_empty() {
            let hint = if !self.library_ribbon.text_search.trim().is_empty() {
                t!("ribbon.text_no_match")
            } else if category == Category::Yours {
                t!("ribbon.templates_empty")
            } else {
                t!("ribbon.favorites_empty")
            };
            ui.weak(hint);
            return;
        }
        let favorite_flags: Vec<bool> = cards
            .iter()
            .map(|c| is_favorite(&self.settings.favorites, &text_key(&c.id)))
            .collect();
        let timeline_size = self.timeline_resolution();
        let now = ui.input(|i| i.time);
        let mut actions = Vec::new();
        let mut hovered_card = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (card, favorite) in cards.iter().zip(&favorite_flags) {
                        let (clips, duration) = self.library_ribbon.card_clips(card, timeline_size);
                        let hover_time = match &self.library_ribbon.text_hover {
                            Some((id, since)) if *id == card.id => Some((now - since) as f32),
                            _ => None,
                        };
                        let texture = match hover_time {
                            Some(time) => {
                                let titles = hover_titles(&clips, duration, time);
                                let image = layers_preview_image(&titles, timeline_size);
                                let handle = self.library_ribbon.text_hover_texture.insert(
                                    ui.ctx().load_texture(
                                        "text-hover",
                                        image,
                                        egui::TextureOptions::LINEAR,
                                    ),
                                );
                                Some(handle.id())
                            }
                            None => self.static_preview(ui.ctx(), card, &clips, timeline_size),
                        };
                        let hovered = text_card(
                            ui,
                            card,
                            duration,
                            texture,
                            *favorite,
                            &mut self.library_ribbon.dragged_template,
                            &mut actions,
                        );
                        if hovered {
                            hovered_card = Some(card.id.clone());
                        }
                    }
                });
            });
        self.update_text_hover(ui.ctx(), hovered_card, now);
        for action in actions {
            match action {
                TextAction::Toggle(id) => self.toggle_favorite_key(&text_key(&id)),
                TextAction::Insert(template) => self.insert_text_at_playhead(template.as_deref()),
                TextAction::AsSubtitles(id) => {
                    let id = format!("{}{id}", crate::text_templates::SUBTITLE_PREFIX);
                    self.choose_subtitle_template(&id);
                }
                TextAction::Delete(id) => {
                    self.settings.text_templates.retain(|t| t.id != id);
                    self.settings.favorites.retain(|f| *f != text_key(&id));
                    self.persist_settings();
                }
            }
        }
    }

    /// The card at rest: every layer as it looks once its In has played.
    fn static_preview(
        &mut self,
        ctx: &egui::Context,
        card: &TextCard,
        clips: &[TemplateClip],
        timeline_size: (u32, u32),
    ) -> Option<egui::TextureId> {
        let titles: Vec<vv_core::TitleParams> = clips.iter().map(|c| c.title.clone()).collect();
        let stale = self
            .library_ribbon
            .text_previews
            .get(&card.id)
            .is_none_or(|(old, size, _)| *old != titles || *size != timeline_size);
        if stale {
            let image = layers_preview_image(&titles, timeline_size);
            let texture = ctx.load_texture(
                format!("text-preview-{}", card.id),
                image,
                egui::TextureOptions::LINEAR,
            );
            self.library_ribbon
                .text_previews
                .insert(card.id.clone(), (titles, timeline_size, texture));
        }
        self.library_ribbon
            .text_previews
            .get(&card.id)
            .map(|(_, _, texture)| texture.id())
    }

    fn update_text_hover(&mut self, ctx: &egui::Context, card: Option<String>, now: f64) {
        let ribbon = &mut self.library_ribbon;
        match card {
            Some(id) => {
                if ribbon.text_hover.as_ref().is_none_or(|(old, _)| *old != id) {
                    ribbon.text_hover = Some((id, now));
                }
                ctx.request_repaint();
            }
            None => ribbon.text_hover = None,
        }
    }

    /// Audio items of the user library in `category`, or the starred ones of
    /// the tab.
    fn show_asset_category(
        &mut self,
        ui: &mut egui::Ui,
        tab: RibbonTab,
        category: Category,
        preview_action: &mut Option<Browsed>,
    ) {
        let favorites_only = category == Category::Favorites;
        let folders = if favorites_only {
            tab.folders()
        } else {
            category.folder_id().into_iter().collect()
        };
        for name in self.library.pending() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(t!("library.copying", name = name));
            });
        }
        if self.library.pending().next().is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
        let entries = self.ribbon_asset_entries(&folders, favorites_only);
        let empty_hint = match (favorites_only, tab) {
            (true, _) => t!("ribbon.favorites_empty"),
            _ => t!("ribbon.audio_empty"),
        };
        let events = pool_view::show_pool(
            ui,
            &mut self.library_ribbon.pool,
            PoolView {
                folders: Vec::new(),
                entries,
                menus: &AssetMenus,
                kinetic_scroll: self.settings.kinetic_scroll_media_pool,
                layout: &mut self.settings.panels.library_layout,
                item_hint: t!("ribbon.item_hint"),
                empty_hint: Some(empty_hint),
            },
        );
        for event in events {
            match event {
                PoolEvent::Clicked(id) if tab == RibbonTab::Audio => {
                    *preview_action = Some(Browsed::Asset(id));
                    self.library_ribbon.autoplay = true;
                }
                PoolEvent::Open(id) => *preview_action = Some(Browsed::Asset(id)),
                PoolEvent::ToggleFavorite(id) => self.toggle_favorite_key(&asset_key(&id)),
                PoolEvent::Insert(id) => self.insert_assets_at_playhead(&[id], false),
                PoolEvent::Action(RibbonAction::Remove) => {
                    let ids: Vec<String> =
                        self.library_ribbon.pool.selected.iter().cloned().collect();
                    self.remove_ribbon_assets(&ids);
                }
                _ => {}
            }
        }
        self.import_dragged_ribbon_assets(ui.ctx(), false);
        let library = &self.library;
        self.library_ribbon
            .pool
            .forget(|id| library.assets.iter().any(|a| a.id == *id), |_| false);
    }

    pub(crate) fn remove_ribbon_assets(&mut self, ids: &[String]) {
        for id in ids {
            if matches!(&self.browsing, Some(Browsed::Asset(b)) if b == id) {
                self.stop_browsing();
            }
            if let Err(e) = self.library.remove_asset(id) {
                self.project_error = Some(t!("library.save_failed", error = e).into_owned());
            }
            self.settings.favorites.retain(|f| *f != asset_key(id));
        }
        self.persist_settings();
    }

    pub(crate) fn ribbon_asset_entries(
        &mut self,
        folders: &[String],
        favorites_only: bool,
    ) -> Vec<PoolEntry<String, String>> {
        let mut entries = Vec::new();
        for asset in &self.library.assets {
            if !asset.folder.as_ref().is_some_and(|f| folders.contains(f)) {
                continue;
            }
            let favorite = is_favorite(&self.settings.favorites, &asset_key(&asset.id));
            if favorites_only && !favorite {
                continue;
            }
            let Some(item) = asset.item() else {
                continue;
            };
            let offline = *self
                .library_ribbon
                .offline
                .entry(asset.id.clone())
                .or_insert_with(|| asset.is_offline());
            if item.compound.is_none()
                && item.meta.has_video
                && !offline
                && !self.thumbnails.contains_key(&item.content_hash)
            {
                self.thumbnails.insert(item.content_hash, None);
                let seconds = if item.meta.is_image() {
                    0.0
                } else {
                    item.meta.duration_frames as f64 / item.meta.fps.as_f64()
                };
                self.thumbnail_worker
                    .get_or_insert_with(thumbnail_worker::ThumbnailWorker::spawn)
                    .enqueue(item.path.clone(), item.content_hash, seconds);
            }
            let thumb = if offline {
                Thumb::Offline
            } else {
                match self.thumbnails.get(&item.content_hash).cloned().flatten() {
                    Some(texture) => Thumb::Texture(texture),
                    None => Thumb::None,
                }
            };
            entries.push(PoolEntry {
                id: asset.id.clone(),
                name: asset.name.clone(),
                folder: None,
                meta: item.meta.clone(),
                thumb,
                proxy: None,
                renamable: false,
                hover: Some(HoverActions { favorite }),
                added: false,
            });
        }
        entries
    }

    /// Assets dragged out of the ribbon become pool items, so that the
    /// timeline can take them.
    pub(crate) fn import_dragged_ribbon_assets(&mut self, ctx: &egui::Context, sticker: bool) {
        let Some(drag) = egui::DragAndDrop::payload::<pool_view::ItemsDrag<String>>(ctx) else {
            return;
        };
        let inside = self
            .library_ribbon
            .area
            .is_some_and(|area| timeline_ui::pointer_over(ctx, area));
        if inside {
            return;
        }
        let mut assets = Vec::with_capacity(drag.0.len());
        for id in &drag.0 {
            match self.sticker_asset_id(id) {
                Some(asset) => assets.push(asset),
                None => {
                    ctx.request_repaint();
                    return;
                }
            }
        }
        let media: Vec<MediaId> = assets
            .iter()
            .flat_map(|id| self.import_library_asset(id))
            .collect();
        let set = self.drag_set(&media, sticker.then_some(STICKER_SECS));
        egui::DragAndDrop::set_payload(ctx, set);
    }

    /// Favorite media: files of any project, starred by the user.
    fn show_media_favorites(&mut self, ui: &mut egui::Ui, preview_action: &mut Option<Browsed>) {
        let paths: Vec<String> = self
            .settings
            .favorites
            .iter()
            .filter_map(|k| k.strip_prefix("media:").map(str::to_owned))
            .collect();
        let mut entries = Vec::new();
        for path in &paths {
            let path_buf = PathBuf::from(path);
            let pooled = self
                .session
                .project
                .media_pool
                .values()
                .find(|item| item.compound.is_none() && item.path == path_buf);
            let (meta, thumb) = match pooled {
                Some(item) => {
                    let thumb = match self.thumbnails.get(&item.content_hash).cloned().flatten() {
                        Some(texture) => Thumb::Texture(texture),
                        None => Thumb::None,
                    };
                    (item.meta.clone(), thumb)
                }
                None => {
                    let probed = self
                        .library_ribbon
                        .media_meta
                        .entry(path_buf.clone())
                        .or_insert_with(|| vv_media::probe_media(&path_buf).ok());
                    match probed {
                        Some(meta) => (meta.clone(), Thumb::None),
                        None => (missing_meta(), Thumb::Offline),
                    }
                }
            };
            entries.push(PoolEntry {
                id: path.clone(),
                name: vv_session::file_label(&path_buf),
                folder: None,
                meta,
                thumb,
                proxy: None,
                renamable: false,
                hover: Some(HoverActions { favorite: true }),
                added: false,
            });
        }
        let events = pool_view::show_pool(
            ui,
            &mut self.library_ribbon.pool,
            PoolView {
                folders: Vec::new(),
                entries,
                menus: &PathMenus,
                kinetic_scroll: self.settings.kinetic_scroll_media_pool,
                layout: &mut self.settings.panels.media_pool_layout,
                item_hint: t!("ribbon.item_hint"),
                empty_hint: Some(t!("ribbon.favorites_empty")),
            },
        );
        for event in events {
            match event {
                PoolEvent::ToggleFavorite(path) => self.toggle_favorite_key(&media_key(&path)),
                PoolEvent::Insert(path) => {
                    if let Some(media) = self.pool_media_for_path(Path::new(&path)) {
                        self.insert_at_playhead(&[media], false);
                    }
                }
                PoolEvent::Open(path) => {
                    if let Some(media) = self.pool_media_for_path(Path::new(&path)) {
                        *preview_action = Some(Browsed::Media(media));
                    }
                }
                PoolEvent::Clicked(path) => {
                    if let Some(media) = self.pool_media_for_path(Path::new(&path)) {
                        *preview_action = Some(Browsed::Media(media));
                        self.library_ribbon.autoplay = true;
                    }
                }
                _ => {}
            }
        }
        self.import_dragged_ribbon_paths(ui.ctx());
    }

    fn import_dragged_ribbon_paths(&mut self, ctx: &egui::Context) {
        let Some(drag) = egui::DragAndDrop::payload::<pool_view::ItemsDrag<String>>(ctx) else {
            return;
        };
        let inside = self
            .library_ribbon
            .area
            .is_some_and(|area| timeline_ui::pointer_over(ctx, area));
        if inside {
            return;
        }
        let media: Vec<MediaId> = drag
            .0
            .iter()
            .filter_map(|path| self.pool_media_for_path(Path::new(path)))
            .collect();
        let set = self.drag_set(&media, None);
        egui::DragAndDrop::set_payload(ctx, set);
    }

    /// Carries out what the text drop on the timeline was dragging.
    pub(crate) fn dropped_text_template(&mut self) -> Option<String> {
        self.library_ribbon.take_dragged_template()
    }
}

fn missing_meta() -> vv_core::MediaMeta {
    vv_core::MediaMeta {
        duration_frames: 0,
        fps: vv_core::Rational::new(25, 1),
        width: 0,
        height: 0,
        has_video: false,
        has_audio: false,
        sample_rate: 0,
        channels: 0,
        audio_streams: 0,
        file: Default::default(),
    }
}

fn text_card(
    ui: &mut egui::Ui,
    card: &TextCard,
    duration: f32,
    preview: Option<egui::TextureId>,
    favorite: bool,
    dragged_template: &mut Option<String>,
    actions: &mut Vec<TextAction>,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(TEXT_CARD, egui::Sense::click_and_drag());
    let visuals = ui.visuals();
    let fill = if response.hovered() {
        visuals.widgets.hovered.bg_fill
    } else {
        visuals.widgets.inactive.bg_fill
    };
    let weak_color = visuals.weak_text_color();
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, fill);
    let preview_rect = egui::Rect::from_min_size(rect.min, TEXT_PREVIEW);
    if let Some(texture) = preview {
        painter.image(
            texture,
            preview_rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
    painter.text(
        egui::pos2(rect.center().x, rect.bottom() - 4.0),
        egui::Align2::CENTER_BOTTOM,
        &card.name,
        egui::FontId::proportional(crate::theme::FONT_SMALL),
        weak_color,
    );
    if !response.hovered() {
        painter.text(
            preview_rect.right_top() + egui::vec2(-4.0, 3.0),
            egui::Align2::RIGHT_TOP,
            format!("{duration:.0}s"),
            egui::FontId::proportional(crate::theme::FONT_TINY),
            egui::Color32::from_white_alpha(170),
        );
    }
    let template = (card.id != DEFAULT_TEXT_ID).then(|| card.id.clone());
    if response.drag_started() {
        egui::DragAndDrop::set_payload(ui.ctx(), timeline_ui::Generator::Text);
        *dragged_template = template.clone();
    }
    if response.dragged() {
        media_pool_ui::show_drag_ghost(ui, response.id, &card.name);
    }
    if template.is_some() {
        response.context_menu(|ui| {
            if ui.button(t!("ribbon.use_as_subtitles")).clicked() {
                actions.push(TextAction::AsSubtitles(card.id.clone()));
                ui.close();
            }
            if card.user && ui.button(t!("ribbon.remove")).clicked() {
                actions.push(TextAction::Delete(card.id.clone()));
                ui.close();
            }
        });
    }
    if ui.rect_contains_pointer(rect) && !response.dragged() {
        let (star, plus) = hover_buttons(ui, response.id, rect, favorite);
        if star {
            actions.push(TextAction::Toggle(card.id.clone()));
        }
        if plus || (response.clicked() && !star) {
            actions.push(TextAction::Insert(template));
        }
    }
    response.hovered()
}

#[cfg(test)]
#[path = "tests/library_ribbon.rs"]
mod tests;
