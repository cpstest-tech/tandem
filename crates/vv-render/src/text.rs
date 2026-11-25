//! Title rasterization (`ClipSource::Text`) via `cosmic-text`. Each element
//! (background, outline, text) is a single color: an 8-bit coverage
//! mask per element is enough, which the compositor colors and stacks
//! (`LayerContent::Text`). Masks cover only the area they draw on, so
//! their cost follows the size of the title rather than of the frame.

use crate::text_anim_render::Part;
use crate::text_fx;
use cosmic_text::{
    Align, Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, Style, SwashCache,
    UnderlineStyle, Weight,
};
use rayon::prelude::*;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use vv_core::{BackgroundShape, HAnchor, Rgba, TextAlign, TextFill, TitleParams, VAnchor};

/// Text coverage (0 = transparent).
#[derive(Clone)]
pub struct TextMask {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl TextMask {
    pub fn empty(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; (width * height) as usize],
        }
    }
}

/// A mask covering part of the output frame, its top-left at (`x`, `y`).
/// Outside it the coverage is 0. Shared: the compositor keeps its texture
/// for as long as the mask lives.
#[derive(Clone)]
pub struct PlacedMask {
    pub x: u32,
    pub y: u32,
    pub mask: Arc<TextMask>,
}

impl PlacedMask {
    pub(crate) fn nothing() -> Self {
        Self {
            x: 0,
            y: 0,
            mask: Arc::new(TextMask::empty(1, 1)),
        }
    }

    pub(crate) fn is_nothing(&self) -> bool {
        self.mask.data.len() == 1 && self.mask.data[0] == 0
    }

    /// `mask` with its top-left at `origin` (frame pixels, may be outside),
    /// cut to the frame and to its covered pixels plus a blank border, so a
    /// bilinear sampler sees the same edges as on a full-frame mask.
    pub(crate) fn place(mask: &TextMask, origin: (i32, i32), frame: (u32, u32)) -> Self {
        let Some(b) = text_fx::bounds(mask) else {
            return Self::nothing();
        };
        let x0 = (origin.0 + b.x0 as i32 - 1).max(0);
        let y0 = (origin.1 + b.y0 as i32 - 1).max(0);
        let x1 = (origin.0 + b.x1 as i32 + 1).min(frame.0 as i32);
        let y1 = (origin.1 + b.y1 as i32 + 1).min(frame.1 as i32);
        if x1 <= x0 || y1 <= y0 {
            return Self::nothing();
        }
        let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
        let mut out = TextMask::empty(w, h);
        let stride = mask.width as i32;
        for row in 0..h as i32 {
            let sy = y0 + row - origin.1;
            if sy < 0 || sy >= mask.height as i32 {
                continue;
            }
            let sx0 = (x0 - origin.0).max(0);
            let sx1 = (x1 - origin.0).min(stride);
            if sx1 <= sx0 {
                continue;
            }
            let dx = (sx0 + origin.0 - x0) as usize;
            let src = &mask.data[(sy * stride + sx0) as usize..(sy * stride + sx1) as usize];
            let dst_row = row as usize * w as usize;
            out.data[dst_row + dx..dst_row + dx + src.len()].copy_from_slice(src);
        }
        Self {
            x: x0 as u32,
            y: y0 as u32,
            mask: Arc::new(out),
        }
    }

    /// Coverage at frame pixel (`x`, `y`).
    pub fn at(&self, x: u32, y: u32) -> u8 {
        if x < self.x || y < self.y {
            return 0;
        }
        let (lx, ly) = (x - self.x, y - self.y);
        if lx >= self.mask.width || ly >= self.mask.height {
            return 0;
        }
        self.mask.data[(ly * self.mask.width + lx) as usize]
    }
}

/// The layers of a title from bottom to top, over an output frame of `size`.
pub struct TitleRender {
    pub size: (u32, u32),
    pub(crate) layers: Vec<TitleLayer>,
}

/// One color of a title (opacity already in its alpha) and where it goes.
pub(crate) enum TitleLayer {
    Mask(PlacedMask, Rgba),
    /// Sprites of an animated title, which the compositor moves on the GPU.
    Parts(Vec<Part>, Rgba),
}

#[cfg(test)]
thread_local! {
    /// Makes the compositor take the layers of animated titles from
    /// `TitleRender::masks` instead of drawing them itself.
    pub(crate) static CPU_PARTS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl TitleRender {
    /// The coverage mask and color of each layer, drawn on the CPU: for
    /// whoever shows a title without the compositor.
    pub fn masks(&self) -> Vec<(PlacedMask, Rgba)> {
        self.layers
            .iter()
            .map(|layer| match layer {
                TitleLayer::Mask(mask, color) => (mask.clone(), *color),
                TitleLayer::Parts(parts, color) => (
                    crate::text_anim_render::draw_parts(parts, self.size),
                    *color,
                ),
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFace {
    pub weight: u16,
    pub italic: bool,
    pub name: String,
}

type CacheKey = (TitleParams, (u32, u32), (u32, u32));

struct TextState {
    font_system: FontSystem,
    swash: SwashCache,
}

/// Most recent last.
type RenderCache = Mutex<Vec<(CacheKey, Arc<TitleRender>)>>;

pub(crate) fn render_cache() -> &'static RenderCache {
    static CACHE: OnceLock<RenderCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

fn animated_render_cache() -> &'static RenderCache {
    static CACHE: OnceLock<RenderCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Vec::new()))
}

const CACHE_LEN: usize = 16;

fn state() -> &'static Mutex<TextState> {
    static STATE: OnceLock<Mutex<TextState>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(TextState {
            font_system: FontSystem::new(),
            swash: SwashCache::new(),
        })
    })
}

fn lock() -> std::sync::MutexGuard<'static, TextState> {
    state().lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs `f` with the font system; only shaping and glyph rasterization
/// need it, so the mask work after it does not hold other renders back.
pub(crate) fn with_fonts<R>(f: impl FnOnce(&mut FontSystem, &mut SwashCache) -> R) -> R {
    let mut state = lock();
    let TextState { font_system, swash } = &mut *state;
    f(font_system, swash)
}

/// Scanning the system fonts takes a while: doing it ahead of time
/// avoids a stall on the first title.
pub fn warm_up() {
    let _ = state();
}

/// Adds a font file (TTF/OTF bytes) to the families titles can use.
pub fn register_font(data: Vec<u8>) {
    lock().font_system.db_mut().load_font_data(data);
}

pub fn font_families() -> Vec<String> {
    let state = lock();
    let mut families: Vec<String> = state
        .font_system
        .db()
        .faces()
        .filter_map(|f| f.families.first().map(|(name, _)| name.clone()))
        .collect();
    families.sort_by_key(|f| f.to_lowercase());
    families.dedup();
    families
}

pub fn font_faces(family: &str) -> Vec<FontFace> {
    let state = lock();
    let mut faces: Vec<FontFace> = state
        .font_system
        .db()
        .faces()
        .filter(|f| f.families.iter().any(|(name, _)| name == family))
        .map(|f| {
            let italic = f.style != cosmic_text::fontdb::Style::Normal;
            FontFace {
                weight: f.weight.0,
                italic,
                name: face_name(f.weight.0, italic),
            }
        })
        .collect();
    faces.sort_by_key(|f| (f.italic, f.weight));
    faces.dedup_by_key(|f| (f.italic, f.weight));
    faces
}

pub fn face_name(weight: u16, italic: bool) -> String {
    let base = match weight {
        0..=149 => "Thin",
        150..=249 => "ExtraLight",
        250..=349 => "Light",
        350..=449 => "Regular",
        450..=549 => "Medium",
        550..=649 => "SemiBold",
        650..=749 => "Bold",
        750..=849 => "ExtraBold",
        _ => "Black",
    };
    match (italic, base) {
        (true, "Regular") => "Italic".into(),
        (true, _) => format!("{base} Italic"),
        (false, _) => base.into(),
    }
}

/// Title masks for an output frame of `output_size`; the measures in
/// `params` are in pixels of a timeline of `timeline_size`.
pub fn render_title(
    params: &TitleParams,
    timeline_size: (u32, u32),
    output_size: (u32, u32),
) -> Arc<TitleRender> {
    let key: CacheKey = (params.clone(), timeline_size, output_size);
    // Per-frame animation entries must not evict prewarmed still titles.
    let renders = if params.anim_state.is_some() {
        animated_render_cache()
    } else {
        render_cache()
    };
    {
        let mut cache = renders.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pos) = cache.iter().position(|(k, _)| *k == key) {
            let entry = cache.remove(pos);
            let render = entry.1.clone();
            cache.push(entry);
            return render;
        }
    }
    let mut render = if params.anim_state.is_some() {
        crate::text_anim_render::rasterize(params, timeline_size, output_size)
    } else {
        rasterize(params, timeline_size, output_size)
    };
    crate::text_sticker::append(&mut render, params, timeline_size, output_size);
    let render = Arc::new(render);
    let mut cache = renders.lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() >= CACHE_LEN {
        cache.remove(0);
    }
    cache.push((key, render.clone()));
    render
}

type Prewarm = (TitleParams, (u32, u32), (u32, u32));

#[derive(Default)]
struct PrewarmQueue {
    pending: VecDeque<Prewarm>,
    active: Option<Prewarm>,
}

impl PrewarmQueue {
    fn push(&mut self, mut request: Prewarm) {
        if request.0.anim_state.is_some() {
            request.0.anim_state = Some(Default::default());
        }
        if self.active.as_ref() == Some(&request) || self.pending.contains(&request) {
            return;
        }
        if self.pending.len() >= CACHE_LEN {
            self.pending.pop_front();
        }
        self.pending.push_back(request);
    }
}

/// Prepares in the background what the first frame of a title costs more
/// than the others for (glyphs, outlines, glows), so that frame is as cheap
/// when playback reaches it. `params` as `render_title` will get them.
pub fn prewarm(params: &TitleParams, timeline_size: (u32, u32), output_size: (u32, u32)) {
    prewarm_one(params, timeline_size, output_size);
    if !params.anim.is_empty() {
        let mut other = params.clone();
        other.anim_state = if params.anim_state.is_some() {
            None
        } else {
            Some(Default::default())
        };
        prewarm_one(&other, timeline_size, output_size);
    }
}

fn prewarm_one(params: &TitleParams, timeline_size: (u32, u32), output_size: (u32, u32)) {
    let ready = if params.anim_state.is_some() {
        crate::text_anim_render::has_sprites(params, timeline_size, output_size)
    } else {
        let cache = render_cache().lock().unwrap_or_else(|e| e.into_inner());
        cache
            .iter()
            .any(|(k, _)| k.1 == timeline_size && k.2 == output_size && k.0 == *params)
    };
    if ready {
        return;
    }
    static WORKER: OnceLock<Arc<(Mutex<PrewarmQueue>, Condvar)>> = OnceLock::new();
    let worker = WORKER.get_or_init(|| {
        let queue = Arc::new((Mutex::new(PrewarmQueue::default()), Condvar::new()));
        let worker_queue = queue.clone();
        let spawned = std::thread::Builder::new()
            .name("vv-text-prewarm".into())
            .spawn(move || {
                let (lock, ready) = &*worker_queue;
                loop {
                    let (params, timeline_size, output_size) = {
                        let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
                        while queue.pending.is_empty() {
                            queue = ready.wait(queue).unwrap_or_else(|e| e.into_inner());
                        }
                        let request = queue.pending.pop_front().unwrap();
                        queue.active = Some(request.clone());
                        request
                    };
                    if params.anim_state.is_some() {
                        crate::text_anim_render::build(&params, timeline_size, output_size);
                    } else {
                        render_title(&params, timeline_size, output_size);
                    }
                    lock.lock().unwrap_or_else(|e| e.into_inner()).active = None;
                }
            });
        if let Err(e) = spawned {
            eprintln!("vv-render: no text prewarm thread: {e}");
        }
        queue
    });
    worker.0.lock().unwrap_or_else(|e| e.into_inner()).push((
        params.clone(),
        timeline_size,
        output_size,
    ));
    worker.1.notify_one();
}

pub(crate) fn text_attrs(params: &TitleParams) -> (Attrs<'_>, Align) {
    let family = if params.font_family.is_empty() {
        Family::SansSerif
    } else {
        Family::Name(&params.font_family)
    };
    let mut attrs = Attrs::new()
        .family(family)
        .weight(Weight(params.font_weight))
        .style(if params.italic {
            Style::Italic
        } else {
            Style::Normal
        })
        .letter_spacing(params.tracking / 1000.0);
    if params.underline {
        attrs.text_decoration.underline = UnderlineStyle::Single;
    }
    attrs.text_decoration.strikethrough = params.strikethrough;
    let align = match params.align {
        TextAlign::Left => Align::Left,
        TextAlign::Center => Align::Center,
        TextAlign::Right => Align::Right,
        TextAlign::Justify => Align::Justified,
    };
    (attrs, align)
}

/// Shapes the text into `buffer` and returns the size of the block.
/// Without a width, alignment has no reference: the layout is redone on the
/// longest line. The extra pixel keeps rounding from wrapping precisely
/// that line.
fn shape_block(
    font_system: &mut FontSystem,
    buffer: &mut Buffer,
    params: &TitleParams,
    attrs: &Attrs,
    align: Align,
) -> (f32, f32) {
    buffer.set_size(None, None);
    buffer.set_text(
        &params.display_text(),
        attrs,
        Shaping::Advanced,
        Some(align),
    );
    buffer.shape_until_scroll(font_system, false);
    let block_w = buffer.layout_runs().map(|r| r.line_w).fold(0.0, f32::max);
    buffer.set_size(Some(block_w + 1.0), None);
    buffer.shape_until_scroll(font_system, false);
    let block_h = buffer
        .layout_runs()
        .map(|r| r.line_top + r.line_height)
        .fold(0.0, f32::max);
    (block_w, block_h)
}

/// Text block and background padding of a title, in timeline pixels: what
/// the background covers on the axes left at 0, which mean "around the
/// text" (see `TitleBackground`). The OTIO export needs them separately,
/// because Resolve has no such shorthand and shapes the text its own way.
pub fn title_metrics(params: &TitleParams) -> vv_core::TitleMetrics {
    let font_size = params.size.max(1.0);
    let line_height = (font_size * 1.2 + params.line_spacing).max(1.0);
    let (attrs, align) = text_attrs(params);
    let block = with_fonts(|font_system, _| {
        let mut buffer = Buffer::new(font_system, Metrics::new(font_size, line_height));
        shape_block(font_system, &mut buffer, params, &attrs, align)
    });
    vv_core::TitleMetrics {
        block,
        padding: font_size * params.background.padding,
    }
}

fn with_opacity(color: Rgba, opacity: f32) -> Rgba {
    Rgba {
        a: color.a * (opacity / 100.0).clamp(0.0, 1.0),
        ..color
    }
}

/// Glyph coverage rectangles in frame pixels: x, y, width, height, coverage.
pub(crate) type GlyphRect = (i32, i32, u32, u32, u8);

pub(crate) fn rect_bounds(rects: &[GlyphRect]) -> Option<(i32, i32, i32, i32)> {
    rects.iter().fold(None, |acc, &(x, y, w, h, _)| {
        let (x1, y1) = (x + w as i32, y + h as i32);
        Some(match acc {
            None => (x, y, x1, y1),
            Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(x1), d.max(y1)),
        })
    })
}

/// `rects` drawn into a mask of `size` whose top-left is `origin`.
pub(crate) fn coverage_mask(rects: &[GlyphRect], origin: (i32, i32), size: (u32, u32)) -> TextMask {
    let mut mask = TextMask::empty(size.0, size.1);
    for &(x, y, w, h, coverage) in rects {
        let coverage = u32::from(coverage);
        for py in (y - origin.1).max(0)..(y - origin.1 + h as i32).min(size.1 as i32) {
            let row = py as usize * size.0 as usize;
            for px in (x - origin.0).max(0)..(x - origin.0 + w as i32).min(size.0 as i32) {
                let dst = &mut mask.data[row + px as usize];
                // Overlapping glyphs (negative tracking): "over" between coverages.
                *dst = (u32::from(*dst) + coverage * (255 - u32::from(*dst)) / 255) as u8;
            }
        }
    }
    mask
}

/// Pixels a style can reach beyond the glyphs.
pub(crate) fn reach(params: &TitleParams, scale: f32, font_px: f32, halo: bool) -> i32 {
    let style = &params.style;
    let stroke = style.strokes.iter().map(|s| s.width).fold(0.0, f32::max);
    let shadow = style
        .shadows
        .iter()
        .map(|s| s.offset[0].abs().max(s.offset[1].abs()) + s.blur * 1.5)
        .fold(0.0, f32::max);
    // Grown by 0.35 radius, then blurred with sigma 0.65 radius (3 sigma).
    let glow = style
        .glows
        .iter()
        .map(|g| g.radius * 2.4)
        .fold(0.0, f32::max);
    let extrude = style.extrude.as_ref().map_or(0.0, |e| e.depth);
    let mut total = (stroke + shadow + glow + extrude) * scale;
    for underlay in &style.underlays {
        let mut child = params.clone();
        child.style = underlay.style.clone();
        let offset = underlay.offset[0].abs().max(underlay.offset[1].abs()) * scale;
        total = total.max(reach(&child, scale, font_px, halo) as f32 + offset);
    }
    total += font_px * if halo { 0.6 } else { 0.2 };
    total.ceil() as i32 + 2
}

fn rasterize(
    params: &TitleParams,
    timeline_size: (u32, u32),
    output_size: (u32, u32),
) -> TitleRender {
    let (width, height) = (output_size.0.max(1), output_size.1.max(1));
    let scale = width as f32 / timeline_size.0.max(1) as f32;
    let font_size = (params.size * scale).max(1.0);
    let line_height = (font_size * 1.2 + params.line_spacing * scale).max(1.0);

    let (attrs, align) = text_attrs(params);
    let mut rects: Vec<GlyphRect> = Vec::new();
    let (block_w, block_h, origin_x, origin_y) = with_fonts(|font_system, swash| {
        let mut buffer = Buffer::new(font_system, Metrics::new(font_size, line_height));
        let (block_w, block_h) = shape_block(font_system, &mut buffer, params, &attrs, align);
        let (origin_x, origin_y) = block_origin(params, (width, height), scale, (block_w, block_h));
        buffer.draw(
            font_system,
            swash,
            Color::rgb(255, 255, 255),
            |x, y, w, h, color| {
                if color.a() != 0 {
                    rects.push((x + origin_x, y + origin_y, w, h, color.a()));
                }
            },
        );
        (block_w, block_h, origin_x, origin_y)
    });

    let block_center = (
        origin_x as f32 + block_w / 2.0,
        origin_y as f32 + block_h / 2.0,
    );
    let mut layers = background_layers(
        params,
        (width, height),
        scale,
        (block_center, (block_w, block_h)),
    );
    let margin = reach(params, scale, font_size, false);
    // Off-frame ink still makes its layers (empty), so the stack is stable.
    let (origin, text) = match rect_bounds(&rects) {
        Some((x0, y0, x1, y1))
            if x1 + margin > 0
                && y1 + margin > 0
                && x0 - margin < width as i32
                && y0 - margin < height as i32 =>
        {
            let origin = (x0 - margin, y0 - margin);
            let size = ((x1 - x0 + 2 * margin) as u32, (y1 - y0 + 2 * margin) as u32);
            (origin, coverage_mask(&rects, origin, size))
        }
        _ => ((0, 0), TextMask::empty(1, 1)),
    };
    let local_center = (
        block_center.0 - origin.0 as f32,
        block_center.1 - origin.1 as f32,
    );
    let text_size = (text.width, text.height);
    let (styled, _) = style_layers(text, params, scale, local_center, (block_w, block_h));
    let placed: Vec<_> = styled
        .into_par_iter()
        .map(|layer| {
            let mask = match layer.step {
                1 => layer.mask,
                step => text_fx::enlarged(&layer.mask, step, text_size),
            };
            (
                PlacedMask::place(&mask, origin, (width, height)),
                layer.color,
            )
        })
        .collect();
    layers.extend(placed);
    TitleRender {
        size: (width, height),
        layers: layers
            .into_iter()
            .map(|(mask, color)| TitleLayer::Mask(mask, color))
            .collect(),
    }
}

/// Top-left of the text block in frame pixels.
pub(crate) fn block_origin(
    params: &TitleParams,
    (width, height): (u32, u32),
    scale: f32,
    (block_w, block_h): (f32, f32),
) -> (i32, i32) {
    let anchor_x = width as f32 / 2.0 + params.position[0] * scale;
    let anchor_y = height as f32 / 2.0 - params.position[1] * scale;
    let origin_x = anchor_x
        - match params.anchor.0 {
            HAnchor::Left => 0.0,
            HAnchor::Center => block_w / 2.0,
            HAnchor::Right => block_w,
        };
    let origin_y = anchor_y
        - match params.anchor.1 {
            VAnchor::Top => 0.0,
            VAnchor::Middle => block_h / 2.0,
            VAnchor::Bottom => block_h,
        };
    (origin_x.round() as i32, origin_y.round() as i32)
}

/// The background shape of a title (rounded rectangle or pill), if enabled.
pub(crate) fn background_layers(
    params: &TitleParams,
    (width, height): (u32, u32),
    scale: f32,
    (block_center, (block_w, block_h)): ((f32, f32), (f32, f32)),
) -> Vec<(PlacedMask, Rgba)> {
    let scale_px = |v: f32| v * scale;
    let font_size = (params.size * scale).max(1.0);
    let mut layers = Vec::new();
    let bg = &params.background;
    if bg.enabled {
        let padding = font_size * bg.padding;
        let box_w = if bg.width > 0.0 {
            bg.width * width as f32
        } else {
            block_w + padding * 2.0
        };
        let box_h = if bg.height > 0.0 {
            bg.height * height as f32
        } else {
            block_h + padding * 2.0
        };
        let center = (
            block_center.0 + scale_px(bg.center[0]),
            block_center.1 - scale_px(bg.center[1]),
        );
        let radius = match bg.shape {
            BackgroundShape::Pill => box_w.min(box_h) / 2.0,
            BackgroundShape::Speech => 0.3 * box_w.min(box_h),
            _ => bg.corner_radius.clamp(0.0, 0.5) * box_w.min(box_h),
        };
        let outline = scale_px(bg.outline_width).max(0.0);
        let (origin, fill, ring) = shape_masks(
            bg.shape,
            width,
            height,
            center,
            (box_w, box_h),
            radius,
            outline,
            bg.rotation.to_radians(),
        );
        let place = |mask: &TextMask| PlacedMask::place(mask, origin, (width, height));
        let opacity = (bg.opacity / 100.0).clamp(0.0, 1.0);
        let local_center = (center.0 - origin.0 as f32, center.1 - origin.1 as f32);
        match &bg.gradient {
            Some(g) => layers.extend(
                text_fx::gradient_layers(
                    &fill,
                    g,
                    local_center,
                    (box_w, box_h),
                    -bg.rotation,
                    opacity,
                )
                .iter()
                .map(|(m, c)| (place(m), *c)),
            ),
            None => layers.push((place(&fill), with_opacity(bg.color, bg.opacity))),
        }
        if let Some(ring) = ring {
            layers.push((place(&ring), with_opacity(bg.outline_color, bg.opacity)));
        }
    }

    layers
}

/// One color of the style of a title, as large as its text mask.
pub(crate) struct StyledLayer {
    pub mask: TextMask,
    /// Text mask pixels per texel: a blurred layer is kept smaller.
    pub step: u32,
    pub color: Rgba,
}

impl StyledLayer {
    fn full(mask: TextMask, color: Rgba) -> Self {
        Self {
            mask,
            step: 1,
            color,
        }
    }
}

/// Shadows, glows, extrusion, strokes and fill of `text`, bottom up, and
/// the index where the fill layers start.
pub(crate) fn style_layers(
    text: TextMask,
    params: &TitleParams,
    scale: f32,
    block_center: (f32, f32),
    (block_w, block_h): (f32, f32),
) -> (Vec<StyledLayer>, usize) {
    let scale_px = |v: f32| v * scale;
    let style = &params.style;
    let mut backplates = Vec::new();
    for underlay in &style.underlays {
        let mut child = params.clone();
        child.style = underlay.style.clone();
        child.color = underlay.color;
        let shifted = text_fx::shifted(
            &text,
            scale_px(underlay.offset[0]).round() as i32,
            -scale_px(underlay.offset[1]).round() as i32,
        );
        backplates.extend(style_layers(shifted, &child, scale, block_center, (block_w, block_h)).0);
    }
    // Widest stroke first: the narrower ones sit on top of it.
    let mut strokes: Vec<_> = style.strokes.iter().filter(|s| s.width > 0.0).collect();
    strokes.sort_by(|a, b| b.width.total_cmp(&a.width));
    let stroke_masks: Vec<StyledLayer> = strokes
        .par_iter()
        .map(|s| StyledLayer::full(text_fx::grow(&text, scale_px(s.width), s.join), s.color))
        .collect();
    // Shadow, glow and extrusion follow the outermost outline.
    let silhouette = stroke_masks.first().map_or(&text, |l| &l.mask);

    enum Under<'a> {
        Shadow(&'a vv_core::TextShadow),
        Glow(&'a vv_core::TextGlow),
        Extrude(&'a vv_core::TextExtrude),
    }
    let under: Vec<Under> = (style.shadows.iter().map(Under::Shadow))
        .chain(style.glows.iter().map(Under::Glow))
        .chain(
            style
                .extrude
                .iter()
                .filter(|e| e.depth > 0.0)
                .map(Under::Extrude),
        )
        .collect();
    let layers: Vec<StyledLayer> = under
        .par_iter()
        .map(|item| match item {
            Under::Shadow(shadow) => {
                let (mask, step) = text_fx::soft_shadow(
                    silhouette,
                    scale_px(shadow.offset[0]).round() as i32,
                    (-scale_px(shadow.offset[1])).round() as i32,
                    scale_px(shadow.blur) / 2.0,
                );
                StyledLayer {
                    mask,
                    step,
                    color: with_opacity(shadow.color, shadow.opacity),
                }
            }
            Under::Glow(glow) => {
                let radius = scale_px(glow.radius).max(0.0);
                let (mask, step) = text_fx::glow(silhouette, radius, glow.intensity.max(0.0));
                StyledLayer {
                    mask,
                    step,
                    color: glow.color,
                }
            }
            Under::Extrude(ex) => StyledLayer::full(
                text_fx::extruded(silhouette, scale_px(ex.depth), ex.angle),
                ex.color,
            ),
        })
        .collect();
    backplates.extend(layers);
    let mut layers = backplates;
    layers.extend(stroke_masks);

    let fill_start = layers.len();
    match &style.fill {
        TextFill::Solid => layers.push(StyledLayer::full(text, params.color)),
        TextFill::Gradient(g) => {
            let gradient = text_fx::gradient_layers(
                &text,
                g,
                block_center,
                (block_w, block_h),
                0.0,
                params.color.a,
            );
            if gradient.is_empty() {
                layers.push(StyledLayer::full(text, params.color));
            } else {
                layers.extend(gradient.into_iter().map(|(m, c)| StyledLayer::full(m, c)));
            }
        }
    }
    (layers, fill_start)
}

/// Top-left in the frame, fill and (if `outline > 0`) inner border of a
/// rounded rectangle, with the edges antialiased from the signed distance.
/// The masks cover only the rectangle's box, cut to the frame.
/// Signed distance (pixels, negative inside) from `shape` of half size
/// `half`, at `(x, y)` in its own unrotated frame, y down.
fn shape_distance(
    shape: BackgroundShape,
    (x, y): (f32, f32),
    half: (f32, f32),
    radius: f32,
) -> f32 {
    let rounded = |x: f32, y: f32, half: (f32, f32), radius: f32| {
        let px = x.abs() - (half.0 - radius);
        let py = y.abs() - (half.1 - radius);
        if px > 0.0 && py > 0.0 {
            (px * px + py * py).sqrt() - radius
        } else {
            px.max(py) - radius
        }
    };
    let ellipse = |x: f32, y: f32, half: (f32, f32)| {
        let k = (x / half.0.max(1e-3)).hypot(y / half.1.max(1e-3));
        (k - 1.0) * half.0.min(half.1)
    };
    let circle = |x: f32, y: f32, c: (f32, f32), r: f32| (x - c.0).hypot(y - c.1) - r;
    match shape {
        BackgroundShape::RoundedRect | BackgroundShape::Pill => rounded(x, y, half, radius),
        BackgroundShape::Ellipse => ellipse(x, y, half),
        BackgroundShape::Speech => {
            let body = rounded(x, y, half, radius);
            // Tail: a triangle from the bottom edge down to the left.
            let a = (-0.45 * half.0, half.1 - radius.min(half.1 * 0.5));
            let b = (-0.05 * half.0, half.1 - radius.min(half.1 * 0.5));
            let c = (-0.7 * half.0, half.1 * 1.75);
            body.min(triangle_distance((x, y), a, b, c))
        }
        BackgroundShape::Thought => {
            let body = ellipse(x, y, half);
            let small = circle(x, y, (-0.55 * half.0, 1.3 * half.1), 0.22 * half.1);
            let tiny = circle(x, y, (-0.8 * half.0, 1.75 * half.1), 0.12 * half.1);
            body.min(small).min(tiny)
        }
        BackgroundShape::Burst => {
            let (nx, ny) = (x / half.0.max(1e-3), y / half.1.max(1e-3));
            let angle = ny.atan2(nx);
            let spikes = 14.0;
            let wave = ((angle * spikes / std::f32::consts::TAU).fract() - 0.5).abs() * 2.0;
            let reach = 1.0 + 0.3 * wave;
            (nx.hypot(ny) - reach) * half.0.min(half.1) * 0.8
        }
    }
}

/// Signed distance from triangle `a b c` (iq's sdTriangle).
fn triangle_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    let sub = |u: (f32, f32), v: (f32, f32)| (u.0 - v.0, u.1 - v.1);
    let dot = |u: (f32, f32), v: (f32, f32)| u.0 * v.0 + u.1 * v.1;
    let edges = [(a, b), (b, c), (c, a)];
    let mut d = f32::MAX;
    let mut sign = 0.0f32;
    let mut signs = [0.0f32; 3];
    for (i, (p0, p1)) in edges.iter().enumerate() {
        let e = sub(*p1, *p0);
        let w = sub(p, *p0);
        let t = (dot(w, e) / dot(e, e).max(1e-6)).clamp(0.0, 1.0);
        let q = (w.0 - e.0 * t, w.1 - e.1 * t);
        d = d.min(dot(q, q));
        signs[i] = e.0 * w.1 - e.1 * w.0;
    }
    if signs.iter().all(|s| *s >= 0.0) || signs.iter().all(|s| *s <= 0.0) {
        sign = -1.0;
    }
    if sign == 0.0 {
        sign = 1.0;
    }
    sign * d.sqrt()
}

#[allow(clippy::too_many_arguments)]
fn shape_masks(
    shape: BackgroundShape,
    width: u32,
    height: u32,
    center: (f32, f32),
    size: (f32, f32),
    radius: f32,
    outline: f32,
    rotation: f32,
) -> ((i32, i32), TextMask, Option<TextMask>) {
    let half = (size.0 / 2.0, size.1 / 2.0);
    let grown = (half.0 * shape.overhang(), half.1 * shape.overhang());
    let reach = if rotation == 0.0 {
        grown
    } else {
        let r = grown.0.hypot(grown.1);
        (r, r)
    };
    let (sin, cos) = (-rotation).sin_cos();
    let x_range = ((center.0 - reach.0 - 1.0).floor().max(0.0) as u32)
        ..((center.0 + reach.0 + 1.0).ceil().clamp(0.0, width as f32) as u32);
    let y_range = ((center.1 - reach.1 - 1.0).floor().max(0.0) as u32)
        ..((center.1 + reach.1 + 1.0).ceil().clamp(0.0, height as f32) as u32);
    let (box_w, box_h) = (x_range.len().max(1) as u32, y_range.len().max(1) as u32);
    let (ox, oy) = (x_range.start, y_range.start);
    let len = (box_w * box_h) as usize;
    let mut fill = vec![0u8; len];
    let mut ring = (outline > 0.0).then(|| vec![0u8; len]);
    let coverage = |d: f32| (0.5 - d).clamp(0.0, 1.0);
    for y in y_range {
        for x in x_range.clone() {
            let (dx, dy) = (x as f32 + 0.5 - center.0, y as f32 + 0.5 - center.1);
            let local = (dx * cos - dy * sin, dx * sin + dy * cos);
            let d = shape_distance(shape, local, half, radius);
            let i = ((y - oy) * box_w + x - ox) as usize;
            let inside = coverage(d);
            fill[i] = (inside * 255.0).round() as u8;
            if let Some(ring) = &mut ring {
                ring[i] = ((inside - coverage(d + outline)).max(0.0) * 255.0).round() as u8;
            }
        }
    }
    let mask = |data| TextMask {
        width: box_w,
        height: box_h,
        data,
    };
    ((ox as i32, oy as i32), mask(fill), ring.map(mask))
}

/// A title with its masks as large as the frame, as the tests read them.
#[cfg(test)]
pub(crate) struct DenseRender {
    pub layers: Vec<(TextMask, Rgba)>,
}

#[cfg(test)]
impl DenseRender {
    pub fn text(&self) -> &TextMask {
        &self.layers.last().expect("the text is always there").0
    }
}

#[cfg(test)]
pub(crate) fn dense_title(
    params: &TitleParams,
    timeline_size: (u32, u32),
    output_size: (u32, u32),
) -> DenseRender {
    let render = render_title(params, timeline_size, output_size);
    let (w, h) = render.size;
    let layers = render
        .masks()
        .iter()
        .map(|(placed, color)| {
            let mut mask = TextMask::empty(w, h);
            for y in 0..h {
                for x in 0..w {
                    mask.data[(y * w + x) as usize] = placed.at(x, y);
                }
            }
            (mask, *color)
        })
        .collect();
    DenseRender { layers }
}

#[cfg(test)]
#[path = "tests/text.rs"]
mod tests;
