//! How the decoded frame of a clip is obtained: a cache filled in the
//! background (preview, `render_ahead.rs`) or synchronous decoding (export,
//! `export.rs`). The clip -> source frame mapping is a single one.

use std::sync::Arc;
use vv_core::{
    Clip, ClipSource, FrameIdx, MediaId, Project, Rgba, Timeline, TitleParams, Transform,
};
use vv_media::FrameYuv420;

use crate::export::ExportError;

/// `&mut self`: the export keeps the decoders open.
pub trait FrameProvider {
    /// `Ok(None)` = not available now (preview: not cached yet;
    /// export: past the end of the file). A decode error stays `Err`, so
    /// the export does not turn it into a silent black frame.
    fn frame_for(
        &mut self,
        project: &Project,
        clip: &Clip,
        timeline_frame: FrameIdx,
    ) -> Result<Option<Arc<FrameYuv420>>, ExportError>;

    /// The composed frame of `nested` (the nested timeline of a compound
    /// clip) at `local_frame`, as a GPU texture. `None` from the default
    /// implementation: whoever cannot compose on the GPU falls back on
    /// `frame_for`, i.e. on the frame composed and brought back to the CPU.
    /// Whoever implements it composes recursively with `track_layers_at`, passing itself.
    fn compound_texture(
        &mut self,
        _project: &Project,
        _nested: &Timeline,
        _local_frame: FrameIdx,
    ) -> Result<Option<vv_render::PooledTexture>, ExportError> {
        Ok(None)
    }

    /// The frame at `progress` of a Smooth Cut from `from` to `to`, as a GPU
    /// texture of `timeline_size`. `None` from the default implementation:
    /// the caller falls back on a dissolve.
    fn smooth_cut_texture(
        &mut self,
        _from: &OwnedLayer,
        _to: &OwnedLayer,
        _progress: f32,
        _timeline_size: (u32, u32),
    ) -> Option<vv_render::PooledTexture> {
        None
    }

    /// `a` composed and run through `passes` in order, as a GPU texture of
    /// `timeline_size`; `b` is the incoming side of a transition (empty for
    /// effects). `None` from the default implementation: the caller keeps
    /// `a` as it is.
    fn asset_texture(
        &mut self,
        _passes: &[AssetPass],
        _a: &[OwnedLayer],
        _b: &[OwnedLayer],
        _timeline_size: (u32, u32),
    ) -> Option<vv_render::PooledTexture> {
        None
    }

    /// `fill` cut out by `matte` (see `stack_layers`), as a GPU texture of
    /// `timeline_size`. `None` from the default implementation: the caller
    /// shows `fill` as it is.
    fn matte_texture(
        &mut self,
        _fill: &[OwnedLayer],
        _matte: &[OwnedLayer],
        _mode: vv_core::MatteMode,
        _timeline_size: (u32, u32),
    ) -> Option<vv_render::PooledTexture> {
        None
    }
}

/// The nested timeline of `clip`, if it is a compound clip.
fn nested_timeline_of<'a>(project: &'a Project, clip: &Clip) -> Option<&'a Timeline> {
    let ClipSource::Media(media_id) = &clip.source else {
        return None;
    };
    let nested_id = project.media_pool.get(*media_id)?.compound?;
    project.timelines.get(nested_id)
}

/// `(media, source frame)` of `clip` at `timeline_frame`; `None` if it is not
/// a Media clip.
pub fn media_source_frame(clip: &Clip, timeline_frame: FrameIdx) -> Option<(MediaId, FrameIdx)> {
    let ClipSource::Media(media_id) = &clip.source else {
        return None;
    };
    Some((*media_id, clip.picture_frame_at(timeline_frame)))
}

/// *Native* resolution of the media, the unit of the crop even when decoding
/// the proxy. Generators: the timeline's; `(1, 1)` if the media is missing.
pub fn clip_source_size(project: &Project, clip: &Clip, timeline_size: (u32, u32)) -> (u32, u32) {
    match &clip.source {
        ClipSource::Media(id) => project
            .media_pool
            .get(*id)
            .map(|m| (m.meta.width, m.meta.height))
            .unwrap_or((1, 1)),
        ClipSource::SolidColor | ClipSource::Text | ClipSource::Adjustment => timeline_size,
    }
}

/// vv-render does not depend on vv-media: it takes raw byte planes.
pub fn as_render_yuv_frame(frame: &FrameYuv420) -> vv_render::YuvFrame<'_> {
    with_matte(frame, None)
}

/// Like `as_render_yuv_frame`, the coverage taken from `matte` if any.
pub fn with_matte<'a>(
    frame: &'a FrameYuv420,
    matte: Option<&'a vv_media::matte::Matte>,
) -> vv_render::YuvFrame<'a> {
    let (alpha, alpha_size) = match matte {
        Some(m) => (m.data.as_slice(), Some((m.width, m.height))),
        None => (frame.alpha.as_deref().unwrap_or(&[255]), None),
    };
    vv_render::YuvFrame {
        y: &frame.y,
        width: frame.width,
        height: frame.height,
        chroma: match &frame.chroma {
            vv_media::Chroma::Planar { u, v } => vv_render::YuvChroma::Planar { u, v },
            vv_media::Chroma::Interleaved(uv) => vv_render::YuvChroma::Interleaved(uv),
        },
        chroma_width: frame.chroma_width,
        chroma_height: frame.chroma_height,
        matrix: frame.matrix,
        full_range: frame.full_range,
        alpha,
        alpha_size,
    }
}

/// A compositing layer that owns what `vv_render::Layer` borrows.
pub struct OwnedLayer {
    pub content: OwnedContent,
    pub transform: Transform,
    pub opacity: f32,
    /// Only the active filters of `EffectStack::filters`, in their order.
    pub filters: Vec<vv_core::FilterValue>,
    pub blend: vv_core::BlendMode,
    /// Only the active masks of `EffectStack::masks` (`ClipMask::is_active`).
    pub masks: Vec<vv_core::MaskValue>,
    /// Asset effects to run over what this layer adjusts. Only set on an
    /// adjustment layer: `stack_layers` applies them to the stack below.
    pub assets: Vec<AssetPass>,
}

/// One asset shader pass, ready for the compositor.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetPass {
    pub id: String,
    pub params: [f32; vv_render::assets::MAX_PARAMS],
    pub progress: f32,
    pub time: f32,
    pub amount: f32,
}

/// The enabled asset effects of `clip` as passes at timeline `frame`.
/// `amount` scales how much of the effect shows (fades of an effect element).
fn asset_passes(clip: &Clip, frame: FrameIdx, fps: f64, amount: f32) -> Vec<AssetPass> {
    let animations = &clip.effects.animations;
    if clip.effects.assets.is_empty()
        && animations.in_asset.is_none()
        && animations.out_asset.is_none()
        && animations.combo_asset.is_none()
    {
        return Vec::new();
    }
    let library = vv_render::assets::library();
    let local = (frame - clip.timeline_start) as f32;
    let progress = (local / clip.timeline_len.max(1) as f32).clamp(0.0, 1.0);
    let mut passes: Vec<_> = clip
        .effects
        .assets
        .iter()
        .filter(|fx| fx.enabled)
        .filter_map(|fx| {
            let asset = library.get(&fx.id)?;
            Some(AssetPass {
                id: fx.id.clone(),
                params: asset.shader_params(&fx.params),
                progress,
                time: local / fps.max(1.0) as f32,
                amount,
            })
        })
        .collect();
    for (id, progress, time) in clip.effects.animations.asset_times(
        local as f64 / fps.max(1.0),
        clip.timeline_len as f64 / fps.max(1.0),
    ) {
        if let Some(asset) = library
            .get(id)
            .filter(|a| a.kind == vv_render::assets::AssetKind::Animation)
        {
            passes.push(AssetPass {
                id: id.to_owned(),
                params: asset.shader_params(&[]),
                progress,
                time,
                amount: 1.0,
            });
        }
    }
    passes
}

#[allow(clippy::large_enum_variant)] // Layers are short-lived and few per frame; boxing buys nothing.
pub enum OwnedContent {
    Video {
        frame: Arc<FrameYuv420>,
        /// Native resolution of the media (not of the proxy): the units of the crop.
        source_size: (u32, u32),
        /// Background removal: the frame's coverage.
        matte: Option<Arc<vv_media::matte::Matte>>,
    },
    /// Compound clip composed on the GPU (see `FrameProvider::compound_texture`).
    Texture {
        texture: vv_render::PooledTexture,
        /// Resolution of the nested timeline: the units of the crop, like
        /// `Video`'s `source_size` (the texture can be smaller).
        source_size: (u32, u32),
    },
    Solid(Rgba),
    Text(TitleParams),
    Adjustment,
}

impl OwnedLayer {
    pub fn as_render(&self) -> vv_render::Layer<'_> {
        let content = match &self.content {
            OwnedContent::Video {
                frame,
                source_size,
                matte,
            } => vv_render::LayerContent::Video {
                frame: with_matte(frame, matte.as_deref()),
                source_size: *source_size,
            },
            OwnedContent::Texture {
                texture,
                source_size,
            } => vv_render::LayerContent::Texture {
                texture,
                source_size: *source_size,
            },
            OwnedContent::Solid(color) => vv_render::LayerContent::Solid(*color),
            OwnedContent::Text(title) => vv_render::LayerContent::Text(title),
            OwnedContent::Adjustment => vv_render::LayerContent::Adjustment,
        };
        vv_render::Layer {
            content,
            transform: self.transform,
            opacity: self.opacity,
            filters: &self.filters,
            blend: self.blend,
            masks: &self.masks,
        }
    }

    /// `true` if composing it gives the same pixels as `other`. Frames are
    /// compared by identity: equal only while `other` holds its `Arc`. A
    /// compound clip's texture is composed anew every time, never equal.
    fn renders_same(&self, other: &OwnedLayer) -> bool {
        let same_content = match (&self.content, &other.content) {
            (
                OwnedContent::Video {
                    frame,
                    source_size,
                    matte,
                },
                OwnedContent::Video {
                    frame: frame2,
                    source_size: source_size2,
                    matte: matte2,
                },
            ) => {
                Arc::ptr_eq(frame, frame2)
                    && source_size == source_size2
                    && match (matte, matte2) {
                        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                        (None, None) => true,
                        _ => false,
                    }
            }
            (OwnedContent::Solid(color), OwnedContent::Solid(color2)) => color == color2,
            (OwnedContent::Text(title), OwnedContent::Text(title2)) => title == title2,
            // What it adjusts is the rest of the stack, compared on its own.
            (OwnedContent::Adjustment, OwnedContent::Adjustment) => true,
            _ => false,
        };
        same_content
            && self.transform == other.transform
            && self.opacity == other.opacity
            && self.filters == other.filters
            && self.blend == other.blend
            && self.masks == other.masks
            && self.assets == other.assets
    }
}

/// `true` if the two stacks compose to the same pixels (see
/// `OwnedLayer::renders_same`).
pub fn renders_same(a: &[OwnedLayer], b: &[OwnedLayer]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.renders_same(b))
}

/// Adds to any provider the GPU composition of compound clips: the nested
/// timeline becomes a texture that stays on the card, instead of a composed
/// frame brought back to the CPU and converted to YUV again.
/// `inner` provides the real media, which the rest of the pipeline decodes
/// as always (including those inside the nested timelines).
pub struct GpuCompounds<'a> {
    inner: &'a mut dyn FrameProvider,
    compositor: &'a vv_render::Compositor,
    depth: u32,
    preview_divisor: u32,
}

impl<'a> GpuCompounds<'a> {
    pub fn new(inner: &'a mut dyn FrameProvider, compositor: &'a vv_render::Compositor) -> Self {
        Self {
            inner,
            compositor,
            depth: 0,
            preview_divisor: 1,
        }
    }

    /// Composes the compound clips at most at their resolution divided by
    /// `divisor` (see `OutputFrame::reduced`).
    pub fn with_preview_divisor(self, divisor: u32) -> Self {
        Self {
            preview_divisor: divisor,
            ..self
        }
    }
}

impl FrameProvider for GpuCompounds<'_> {
    fn frame_for(
        &mut self,
        project: &Project,
        clip: &Clip,
        timeline_frame: FrameIdx,
    ) -> Result<Option<Arc<FrameYuv420>>, ExportError> {
        self.inner.frame_for(project, clip, timeline_frame)
    }

    fn compound_texture(
        &mut self,
        project: &Project,
        nested: &Timeline,
        local_frame: FrameIdx,
    ) -> Result<Option<vv_render::PooledTexture>, ExportError> {
        if self.depth >= vv_core::MAX_COMPOUND_DEPTH {
            return Ok(None);
        }
        self.depth += 1;
        let layers = nested_layers(project, nested, local_frame, self);
        self.depth -= 1;
        let Some(layers) = layers? else {
            return Ok(None);
        };
        let render_layers: Vec<vv_render::Layer> =
            layers.iter().map(OwnedLayer::as_render).collect();
        let (width, height) = nested.resolution;
        // Transparent background: where `nested` has nothing to show, what is
        // below in the outer timeline must stay visible. And the texture does
        // not go back to the pool, because we hold it until the pass.
        Ok(Some(
            self.compositor.render_layers_to_owned_texture_transparent(
                &render_layers,
                vv_render::OutputFrame::exact(width, height).reduced(self.preview_divisor),
            ),
        ))
    }

    fn smooth_cut_texture(
        &mut self,
        from: &OwnedLayer,
        to: &OwnedLayer,
        progress: f32,
        (width, height): (u32, u32),
    ) -> Option<vv_render::PooledTexture> {
        Some(self.compositor.render_smooth_cut_to_owned_texture(
            &from.as_render(),
            &to.as_render(),
            progress,
            vv_render::OutputFrame::exact(width, height).reduced(self.preview_divisor),
        ))
    }

    fn asset_texture(
        &mut self,
        passes: &[AssetPass],
        a: &[OwnedLayer],
        b: &[OwnedLayer],
        (width, height): (u32, u32),
    ) -> Option<vv_render::PooledTexture> {
        let output = vv_render::OutputFrame::exact(width, height).reduced(self.preview_divisor);
        let compose = |layers: &[OwnedLayer]| {
            let layers: Vec<_> = layers.iter().map(OwnedLayer::as_render).collect();
            self.compositor
                .render_layers_to_owned_texture_transparent(&layers, output)
        };
        let library = vv_render::assets::library();
        let mut current = compose(a);
        let incoming = (!b.is_empty()).then(|| compose(b));
        for pass in passes {
            let Some(asset) = library.get(&pass.id) else {
                continue;
            };
            let custom = vv_render::CustomPass {
                key: &pass.id,
                source: &asset.shader,
                params: pass.params,
                curves: asset.curve_values(pass.progress),
                progress: pass.progress,
                time: pass.time,
                amount: pass.amount,
            };
            let second: &vv_render::wgpu::Texture = match &incoming {
                Some(texture) => texture,
                None => &current,
            };
            if let Some(next) = self
                .compositor
                .render_custom_pass(&custom, &current, second)
            {
                current = next;
            }
        }
        Some(current)
    }

    fn matte_texture(
        &mut self,
        fill: &[OwnedLayer],
        matte: &[OwnedLayer],
        mode: vv_core::MatteMode,
        (width, height): (u32, u32),
    ) -> Option<vv_render::PooledTexture> {
        let fill: Vec<_> = fill.iter().map(OwnedLayer::as_render).collect();
        let matte: Vec<_> = matte.iter().map(OwnedLayer::as_render).collect();
        Some(self.compositor.render_matted_to_owned_texture(
            &fill,
            &matte,
            mode,
            vv_render::OutputFrame::exact(width, height).reduced(self.preview_divisor),
        ))
    }
}

/// The layers of `nested` at `local_frame`, or `None` if a clip covering that
/// frame does not have its content yet (a media being decoded): a half-composed
/// frame would be worse than no frame. No active clip is a valid case,
/// not a "not ready": it gives a transparent frame.
fn nested_layers(
    project: &Project,
    nested: &Timeline,
    local_frame: FrameIdx,
    provider: &mut dyn FrameProvider,
) -> Result<Option<Vec<OwnedLayer>>, ExportError> {
    let mut groups = Vec::new();
    for (track_index, clip) in nested.active_video_clips_at(local_frame) {
        let clip_layers = track_layers_at(
            project,
            nested,
            track_index,
            clip,
            local_frame,
            nested.resolution,
            provider,
        )?;
        if clip_layers.is_empty() {
            return Ok(None);
        }
        groups.push((clip.effects.matte, clip_layers));
    }
    Ok(Some(stack_layers(groups, provider, nested.resolution)))
}

/// The stack from the layers of each track, bottom to top, each with the
/// matte mode of its clip: a matte is not shown, it cuts out the track
/// below it instead. A matte with nothing below is dropped. An
/// adjustment below stands for the whole stack it adjusts.
pub fn stack_layers(
    groups: impl IntoIterator<Item = (Option<vv_core::MatteMode>, Vec<OwnedLayer>)>,
    provider: &mut dyn FrameProvider,
    timeline_size: (u32, u32),
) -> Vec<OwnedLayer> {
    let mut stack: Vec<Vec<OwnedLayer>> = Vec::new();
    for (matte, layers) in groups {
        if matte.is_none() && layers.iter().any(|l| !l.assets.is_empty()) {
            let passes: Vec<AssetPass> = layers.iter().flat_map(|l| l.assets.clone()).collect();
            let below: Vec<OwnedLayer> = stack.drain(..).flatten().chain(layers).collect();
            // Alone it adjusts nothing.
            if below.len() > 1
                && let Some(texture) = provider.asset_texture(&passes, &below, &[], timeline_size)
            {
                stack.push(vec![texture_layer(
                    texture,
                    timeline_size,
                    1.0,
                    Default::default(),
                )]);
            } else {
                stack.push(below);
            }
            continue;
        }
        let Some(mode) = matte else {
            stack.push(layers);
            continue;
        };
        let Some(mut fill) = stack.pop() else {
            continue;
        };
        // An adjustment replaces the stack with its adjusted version: that is
        // what the matte cuts out.
        if fill
            .iter()
            .any(|l| matches!(l.content, OwnedContent::Adjustment))
        {
            let adjusted: Vec<OwnedLayer> = stack.drain(..).flatten().chain(fill).collect();
            match provider.matte_texture(&adjusted, &layers, mode, timeline_size) {
                Some(texture) => stack.push(vec![OwnedLayer {
                    content: OwnedContent::Texture {
                        texture,
                        source_size: timeline_size,
                    },
                    transform: Transform::default(),
                    opacity: 1.0,
                    filters: Vec::new(),
                    blend: vv_core::BlendMode::Normal,
                    masks: Vec::new(),
                    assets: Vec::new(),
                }]),
                None => stack.push(adjusted),
            }
            continue;
        }
        let Some(blend) = fill.last().map(|l| l.blend) else {
            continue;
        };
        // Composed alone, the fill blends onto the stack only once it is cut out.
        let blends: Vec<_> = fill
            .iter_mut()
            .map(|l| std::mem::take(&mut l.blend))
            .collect();
        let Some(texture) = provider.matte_texture(&fill, &layers, mode, timeline_size) else {
            for (layer, blend) in fill.iter_mut().zip(blends) {
                layer.blend = blend;
            }
            stack.push(fill);
            continue;
        };
        stack.push(vec![OwnedLayer {
            content: OwnedContent::Texture {
                texture,
                source_size: timeline_size,
            },
            transform: Transform::default(),
            opacity: 1.0,
            filters: Vec::new(),
            blend,
            masks: Vec::new(),
            assets: Vec::new(),
        }]);
    }
    stack.into_iter().flatten().collect()
}

fn texture_layer(
    texture: vv_render::PooledTexture,
    size: (u32, u32),
    opacity: f32,
    blend: vv_core::BlendMode,
) -> OwnedLayer {
    OwnedLayer {
        content: OwnedContent::Texture {
            texture,
            source_size: size,
        },
        transform: Transform::default(),
        opacity,
        filters: Vec::new(),
        blend,
        masks: Vec::new(),
        assets: Vec::new(),
    }
}

/// `layer` run through the asset effects of its clip. An adjustment layer
/// only carries them (`stack_layers` applies them to what is below); any
/// other is composed alone, processed and put back as a texture, so the
/// effect sees its transform, filters and masks but not its opacity and blend.
fn with_assets(
    mut layer: OwnedLayer,
    mut passes: Vec<AssetPass>,
    provider: &mut dyn FrameProvider,
    timeline_size: (u32, u32),
) -> OwnedLayer {
    if passes.is_empty() {
        return layer;
    }
    let source_size = match &layer.content {
        OwnedContent::Video { source_size, .. } | OwnedContent::Texture { source_size, .. } => {
            *source_size
        }
        _ => timeline_size,
    };
    let (w, h) = (timeline_size.0 as f32, timeline_size.1 as f32);
    let fit = (w / source_size.0.max(1) as f32).min(h / source_size.1.max(1) as f32);
    for pass in &mut passes {
        if pass.id.starts_with("animations/") {
            pass.params = [
                0.5 + layer.transform.position[0] / w,
                0.5 - layer.transform.position[1] / h,
                source_size.0 as f32 * fit * layer.transform.zoom[0] / w,
                source_size.1 as f32 * fit * layer.transform.zoom[1] / h,
                layer.transform.rotation.to_radians(),
                0.0,
                0.0,
                0.0,
            ];
        }
    }
    if matches!(layer.content, OwnedContent::Adjustment) {
        layer.assets = passes;
        return layer;
    }
    let opacity = std::mem::replace(&mut layer.opacity, 1.0);
    let blend = std::mem::take(&mut layer.blend);
    match provider.asset_texture(&passes, std::slice::from_ref(&layer), &[], timeline_size) {
        Some(texture) => texture_layer(texture, timeline_size, opacity, blend),
        None => {
            layer.opacity = opacity;
            layer.blend = blend;
            layer
        }
    }
}

/// The clip's stabilization at timeline `frame`, on the picture it shows.
fn stabilize(
    project: &Project,
    clip: &Clip,
    frame: FrameIdx,
    timeline_size: (u32, u32),
    transform: &mut vv_core::Transform,
) {
    let (Some(stab), ClipSource::Media(media_id)) = (&clip.effects.stabilize, &clip.source) else {
        return;
    };
    let Some(media) = project.media_pool.get(*media_id) else {
        return;
    };
    let (w, h) = (timeline_size.0 as f32, timeline_size.1 as f32);
    let aspect = media.meta.width.max(1) as f32 / media.meta.height.max(1) as f32;
    let fit = if aspect > w / h {
        [w, w / aspect]
    } else {
        [h * aspect, h]
    };
    stab.apply(transform, clip.picture_frame_at(frame), fit);
}

/// The layer of `clip` at timeline frame `frame`, shared by preview
/// and export. `Ok(None)`: media frame not available.
pub fn clip_layer(
    project: &Project,
    clip: &Clip,
    frame: FrameIdx,
    timeline_size: (u32, u32),
    fps: f64,
    provider: &mut dyn FrameProvider,
) -> Result<Option<OwnedLayer>, ExportError> {
    let source_frame = clip.source_frame_at(frame);
    let mut transform = clip.effects.transform.value_at(source_frame);
    stabilize(project, clip, frame, timeline_size, &mut transform);
    clip.effects.animations.apply(
        &mut transform,
        (frame - clip.timeline_start) as f64 / fps.max(1.0),
        clip.timeline_len as f64 / fps.max(1.0),
        timeline_size.0 as f32,
    );
    let push = clip.transition_offset_at(
        frame,
        (timeline_size.0 as f32, timeline_size.1 as f32),
        transform.zoom,
    );
    transform.position[0] += push[0];
    transform.position[1] += push[1];
    let opacity = clip.fade_multiplier_at(frame) * clip.transition_opacity_at(frame);
    let content = clip_content(project, clip, frame, provider)?;
    let layer = build_layer(
        project,
        clip,
        frame,
        source_frame,
        transform,
        opacity,
        content,
        timeline_size,
        fps as f32,
    );
    let amount = clip.fade_multiplier_at(frame)
        * (clip.effects.transform.value_at(source_frame).opacity / 100.0).clamp(0.0, 1.0);
    Ok(layer.map(|layer| {
        let passes = asset_passes(clip, frame, fps, amount);
        with_assets(layer, passes, provider, timeline_size)
    }))
}

/// What a clip shows at `timeline_frame`: nothing if it is not a Media clip,
/// the composed texture if it is a compound clip and the provider can compose
/// it on the GPU, otherwise the decoded frame (or composed and brought back to the CPU).
fn clip_content(
    project: &Project,
    clip: &Clip,
    timeline_frame: FrameIdx,
    provider: &mut dyn FrameProvider,
) -> Result<ClipContent, ExportError> {
    if !matches!(clip.source, ClipSource::Media(_)) {
        return Ok(ClipContent::None);
    }
    if let Some(nested) = nested_timeline_of(project, clip)
        && let Some(texture) =
            provider.compound_texture(project, nested, clip.picture_frame_at(timeline_frame))?
    {
        return Ok(ClipContent::Texture(texture));
    }
    Ok(provider
        .frame_for(project, clip, timeline_frame)?
        .map_or(ClipContent::None, ClipContent::Yuv))
}

/// The already obtained content of a clip, see `clip_content`.
enum ClipContent {
    None,
    Yuv(Arc<FrameYuv420>),
    Texture(vv_render::PooledTexture),
}

/// The part common to `clip_layer` and to the side of a crossing transition
/// (`crossing_side_layer`): given the source frame, the already computed
/// transform and the already decoded media frame (if `ClipSource::Media`),
/// it assembles the right `OwnedLayer` for the clip's source kind.
fn build_layer(
    project: &Project,
    clip: &Clip,
    frame: FrameIdx,
    source_frame: FrameIdx,
    transform: Transform,
    opacity: f32,
    content: ClipContent,
    timeline_size: (u32, u32),
    fps: f32,
) -> Option<OwnedLayer> {
    let filters: Vec<vv_core::FilterValue> = clip
        .effects
        .filters
        .iter()
        .filter(|f| f.enabled)
        .map(|f| f.value_at(source_frame))
        .collect();
    let masks = clip
        .effects
        .masks
        .iter()
        .filter(|m| m.is_active())
        .map(|m| m.value_at(source_frame))
        .collect();
    let blend = clip.effects.blend_mode;
    // The clip opacity is multiplied by the one already carried by the
    // fades.
    let opacity = opacity * (transform.opacity / 100.0).clamp(0.0, 1.0);
    let content = match &clip.source {
        ClipSource::SolidColor => OwnedContent::Solid(
            clip.effects
                .color
                .as_ref()
                .map_or(Rgba::BLACK, |k| k.value_at(source_frame)),
        ),
        ClipSource::Text => {
            let title = clip.effects.title.as_ref()?.at(source_frame);
            let local = (frame - clip.timeline_start).max(0) as f32 / fps;
            OwnedContent::Text(title.animated(local, clip.timeline_len as f32 / fps))
        }
        ClipSource::Adjustment => OwnedContent::Adjustment,
        ClipSource::Media(_) => {
            let source_size = clip_source_size(project, clip, timeline_size);
            let frame_at = frame;
            match content {
                ClipContent::None => return None,
                ClipContent::Yuv(frame) => OwnedContent::Video {
                    frame,
                    source_size,
                    matte: background_matte(project, clip, frame_at),
                },
                ClipContent::Texture(texture) => OwnedContent::Texture {
                    texture,
                    source_size,
                },
            }
        }
    };
    Some(OwnedLayer {
        content,
        transform,
        opacity,
        filters,
        blend,
        masks,
        assets: Vec::new(),
    })
}

/// The matte of the media frame `clip` shows at timeline `frame`, if the
/// clip removes its background and the matte is ready.
fn background_matte(
    project: &Project,
    clip: &Clip,
    frame: FrameIdx,
) -> Option<Arc<vv_media::matte::Matte>> {
    if !clip.effects.remove_background {
        return None;
    }
    let ClipSource::Media(media_id) = &clip.source else {
        return None;
    };
    let item = project.media_pool.get(*media_id)?;
    let source_frame = if item.meta.is_image() {
        0
    } else {
        clip.picture_frame_at(frame)
    };
    vv_media::matte::load_matte(item.content_hash, source_frame)
}

/// The timeline frame to take the content from: `timeline_frame`, or that of
/// the nearest source frame if it falls past the real edges of the media
/// (freeze instead of nothing). Used only by the crossing transitions,
/// where "past the end" is the norm — it is the clip lending its edge to the
/// transition — not an error to report as `clip_layer` does on export.
fn held_timeline_frame(project: &Project, clip: &Clip, timeline_frame: FrameIdx) -> FrameIdx {
    let ClipSource::Media(media_id) = &clip.source else {
        return timeline_frame;
    };
    let Some(media) = project.media_pool.get(*media_id) else {
        return timeline_frame;
    };
    let wanted = clip.picture_frame_at(timeline_frame);
    let clamped = wanted.clamp(0, (media.meta.duration_frames - 1).max(0));
    clip.timeline_frame_at(clamped)
}

/// One side of a crossing transition for a single clip: like `clip_layer`,
/// but the source frame clamps to the edges of the media instead of
/// disappearing, and the position offset is `CrossTransition`'s instead of
/// `Clip::transition_offset_at` (which concerns only the single edges,
/// `transition_in`/`transition_out`).
fn crossing_side_layer(
    project: &Project,
    clip: &Clip,
    frame: FrameIdx,
    timeline_size: (u32, u32),
    fps: f64,
    provider: &mut dyn FrameProvider,
    extra_offset: [f32; 2],
    extra_opacity: f32,
) -> Result<Option<OwnedLayer>, ExportError> {
    let source_frame = clip.source_frame_at(frame);
    let mut transform = clip.effects.transform.value_at(source_frame);
    stabilize(project, clip, frame, timeline_size, &mut transform);
    clip.effects.animations.apply(
        &mut transform,
        (frame - clip.timeline_start) as f64 / fps.max(1.0),
        clip.timeline_len as f64 / fps.max(1.0),
        timeline_size.0 as f32,
    );
    transform.position[0] += extra_offset[0];
    transform.position[1] += extra_offset[1];
    let opacity = clip.fade_multiplier_at(frame) * extra_opacity;
    let content = clip_content(
        project,
        clip,
        held_timeline_frame(project, clip, frame),
        provider,
    )?;
    let layer = build_layer(
        project,
        clip,
        frame,
        source_frame,
        transform,
        opacity,
        content,
        timeline_size,
        fps as f32,
    );
    Ok(layer.map(|layer| {
        let passes = asset_passes(clip, frame, fps, 1.0);
        with_assets(layer, passes, provider, timeline_size)
    }))
}

/// The layers of an active crossing transition at `frame`: tail of `left`,
/// then head of `right` (in that order: `right` on top is what a dissolve
/// fades in).
fn crossing_layers(
    project: &Project,
    left: &Clip,
    right: &Clip,
    crossing: &vv_core::CrossTransition,
    frame: FrameIdx,
    timeline_size: (u32, u32),
    fps: f64,
    provider: &mut dyn FrameProvider,
) -> Result<Vec<OwnedLayer>, ExportError> {
    let progress = crossing.eased_progress_at(frame, left, right);
    if let Some(fx) = crossing.transition.asset.as_ref().filter(|fx| fx.enabled)
        && let Some(layers) = asset_transition_layers(
            project,
            (left, right),
            crossing,
            fx,
            frame,
            progress,
            timeline_size,
            fps,
            provider,
        )?
    {
        return Ok(layers);
    }
    if crossing.transition.kind == vv_core::TransitionKind::SmoothCut {
        return smooth_cut_layers(
            project,
            left,
            right,
            crossing,
            progress,
            timeline_size,
            fps,
            provider,
        );
    }
    let frame_size = (timeline_size.0 as f32, timeline_size.1 as f32);
    // The zoom of each clip at its own source frame: `offsets` needs it to
    // really clear the screen even if one of the two (or both) is zoomed
    // — see `push_clearance`. Recomputed here and again inside
    // `crossing_side_layer`: it costs very little (`Keyframed`), not worth
    // threading through as an extra parameter in several places.
    let left_zoom = left
        .effects
        .transform
        .value_at(left.source_frame_at(frame))
        .zoom;
    let right_zoom = right
        .effects
        .transform
        .value_at(right.source_frame_at(frame))
        .zoom;
    let (left_offset, right_offset) = crossing.offsets(progress, frame_size, left_zoom, right_zoom);
    let (left_opacity, right_opacity) = crossing.opacities(progress);
    let mut layers = Vec::with_capacity(2);
    layers.extend(crossing_side_layer(
        project,
        left,
        frame,
        timeline_size,
        fps,
        provider,
        left_offset,
        left_opacity,
    )?);
    layers.extend(crossing_side_layer(
        project,
        right,
        frame,
        timeline_size,
        fps,
        provider,
        right_offset,
        right_opacity,
    )?);
    Ok(layers)
}

/// An asset transition: both sides live (held at the edges of their media),
/// merged by its shader into one layer. `None` when the asset is unknown or
/// its shader is broken: the caller falls back on a plain dissolve. Nothing
/// until both sides are available.
fn asset_transition_layers(
    project: &Project,
    (left, right): (&Clip, &Clip),
    crossing: &vv_core::CrossTransition,
    fx: &vv_core::AssetFx,
    frame: FrameIdx,
    progress: f32,
    timeline_size: (u32, u32),
    fps: f64,
    provider: &mut dyn FrameProvider,
) -> Result<Option<Vec<OwnedLayer>>, ExportError> {
    let library = vv_render::assets::library();
    let Some(asset) = library.get(&fx.id) else {
        return Ok(None);
    };
    let window = crossing.window(left, right);
    let (Some(from), Some(to)) = (
        crossing_side_layer(
            project,
            left,
            frame,
            timeline_size,
            fps,
            provider,
            [0.0; 2],
            1.0,
        )?,
        crossing_side_layer(
            project,
            right,
            frame,
            timeline_size,
            fps,
            provider,
            [0.0; 2],
            1.0,
        )?,
    ) else {
        return Ok(Some(Vec::new()));
    };
    let pass = AssetPass {
        id: fx.id.clone(),
        params: asset.shader_params(&fx.params),
        progress,
        time: (frame - window.start) as f32 / fps.max(1.0) as f32,
        amount: 1.0,
    };
    let blend = from.blend;
    let Some(texture) = provider.asset_texture(&[pass], &[from], &[to], timeline_size) else {
        return Ok(None);
    };
    Ok(Some(vec![texture_layer(
        texture,
        timeline_size,
        1.0,
        blend,
    )]))
}

/// A Smooth Cut morphs the last frame before its window into the first
/// after it, both held for the whole window. Nothing until both are
/// available; a dissolve between them where the provider cannot morph.
fn smooth_cut_layers(
    project: &Project,
    left: &Clip,
    right: &Clip,
    crossing: &vv_core::CrossTransition,
    progress: f32,
    timeline_size: (u32, u32),
    fps: f64,
    provider: &mut dyn FrameProvider,
) -> Result<Vec<OwnedLayer>, ExportError> {
    let window = crossing.window(left, right);
    let side = |clip: &Clip, frame: FrameIdx, provider: &mut dyn FrameProvider| {
        crossing_side_layer(
            project,
            clip,
            frame,
            timeline_size,
            fps,
            provider,
            [0.0; 2],
            1.0,
        )
    };
    let (Some(from), Some(mut to)) = (
        side(left, window.start, provider)?,
        side(right, window.end - 1, provider)?,
    ) else {
        return Ok(Vec::new());
    };
    if let Some(texture) = provider.smooth_cut_texture(&from, &to, progress, timeline_size) {
        return Ok(vec![OwnedLayer {
            content: OwnedContent::Texture {
                texture,
                source_size: timeline_size,
            },
            transform: Transform::default(),
            opacity: 1.0,
            filters: Vec::new(),
            blend: from.blend,
            masks: Vec::new(),
            assets: Vec::new(),
        }]);
    }
    to.opacity *= progress;
    Ok(vec![from, to])
}

/// The layers of `clip` (on track `track_index`) at timeline frame
/// `frame`: only one in the normal case, but two if `frame` falls in the
/// window of a valid crossing transition involving this clip —
/// the other half of the pair adds itself, pushed according to the
/// shared transition. The entry point to prefer over `clip_layer`
/// wherever a whole track is composed (preview and export), not just
/// an isolated clip.
pub fn track_layers_at(
    project: &Project,
    timeline: &Timeline,
    track_index: usize,
    clip: &Clip,
    frame: FrameIdx,
    timeline_size: (u32, u32),
    provider: &mut dyn FrameProvider,
) -> Result<Vec<OwnedLayer>, ExportError> {
    let track = &timeline.tracks[track_index];
    let fps = timeline.fps.as_f64();
    let mut layers = if let Some((left, right, crossing)) = track.crossing_at(frame)
        && (left.id == clip.id || right.id == clip.id)
    {
        crossing_layers(
            project,
            left,
            right,
            crossing,
            frame,
            timeline_size,
            fps,
            provider,
        )?
    } else {
        clip_layer(project, clip, frame, timeline_size, fps, provider)?
            .into_iter()
            .collect()
    };
    if let Some(background) = canvas_layer(clip, frame, timeline_size, &layers) {
        layers.insert(0, background);
    }
    Ok(layers)
}

/// The clip's `Canvas` as a full-frame layer to go under `layers`.
fn canvas_layer(
    clip: &Clip,
    frame: FrameIdx,
    timeline_size: (u32, u32),
    layers: &[OwnedLayer],
) -> Option<OwnedLayer> {
    let canvas = clip.effects.canvas.as_ref()?;
    let opacity = clip.fade_multiplier_at(frame) * clip.transition_opacity_at(frame);
    let output = (timeline_size.0.max(1) as f32, timeline_size.1.max(1) as f32);
    let (content, transform, filters) = match canvas {
        vv_core::Canvas::Color(color) => {
            (OwnedContent::Solid(*color), Transform::default(), vec![])
        }
        vv_core::Canvas::Image(path) => {
            let frame = canvas_image(path)?;
            let size = (frame.width, frame.height);
            let transform = cover_transform(size, output, 0.0, 0.0);
            let content = OwnedContent::Video {
                frame,
                source_size: size,
                matte: None,
            };
            (content, transform, vec![])
        }
        vv_core::Canvas::Blur { radius } => {
            let (frame, source_size, clip_transform) =
                layers.iter().find_map(|l| match &l.content {
                    OwnedContent::Video {
                        frame, source_size, ..
                    } => Some((frame.clone(), *source_size, l.transform)),
                    _ => None,
                })?;
            let radius = radius * output.0.min(output.1) / 1080.0;
            let mut transform =
                cover_transform(source_size, output, clip_transform.rotation, radius);
            transform.flip = clip_transform.flip;
            let mut blur = vv_core::FilterValue::new(vv_core::FilterKind::GaussianBlur);
            blur.radius = radius.min(vv_core::BLUR_RADIUS_MAX);
            let content = OwnedContent::Video {
                frame,
                source_size,
                matte: None,
            };
            (content, transform, vec![blur])
        }
    };
    Some(OwnedLayer {
        content,
        transform,
        opacity,
        filters,
        blend: vv_core::BlendMode::default(),
        masks: Vec::new(),
        assets: Vec::new(),
    })
}

/// Zooms a `source` fitted into `output` until it covers it once turned by
/// `rotation`, plus `bleed` pixels per side for a blur's soft edge.
fn cover_transform(source: (u32, u32), output: (f32, f32), rotation: f32, bleed: f32) -> Transform {
    let source = (source.0.max(1) as f32, source.1.max(1) as f32);
    let scale = (output.0 / source.0).min(output.1 / source.1);
    let (mut fitted_w, mut fitted_h) = (source.0 * scale, source.1 * scale);
    let quarter_turns = (rotation / 90.0).round() as i32;
    if quarter_turns.rem_euclid(2) == 1 {
        std::mem::swap(&mut fitted_w, &mut fitted_h);
    }
    let zoom = ((output.0 + 2.0 * bleed) / fitted_w).max((output.1 + 2.0 * bleed) / fitted_h);
    Transform {
        zoom: [zoom, zoom],
        rotation: quarter_turns as f32 * 90.0,
        ..Transform::default()
    }
}

/// Canvas pictures, decoded once per path; `None` for one that does not open.
fn canvas_image(path: &std::path::Path) -> Option<Arc<FrameYuv420>> {
    static IMAGES: std::sync::LazyLock<
        std::sync::Mutex<std::collections::HashMap<std::path::PathBuf, Option<Arc<FrameYuv420>>>>,
    > = std::sync::LazyLock::new(Default::default);
    let mut images = IMAGES.lock().unwrap_or_else(|e| e.into_inner());
    images
        .entry(path.to_path_buf())
        .or_insert_with(|| {
            let mut decoder = vv_media::decode::Decoder::open(path).ok()?;
            decoder.next_frame().ok().flatten().map(|(_, frame)| frame)
        })
        .clone()
}

/// Decodes a canvas picture ahead, so the first frame showing it does not wait.
pub fn preload_canvas_image(path: &std::path::Path) {
    canvas_image(path);
}

#[cfg(test)]
#[path = "tests/frame_provider.rs"]
mod tests;
