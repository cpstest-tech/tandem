//! Export: walks the timeline and writes H.264+AAC with the same layers and
//! the same mix as the preview. Runs on a dedicated thread, on a
//! snapshot of the project.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use vv_core::{Clip, ClipId, FrameIdx, Project, Timeline, TimelineId};

use vv_audio::mixer::{PROJECT_SAMPLE_RATE, timeline_frame_to_sample};

use crate::audio_render::{LevelCache, render_offline};

use crate::frame_provider::{
    FrameProvider, GpuCompounds, OwnedLayer, media_source_frame, stack_layers, track_layers_at,
};

pub const PROJECT_CHANNELS: u16 = 2;
const RENDER_AHEAD_FRAMES: usize = 8;
/// Mixed audio chunks (a few seconds each) ready ahead of the encoder.
const AUDIO_AHEAD_CHUNKS: usize = 2;
const PREVIEW_EVERY: std::time::Duration = std::time::Duration::from_millis(400);
const PREVIEW_WIDTH: usize = 360;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AspectMode {
    #[default]
    Fit,
    Fill,
    Stretch,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportSettings {
    pub output_path: PathBuf,
    /// Percentage of the timeline resolution.
    pub scale_percent: u32,
    pub custom_size: Option<(u32, u32)>,
    pub aspect: AspectMode,
    pub video: vv_media::VideoSettings,
    /// `None` = export without audio.
    pub audio: Option<vv_media::AudioSettings>,
    /// GPU decoders to try, in order; empty decodes on the CPU.
    pub hw_decode: Vec<vv_media::HwDevice>,
    /// Output frame rate; `None` keeps the timeline's.
    pub fps: Option<vv_core::Rational>,
    /// Off: only the audio file is written.
    pub video_enabled: bool,
    /// The audio as a separate file next to the video.
    pub audio_file: Option<AudioFileExport>,
    pub motion_blur: Option<Box<crate::motion_blur::MotionBlurSettings>>,
    /// Which dub plays (see `vv_core::dub::switch`).
    pub language: vv_core::dub::Language,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioFileExport {
    pub format: vv_media::AudioFileFormat,
    pub bitrate_kbps: u32,
}

impl ExportSettings {
    pub const SCALE_CHOICES: [u32; 4] = [100, 75, 50, 25];
    pub const MIN_DIMENSION: u32 = 2;

    pub fn new(output_path: PathBuf) -> Self {
        Self {
            output_path,
            scale_percent: 100,
            custom_size: None,
            aspect: AspectMode::Fit,
            video: vv_media::VideoSettings::default(),
            audio: Some(vv_media::AudioSettings::default()),
            hw_decode: Vec::new(),
            fps: None,
            video_enabled: true,
            audio_file: None,
            motion_blur: None,
            language: vv_core::dub::Language::AsIs,
        }
    }

    /// Where the separate audio file goes: next to the video, same name.
    pub fn audio_file_path(&self) -> Option<PathBuf> {
        let file = self.audio_file.as_ref()?;
        Some(self.output_path.with_extension(file.format.extension()))
    }

    pub fn output_fps(&self, timeline_fps: vv_core::Rational) -> vv_core::Rational {
        self.fps.unwrap_or(timeline_fps)
    }

    /// Like `new`, but with the fastest encoders available on this
    /// machine (NVENC, FDK). `new` stays deterministic for the tests.
    /// Blocks on the NVENC check, up to a second the first time.
    pub fn preferred(output_path: PathBuf) -> Self {
        let mut settings = Self::with_preferred_audio(output_path);
        if vv_media::VideoCodec::Nvenc.is_available() {
            settings.video = vv_media::VideoSettings::for_codec(vv_media::VideoCodec::Nvenc);
        }
        settings
    }

    /// `preferred` but for the video encoder, left to x264.
    pub fn with_preferred_audio(output_path: PathBuf) -> Self {
        let mut settings = Self::new(output_path);
        if vv_media::AudioCodec::FdkAac.is_available()
            && let Some(audio) = &mut settings.audio
        {
            audio.codec = vv_media::AudioCodec::FdkAac;
        }
        settings
    }

    /// Reduced to even dimensions (required by the encoders' 4:2:0);
    /// at 100% it stays exactly the timeline's.
    pub fn output_size(&self, timeline_size: (u32, u32)) -> (u32, u32) {
        if let Some(size) = self.custom_size {
            return size;
        }
        if self.scale_percent >= 100 {
            return timeline_size;
        }
        let scale = |v: u32| ((v * self.scale_percent / 100) & !1).max(Self::MIN_DIMENSION);
        (scale(timeline_size.0), scale(timeline_size.1))
    }

    pub fn valid_output_size(&self, timeline_size: (u32, u32)) -> bool {
        let (width, height) = self.output_size(timeline_size);
        [width, height]
            .into_iter()
            .all(|value| value >= Self::MIN_DIMENSION && value % 2 == 0)
    }

    fn needs_timeline_texture(&self, timeline_size: (u32, u32)) -> bool {
        let (width, height) = self.output_size(timeline_size);
        self.custom_size.is_some()
            && u64::from(width) * u64::from(timeline_size.1)
                != u64::from(height) * u64::from(timeline_size.0)
    }

    fn valid_render_size(&self, timeline_size: (u32, u32), texture_limit: u32) -> bool {
        let fits = |(width, height)| width <= texture_limit && height <= texture_limit;
        self.valid_output_size(timeline_size)
            && fits(self.output_size(timeline_size))
            && (!self.needs_timeline_texture(timeline_size) || fits(timeline_size))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    TimelineNotFound,
    MediaNotFound,
    Cancelled,
    /// Decoding, composition or encoding failed.
    Failed(String),
}

impl ExportError {
    pub(crate) fn failed(e: impl std::fmt::Display) -> Self {
        Self::Failed(e.to_string())
    }
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimelineNotFound => f.write_str("timeline not found"),
            Self::MediaNotFound => f.write_str("media not found"),
            Self::Cancelled => f.write_str("export cancelled"),
            Self::Failed(e) => f.write_str(e),
        }
    }
}

#[derive(Default)]
pub struct ExportProgress {
    pub current_frame: FrameIdx,
    pub total_frames: FrameIdx,
    pub done: bool,
    pub error: Option<ExportError>,
    /// Updated along with `current_frame`, not only at the end.
    pub elapsed: std::time::Duration,
    pub decode: StageStats,
    pub compose: StageStats,
    pub encode: StageStats,
    pub output_path: PathBuf,
    /// Decode paths used so far, `None` being the CPU. Still images are left
    /// out: decoded once, they say nothing about the speed.
    pub decoders: Vec<Option<vv_media::HwDevice>>,
    /// GPU adapter of the composition.
    pub compositor: Option<String>,
    pub encoder: Option<vv_media::VideoCodec>,
    /// Until the first frame is written.
    pub startup: Option<std::time::Duration>,
    /// After the last frame is written: encoder flush and audio tail.
    pub finalize: Option<std::time::Duration>,
    /// A recent output frame, small.
    pub preview: Option<PreviewFrame>,
    /// Changes with `preview`, so a reader copies it only when new.
    pub preview_version: u64,
}

/// An RGBA picture for the export window.
#[derive(Clone)]
pub struct PreviewFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl PreviewFrame {
    /// Nearest-neighbour thumbnail of an I420 frame (BT.709, limited range).
    pub fn from_i420(i420: &[u8], width: usize, height: usize, max_width: usize) -> Self {
        let step = width.div_ceil(max_width.max(1)).max(1);
        let (out_w, out_h) = (width / step, height / step);
        let chroma_w = width.div_ceil(2);
        let chroma_h = height.div_ceil(2);
        let (luma, chroma) = i420.split_at(width * height);
        let (u_plane, v_plane) = chroma.split_at(chroma_w * chroma_h);
        let mut rgba = Vec::with_capacity(out_w * out_h * 4);
        for y in 0..out_h {
            for x in 0..out_w {
                let (sx, sy) = (x * step, y * step);
                let luma = (f32::from(luma[sy * width + sx]) - 16.0) * 1.1644;
                let ci = (sy / 2) * chroma_w + sx / 2;
                let u = f32::from(u_plane[ci]) - 128.0;
                let v = f32::from(v_plane[ci]) - 128.0;
                let r = luma + 1.7927 * v;
                let g = luma - 0.2132 * u - 0.5329 * v;
                let b = luma + 2.1124 * u;
                rgba.extend([r, g, b].map(|c| c.clamp(0.0, 255.0) as u8));
                rgba.push(255);
            }
        }
        Self {
            width: out_w as u32,
            height: out_h as u32,
            rgba,
        }
    }
}

impl ExportProgress {
    pub fn fps(&self) -> Option<f64> {
        rate(self.current_frame, self.elapsed)
    }

    /// Share of the elapsed time `stage` spent working; the rest it waited
    /// on the other stages.
    pub fn busy_share(&self, stage: &StageStats) -> Option<f64> {
        (!self.elapsed.is_zero())
            .then(|| (stage.busy.as_secs_f64() / self.elapsed.as_secs_f64()).min(1.0))
    }
}

/// Time a pipeline stage spent working, without the waits on the other
/// stages: its fps is how fast it would go alone, so the slowest one is the
/// bottleneck.
#[derive(Default, Clone, Copy)]
pub struct StageStats {
    pub frames: FrameIdx,
    pub busy: std::time::Duration,
}

impl StageStats {
    pub fn fps(&self) -> Option<f64> {
        rate(self.frames, self.busy)
    }

    fn add(&mut self, busy: std::time::Duration) {
        self.frames += 1;
        self.busy += busy;
    }
}

fn rate(frames: FrameIdx, time: std::time::Duration) -> Option<f64> {
    (frames > 0 && !time.is_zero()).then(|| frames as f64 / time.as_secs_f64())
}

/// Decoder kept open for the active video clip, with a seek/reopen only
/// when the clip changes (not a new decoder for every output frame:
/// every seek is a flush to a keyframe, too slow to do on every frame).
struct ActiveClipDecoder {
    decoder: vv_media::Decoder,
    /// A conformed clip asks for the same source frame on consecutive
    /// timeline frames, and the decoder does not go back.
    last: Option<(FrameIdx, Arc<vv_media::FrameYuv420>)>,
    last_target: Option<FrameIdx>,
    /// Frames decoded ahead of a backward request (reversed clips).
    backward: std::collections::BTreeMap<FrameIdx, Arc<vv_media::FrameYuv420>>,
}

/// Source frames decoded per backward step: each step costs one seek.
const BACKWARD_BLOCK: FrameIdx = 32;

impl ActiveClipDecoder {
    fn open_for(
        path: &Path,
        target_source_frame: FrameIdx,
        is_image: bool,
        hw: &[vv_media::HwDevice],
    ) -> Result<Self, ExportError> {
        // `Decoder::open` on an image would hit EOF after the first frame.
        let mut decoder = if is_image {
            vv_media::Decoder::open_image(path)
        } else {
            vv_media::Decoder::open_with(path, hw, vv_media::HwPriority::Normal)
        }
        .map_err(ExportError::failed)?;
        let secs = target_source_frame as f64 / decoder.fps().as_f64().max(1e-9);
        decoder.seek_to_time(secs).map_err(ExportError::failed)?;
        let mut me = Self {
            decoder,
            last: None,
            last_target: None,
            backward: Default::default(),
        };
        me.advance_to(target_source_frame)?;
        Ok(me)
    }

    /// Decodes forward up to `target`; if already reached it returns the last
    /// frame again. `None` at the end of the stream.
    fn advance_to(
        &mut self,
        target: FrameIdx,
    ) -> Result<Option<Arc<vv_media::FrameYuv420>>, ExportError> {
        let going_back = self.last_target.is_some_and(|last| target < last);
        self.last_target = Some(target);
        if let Some(frame) = self.backward.get(&target) {
            return Ok(Some(frame.clone()));
        }
        if going_back {
            return self.decode_block_ending_at(target);
        }
        self.backward.clear();
        if let Some((idx, frame)) = &self.last
            && *idx >= target
        {
            return Ok(Some(frame.clone()));
        }
        loop {
            match self.decoder.next_frame().map_err(ExportError::failed)? {
                Some((idx, frame)) if idx >= target => {
                    self.last = Some((idx, frame.clone()));
                    return Ok(Some(frame));
                }
                Some(_) => continue,
                None => return Ok(None),
            }
        }
    }
}

impl ActiveClipDecoder {
    /// Seeks before `target` and keeps the block of frames up to it, so the
    /// following backward requests are served without seeking again.
    fn decode_block_ending_at(
        &mut self,
        target: FrameIdx,
    ) -> Result<Option<Arc<vv_media::FrameYuv420>>, ExportError> {
        self.backward.clear();
        let start = (target - BACKWARD_BLOCK + 1).max(0);
        let secs = start as f64 / self.decoder.fps().as_f64().max(1e-9);
        self.decoder
            .seek_to_time(secs)
            .map_err(ExportError::failed)?;
        let mut found = None;
        while let Some((idx, frame)) = self.decoder.next_frame().map_err(ExportError::failed)? {
            if idx >= start {
                self.backward.insert(idx, frame.clone());
            }
            self.last = Some((idx, frame.clone()));
            if idx >= target {
                found = Some(frame);
                break;
            }
        }
        // Past the end of the stream: the last frame decoded.
        Ok(found.or_else(|| self.backward.values().next_back().cloned()))
    }
}

/// Decoders kept open from one frame to the next, reopened only when the
/// clip changes.
#[derive(Default)]
struct StreamingFrameProvider {
    /// One per clip: several tracks can be active on the same frame.
    /// Pruned by `sweep`.
    active: HashMap<ClipId, ActiveClipDecoder>,
    /// The clips asked for since the last `sweep`.
    touched: HashSet<ClipId>,
    hw: Vec<vv_media::HwDevice>,
    /// As `ExportProgress::decoders`.
    used: Vec<Option<vv_media::HwDevice>>,
}

impl StreamingFrameProvider {
    /// Closes the decoders of the clips not asked for since the last call:
    /// whatever `track_layers_at` decoded, nested timelines and crossings
    /// included, stays open for the next frame.
    fn sweep(&mut self) {
        let touched = std::mem::take(&mut self.touched);
        self.active.retain(|id, _| touched.contains(id));
    }
}

impl FrameProvider for StreamingFrameProvider {
    fn frame_for(
        &mut self,
        project: &Project,
        clip: &Clip,
        timeline_frame: FrameIdx,
    ) -> Result<Option<Arc<vv_media::FrameYuv420>>, ExportError> {
        self.touched.insert(clip.id);
        let Some((media_id, source_frame)) = media_source_frame(clip, timeline_frame) else {
            self.active.remove(&clip.id);
            return Ok(None);
        };
        let item = project
            .media_pool
            .get(media_id)
            .ok_or(ExportError::MediaNotFound)?;

        // A compound clip is not decoded: `GpuCompounds` composes it.
        // Getting here means it stopped at the nesting limit —
        // no layer for this clip, as for a missing media.
        if item.compound.is_some() {
            return Ok(None);
        }

        let path = item.path.clone();
        let is_image = item.meta.is_image();

        let decoder = match self.active.entry(clip.id) {
            Entry::Occupied(e) => e.into_mut(),
            Entry::Vacant(e) => e.insert(ActiveClipDecoder::open_for(
                &path,
                source_frame,
                is_image,
                &self.hw,
            )?),
        };
        let frame = decoder.advance_to(source_frame)?;
        let device = decoder.decoder.hw_device();
        if !is_image && !self.used.iter().any(|used| used.as_ref() == device) {
            self.used.push(device.cloned());
        }
        Ok(frame)
    }
}

/// An export of `timeline` started during a first plugin scan would play
/// its enabled plugins as pass-through: the installed list is incomplete.
/// Video-only exports and timelines without plugins never wait.
pub fn scan_blocks_export(
    scanning: bool,
    project: &Project,
    timeline: TimelineId,
    audio: bool,
) -> bool {
    scanning
        && audio
        && project
            .timelines
            .get(timeline)
            .is_some_and(Timeline::has_audio_clips)
        && project.timeline_uses_enabled_plugins(timeline)
}

/// Exports the frames in `range` to `output_path` and, if asked, the audio
/// to its own file. Blocking. A cancelled or failed export leaves none of
/// the files it started.
pub fn export_timeline(
    project: &Project,
    timeline_id: TimelineId,
    settings: &ExportSettings,
    range: std::ops::Range<FrameIdx>,
    progress: &Mutex<ExportProgress>,
    cancel: &AtomicBool,
) -> Result<(), ExportError> {
    let mut created = Vec::new();
    let result = export_files(
        project,
        timeline_id,
        settings,
        range,
        progress,
        cancel,
        &mut created,
    );
    if result.is_err() {
        for path in created {
            let _ = std::fs::remove_file(path);
        }
    }
    result
}

fn export_files(
    project: &Project,
    timeline_id: TimelineId,
    settings: &ExportSettings,
    range: std::ops::Range<FrameIdx>,
    progress: &Mutex<ExportProgress>,
    cancel: &AtomicBool,
    created: &mut Vec<PathBuf>,
) -> Result<(), ExportError> {
    if settings.video_enabled {
        let retimed = retimed_to_output(project, timeline_id, settings, range.clone());
        let (project, settings, video_range) = match &retimed {
            Some((project, settings, range)) => (project, settings, range.clone()),
            None => (project, settings, range.clone()),
        };
        export_video(
            project,
            timeline_id,
            settings,
            video_range,
            progress,
            cancel,
            created,
        )?;
    }
    if let Some(path) = settings.audio_file_path()
        && let Some(file) = &settings.audio_file
    {
        export_audio_file(
            project,
            timeline_id,
            range,
            file,
            &path,
            settings.video_enabled,
            progress,
            cancel,
            created,
        )?;
    }
    progress.lock().unwrap().done = true;
    Ok(())
}

/// Above the timeline's rate, output frames picked from the timeline grid
/// repeat each frame and the motion stays at the timeline's rate: a copy
/// retimed to the output rate renders every output frame on its own.
fn retimed_to_output(
    project: &Project,
    timeline_id: TimelineId,
    settings: &ExportSettings,
    range: std::ops::Range<FrameIdx>,
) -> Option<(Project, ExportSettings, std::ops::Range<FrameIdx>)> {
    use vv_core::Command;
    let from = project.timelines.get(timeline_id)?.fps;
    let to = settings.fps.filter(|to| to.as_f64() > from.as_f64())?;
    let mut project = project.clone();
    vv_core::SetTimelineFps::new(timeline_id, to).apply(&mut project);
    let settings = ExportSettings {
        fps: None,
        ..settings.clone()
    };
    let convert = |frame| vv_core::convert_frames(frame, from, to);
    Some((project, settings, convert(range.start)..convert(range.end)))
}

/// The timeline frame shown by output frame `index`.
fn source_frame(
    index: FrameIdx,
    start: FrameIdx,
    end: FrameIdx,
    timeline_fps: vv_core::Rational,
    output_fps: vv_core::Rational,
) -> FrameIdx {
    if timeline_fps == output_fps {
        return start + index;
    }
    let num = i128::from(index) * i128::from(timeline_fps.num) * i128::from(output_fps.den);
    let den = i128::from(timeline_fps.den) * i128::from(output_fps.num);
    (start + (num / den.max(1)) as FrameIdx).min(end - 1)
}

/// Output frames covering `frames` timeline frames.
fn output_frame_count(
    frames: FrameIdx,
    timeline_fps: vv_core::Rational,
    output_fps: vv_core::Rational,
) -> FrameIdx {
    if timeline_fps == output_fps {
        return frames;
    }
    let num = i128::from(frames) * i128::from(timeline_fps.den) * i128::from(output_fps.num);
    let den = i128::from(timeline_fps.num) * i128::from(output_fps.den);
    ((num + den - 1) / den.max(1)) as FrameIdx
}

#[allow(clippy::too_many_arguments)]
fn export_video(
    project: &Project,
    timeline_id: TimelineId,
    settings: &ExportSettings,
    range: std::ops::Range<FrameIdx>,
    progress: &Mutex<ExportProgress>,
    cancel: &AtomicBool,
    created: &mut Vec<PathBuf>,
) -> Result<(), ExportError> {
    let started = std::time::Instant::now();
    let finish = || {
        let mut p = progress.lock().unwrap();
        let now = started.elapsed();
        if p.current_frame > 0 {
            p.finalize = Some(now.saturating_sub(p.elapsed));
        }
        p.elapsed = now;
    };
    let timeline = project
        .timelines
        .get(timeline_id)
        .ok_or(ExportError::TimelineNotFound)?;

    if !settings.valid_output_size(timeline.resolution) {
        return Err(ExportError::failed(format!(
            "output dimensions must be even and at least {} pixels",
            ExportSettings::MIN_DIMENSION,
        )));
    }

    let range = range.start.max(0)..range.end.min(timeline.total_frames());
    let output_fps = settings.output_fps(timeline.fps);
    let retimed = output_fps != timeline.fps;
    let total_frames =
        output_frame_count((range.end - range.start).max(0), timeline.fps, output_fps);
    progress.lock().unwrap().total_frames = total_frames;
    if total_frames <= 0 {
        finish();
        return Ok(());
    }

    let audio_settings = settings
        .audio
        .as_ref()
        .filter(|_| timeline.has_audio_clips());
    let has_audio_track = audio_settings.is_some();

    let (out_w, out_h) = settings.output_size(timeline.resolution);
    // Compound decode and composition must share the same GPU device.
    let compositor = vv_render::Compositor::new_headless_with_precision(project.precision);
    let texture_limit = compositor.device().limits().max_texture_dimension_2d;
    if !settings.valid_render_size(timeline.resolution, texture_limit) {
        return Err(ExportError::failed(format!(
            "output or intermediate dimensions exceed the GPU's {texture_limit}-pixel texture limit"
        )));
    }
    created.push(settings.output_path.clone());
    let mut encoder = vv_media::Encoder::new(
        &settings.output_path,
        out_w,
        out_h,
        output_fps,
        &settings.video,
        audio_settings.map(|a| (PROJECT_SAMPLE_RATE, PROJECT_CHANNELS, a)),
    )
    .map_err(|e| {
        // Nothing was written if the encoder did not open: an old file stays.
        created.pop();
        ExportError::failed(e)
    })?;

    // Decode, GPU composition and encode on three threads: in series each one
    // waited for the others and none saturated the machine.
    let (decoded_tx, decoded_rx) =
        std::sync::mpsc::sync_channel::<Result<Vec<OwnedLayer>, ExportError>>(RENDER_AHEAD_FRAMES);
    let (composed_tx, composed_rx) =
        std::sync::mpsc::sync_channel::<Result<Vec<u8>, ExportError>>(RENDER_AHEAD_FRAMES);
    let resolution = timeline.resolution;
    let output = vv_render::OutputFrame::scaled(out_w, out_h, resolution);
    {
        let mut p = progress.lock().unwrap();
        p.output_path = settings.output_path.clone();
        p.compositor = Some(compositor.adapter_name().to_owned());
        p.encoder = Some(settings.video.codec);
    }
    let compositor = &compositor;
    std::thread::scope(|scope| {
        let (audio_tx, audio_rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(AUDIO_AHEAD_CHUNKS);
        let audio_mix = has_audio_track.then(|| {
            let samples = audio_samples(timeline, range.clone());
            // Stops once the encoder side is gone.
            scope.spawn(move || {
                let levels = &mut LevelCache::on_disk();
                render_offline(project, timeline, samples, levels, |chunk| {
                    match audio_tx.send(chunk.to_vec()) {
                        Ok(()) => ControlFlow::Continue(()),
                        Err(_) => ControlFlow::Break(()),
                    }
                })
            })
        });

        let decode_range = range.clone();
        let timeline_fps = timeline.fps;
        scope.spawn(move || {
            let mut provider = StreamingFrameProvider {
                hw: settings.hw_decode.clone(),
                ..Default::default()
            };
            // Motion blur reads every timeline frame, not one per output frame.
            let frames: Box<dyn Iterator<Item = FrameIdx>> = if settings.motion_blur.is_some() {
                Box::new(decode_range.clone())
            } else {
                Box::new((0..total_frames).map(|index| {
                    source_frame(
                        index,
                        decode_range.start,
                        decode_range.end,
                        timeline_fps,
                        output_fps,
                    )
                }))
            };
            for frame in frames {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                let t = std::time::Instant::now();
                let decoded = decode_video_frame(
                    project,
                    timeline,
                    &mut provider,
                    compositor,
                    frame,
                    resolution,
                );
                let mut p = progress.lock().unwrap();
                p.decode.add(t.elapsed());
                if p.decoders != provider.used {
                    p.decoders.clone_from(&provider.used);
                }
                drop(p);
                let failed = decoded.is_err();
                // `send` fails only if the next stage has already stopped.
                if decoded_tx.send(decoded).is_err() || failed {
                    return;
                }
            }
        });

        let source_frames = (range.end - range.start).max(0) as usize;
        scope.spawn(move || {
            let mut blur = settings.motion_blur.as_ref().map(|blur| {
                crate::motion_blur::Engine::new(
                    compositor,
                    blur,
                    timeline_fps,
                    output_fps,
                    source_frames,
                    total_frames,
                    (out_w, out_h),
                )
            });
            for decoded in decoded_rx {
                let t = std::time::Instant::now();
                let composed: Vec<Result<Vec<u8>, ExportError>> =
                    match (decoded, &mut blur) {
                        (Ok(layers), Some(blur)) => blur
                            .push(compose_export_texture(
                                compositor, &layers, output, settings,
                            ))
                            .into_iter()
                            .map(Ok)
                            .collect(),
                        (decoded, _) => vec![decoded.map(|layers| {
                            compose_export_frame(compositor, &layers, output, settings)
                        })],
                    };
                progress.lock().unwrap().compose.add(t.elapsed());
                for composed in composed {
                    let failed = composed.is_err();
                    if composed_tx.send(composed).is_err() || failed {
                        return;
                    }
                }
            }
            if let Some(blur) = &mut blur {
                if cancel.load(Ordering::Relaxed) {
                    return;
                }
                for frame in blur.finish() {
                    if composed_tx.send(Ok(frame)).is_err() {
                        return;
                    }
                }
            }
        });

        // Inside the closure: if the encoder exits with an error `composed_rx` must
        // be closed before the join, or the upstream stages stay on `send`.
        let composed_rx = composed_rx;
        // The audio must be written along with the video: all of it after the video and
        // the muxer keeps the whole video in RAM and the interleaving becomes
        // quadratic (minutes on a timeline of a few minutes).
        let mut audio = audio_mix.is_some().then(|| {
            AudioInterleaver::new(
                timeline,
                range.clone(),
                retimed.then_some(output_fps),
                audio_rx,
            )
        });
        let mut last_preview = std::time::Instant::now() - PREVIEW_EVERY;
        let encoded = (0..total_frames).try_for_each(|index| {
            if cancel.load(Ordering::Relaxed) {
                return Err(ExportError::Cancelled);
            }
            let frame_i420 = match composed_rx.recv() {
                Ok(frame_i420) => frame_i420?,
                Err(_) => return Err(ExportError::Cancelled),
            };
            let t = std::time::Instant::now();
            encoder
                .write_video_frame(&frame_i420)
                .map_err(ExportError::failed)?;
            if let Some(audio) = &mut audio {
                audio.write_until(&mut encoder, index + 1)?;
            }

            let preview = (last_preview.elapsed() >= PREVIEW_EVERY).then(|| {
                last_preview = std::time::Instant::now();
                PreviewFrame::from_i420(&frame_i420, out_w as usize, out_h as usize, PREVIEW_WIDTH)
            });
            let mut p = progress.lock().unwrap();
            p.encode.add(t.elapsed());
            p.current_frame = index + 1;
            p.elapsed = started.elapsed();
            if preview.is_some() {
                p.preview = preview;
                p.preview_version += 1;
            }
            if p.startup.is_none() {
                p.startup = Some(p.elapsed);
            }
            Ok(())
        });
        drop(audio);
        let mixed = match audio_mix {
            Some(handle) => handle.join().expect("audio mix thread panicked"),
            None => Ok(()),
        };
        // A mix that failed stops the encoder too: its error is the cause.
        mixed.and(encoded)
    })?;

    encoder.finish().map_err(ExportError::failed)?;
    finish();
    Ok(())
}

#[cfg(test)]
fn render_video_frame(
    project: &Project,
    timeline: &Timeline,
    compositor: &vv_render::Compositor,
    provider: &mut StreamingFrameProvider,
    frame: FrameIdx,
    resolution: (u32, u32),
) -> Result<Vec<u8>, ExportError> {
    let layers = decode_video_frame(project, timeline, provider, compositor, frame, resolution)?;
    let output = vv_render::OutputFrame::exact(resolution.0, resolution.1);
    Ok(compose_video_frame(compositor, &layers, output))
}

/// One timeline frame, decoded and composed as the export does it, in RGBA8
/// at `size` (the timeline's aspect ratio). Frame-accurate: it decodes the
/// frame itself, nothing comes from a cache.
pub fn render_frame_rgba(
    project: &Project,
    timeline_id: TimelineId,
    frame: FrameIdx,
    size: (u32, u32),
    compositor: &vv_render::Compositor,
) -> Result<Vec<u8>, ExportError> {
    let timeline = project
        .timelines
        .get(timeline_id)
        .ok_or(ExportError::TimelineNotFound)?;
    let mut provider = StreamingFrameProvider::default();
    let layers = decode_video_frame(
        project,
        timeline,
        &mut provider,
        compositor,
        frame,
        timeline.resolution,
    )?;
    let layers: Vec<vv_render::Layer> = layers.iter().map(OwnedLayer::as_render).collect();
    let output = vv_render::OutputFrame::scaled(size.0, size.1, timeline.resolution);
    Ok(compositor.render_layers(&layers, output))
}

/// The layers of the frame, from bottom to top. A missing media frame
/// (past the real end of the file) leaves out only that layer.
fn decode_video_frame(
    project: &Project,
    timeline: &Timeline,
    provider: &mut StreamingFrameProvider,
    compositor: &vv_render::Compositor,
    frame: FrameIdx,
    resolution: (u32, u32),
) -> Result<Vec<OwnedLayer>, ExportError> {
    let clips = timeline.active_video_clips_at(frame);
    let mut gpu = GpuCompounds::new(provider, compositor);
    let mut groups = Vec::with_capacity(clips.len());
    for (track_index, clip) in clips {
        let layers = track_layers_at(
            project,
            timeline,
            track_index,
            clip,
            frame,
            resolution,
            &mut gpu,
        )?;
        groups.push((clip.effects.matte, layers));
    }
    let layers = stack_layers(groups, &mut gpu, resolution);
    provider.sweep();
    Ok(layers)
}

fn compose_video_frame(
    compositor: &vv_render::Compositor,
    layers: &[OwnedLayer],
    output: vv_render::OutputFrame,
) -> Vec<u8> {
    let layers: Vec<vv_render::Layer> = layers.iter().map(OwnedLayer::as_render).collect();
    compositor.render_layers_i420(&layers, output)
}

fn compose_export_frame(
    compositor: &vv_render::Compositor,
    layers: &[OwnedLayer],
    output: vv_render::OutputFrame,
    settings: &ExportSettings,
) -> Vec<u8> {
    if !settings.needs_timeline_texture(output.timeline_size) {
        return compose_video_frame(compositor, layers, output);
    }
    let texture = compose_timeline_texture(compositor, layers, output.timeline_size);
    compositor.render_layers_i420(&[resized_timeline(&texture, output, settings)], output)
}

/// Like `compose_export_frame`, but kept on the GPU at the output size.
fn compose_export_texture(
    compositor: &vv_render::Compositor,
    layers: &[OwnedLayer],
    output: vv_render::OutputFrame,
    settings: &ExportSettings,
) -> vv_render::PooledTexture {
    if !settings.needs_timeline_texture(output.timeline_size) {
        let layers: Vec<_> = layers.iter().map(OwnedLayer::as_render).collect();
        return compositor.render_layers_to_owned_texture_transparent(&layers, output);
    }
    let texture = compose_timeline_texture(compositor, layers, output.timeline_size);
    compositor.render_layers_to_owned_texture_transparent(
        &[resized_timeline(&texture, output, settings)],
        output,
    )
}

// Resize the completed timeline so clip positions, masks and effects keep their geometry.
fn compose_timeline_texture(
    compositor: &vv_render::Compositor,
    layers: &[OwnedLayer],
    (width, height): (u32, u32),
) -> vv_render::PooledTexture {
    let layers: Vec<_> = layers.iter().map(OwnedLayer::as_render).collect();
    compositor.render_layers_to_owned_texture_transparent(
        &layers,
        vv_render::OutputFrame::exact(width, height),
    )
}

fn resized_timeline<'a>(
    texture: &'a vv_render::PooledTexture,
    output: vv_render::OutputFrame,
    settings: &ExportSettings,
) -> vv_render::Layer<'a> {
    let (width, height) = output.timeline_size;
    let sx = output.width as f32 / width.max(1) as f32;
    let sy = output.height as f32 / height.max(1) as f32;
    let scale = sx.min(sy);
    let zoom = match settings.aspect {
        AspectMode::Fit => [1.0; 2],
        AspectMode::Fill => [sx.max(sy) / scale; 2],
        AspectMode::Stretch => [sx / scale, sy / scale],
    };
    vv_render::Layer::new(
        vv_render::LayerContent::Texture {
            texture,
            source_size: (width, height),
        },
        vv_core::Transform {
            zoom,
            ..Default::default()
        },
    )
}

/// Writes the audio mix to the encoder as it arrives, aligned to the video
/// frames: all of it after the video and the muxer would keep the whole
/// video in RAM.
struct AudioInterleaver {
    chunks: std::sync::mpsc::Receiver<Vec<f32>>,
    chunk: Vec<f32>,
    /// Of `chunk`, already written.
    taken: usize,
    written: usize,
    fps: f64,
    start_frame: FrameIdx,
    start_sample: u64,
    /// Samples in the mix: the end of the last frame never asks for more.
    total: usize,
    /// Set when the output frame rate is not the timeline's.
    output_fps: Option<f64>,
}

impl AudioInterleaver {
    fn new(
        timeline: &Timeline,
        range: std::ops::Range<FrameIdx>,
        output_fps: Option<vv_core::Rational>,
        chunks: std::sync::mpsc::Receiver<Vec<f32>>,
    ) -> Self {
        let fps = timeline.fps.as_f64();
        let samples = audio_samples(timeline, range.clone());
        Self {
            chunks,
            chunk: Vec::new(),
            taken: 0,
            written: 0,
            fps,
            start_frame: range.start,
            start_sample: timeline_frame_to_sample(range.start, fps, PROJECT_SAMPLE_RATE),
            total: (samples.end - samples.start) as usize * PROJECT_CHANNELS as usize,
            output_fps: output_fps.map(|f| f.as_f64()),
        }
    }

    /// Writes the samples up to the end of the first `frames` output frames,
    /// waiting for the mix to get there.
    fn write_until(
        &mut self,
        encoder: &mut vv_media::Encoder,
        frames: FrameIdx,
    ) -> Result<(), ExportError> {
        let end_sample = match self.output_fps {
            Some(fps) => timeline_frame_to_sample(frames, fps, PROJECT_SAMPLE_RATE),
            None => {
                timeline_frame_to_sample(frames + self.start_frame, self.fps, PROJECT_SAMPLE_RATE)
                    - self.start_sample
            }
        };
        let end = (end_sample as usize * PROJECT_CHANNELS as usize).min(self.total);
        while self.written < end {
            if self.taken == self.chunk.len() {
                let Ok(chunk) = self.chunks.recv() else {
                    return Err(ExportError::failed("the audio mix stopped early"));
                };
                self.chunk = chunk;
                self.taken = 0;
            }
            let count = (end - self.written).min(self.chunk.len() - self.taken);
            encoder
                .write_audio_samples(&self.chunk[self.taken..self.taken + count])
                .map_err(ExportError::failed)?;
            self.taken += count;
            self.written += count;
        }
        Ok(())
    }
}

/// The mix of `range` into its own audio file. With `report` it also drives
/// the progress (an audio-only export).
#[allow(clippy::too_many_arguments)]
fn export_audio_file(
    project: &Project,
    timeline_id: TimelineId,
    range: std::ops::Range<FrameIdx>,
    file: &AudioFileExport,
    path: &Path,
    report: bool,
    progress: &Mutex<ExportProgress>,
    cancel: &AtomicBool,
    created: &mut Vec<PathBuf>,
) -> Result<(), ExportError> {
    let started = std::time::Instant::now();
    let timeline = project
        .timelines
        .get(timeline_id)
        .ok_or(ExportError::TimelineNotFound)?;
    let range = range.start.max(0)..range.end.min(timeline.total_frames());
    let samples = audio_samples(timeline, range.clone());
    let total = (samples.end - samples.start) as f64;
    if report {
        let mut p = progress.lock().unwrap();
        p.total_frames = (range.end - range.start).max(0);
        p.output_path = path.to_owned();
    }
    created.push(path.to_owned());
    let mut encoder = vv_media::AudioFileEncoder::new(
        path,
        PROJECT_SAMPLE_RATE,
        PROJECT_CHANNELS,
        file.format,
        file.bitrate_kbps,
    )
    .map_err(|e| {
        created.pop();
        ExportError::failed(e)
    })?;
    let mut failure = None;
    let mut done = 0usize;
    let frames = (range.end - range.start).max(0) as f64;
    render_offline(
        project,
        timeline,
        samples,
        &mut LevelCache::on_disk(),
        |chunk| {
            if cancel.load(Ordering::Relaxed) {
                return ControlFlow::Break(());
            }
            if let Err(e) = encoder.write_samples(chunk) {
                failure = Some(e);
                return ControlFlow::Break(());
            }
            done += chunk.len() / PROJECT_CHANNELS as usize;
            if report {
                let mut p = progress.lock().unwrap();
                p.current_frame = (done as f64 / total.max(1.0) * frames) as FrameIdx;
                p.elapsed = started.elapsed();
            }
            ControlFlow::Continue(())
        },
    )?;
    if cancel.load(Ordering::Relaxed) {
        return Err(ExportError::Cancelled);
    }
    if let Some(e) = failure {
        return Err(ExportError::failed(e));
    }
    encoder.finish().map_err(ExportError::failed)?;
    if report {
        let mut p = progress.lock().unwrap();
        p.current_frame = p.total_frames;
        p.elapsed = started.elapsed();
    }
    Ok(())
}

fn audio_samples(timeline: &Timeline, range: std::ops::Range<FrameIdx>) -> std::ops::Range<u64> {
    let fps = timeline.fps.as_f64();
    timeline_frame_to_sample(range.start, fps, PROJECT_SAMPLE_RATE)
        ..timeline_frame_to_sample(range.end, fps, PROJECT_SAMPLE_RATE)
}

/// Mix of all the audio tracks at `PROJECT_SAMPLE_RATE`/`PROJECT_CHANNELS`,
/// over the timeline frames in `range`: the same `mix_range` as the preview.
pub fn mix_audio_track(
    project: &Project,
    timeline: &Timeline,
    range: std::ops::Range<FrameIdx>,
) -> Result<Vec<f32>, ExportError> {
    let samples = audio_samples(timeline, range);
    let mut mixed =
        Vec::with_capacity((samples.end - samples.start) as usize * PROJECT_CHANNELS as usize);
    render_offline(
        project,
        timeline,
        samples,
        &mut LevelCache::on_disk(),
        |chunk| {
            mixed.extend_from_slice(chunk);
            ControlFlow::Continue(())
        },
    )?;
    Ok(mixed)
}

#[cfg(test)]
#[path = "tests/export.rs"]
mod tests;
