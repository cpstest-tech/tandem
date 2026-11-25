//! Face detection and shot cuts for the automatic reframe, on the CPU
//! through tract. YuNet (OpenCV Zoo, MIT, see `assets/LICENSE-YUNET`) is
//! small enough to ship inside the binary: nothing is downloaded.
//! The tracking itself is pure and lives in `vv_core::reframe`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use tract_onnx::prelude::*;
use vv_core::FrameIdx;
use vv_core::reframe::{Analysis, Face, Sample};
use vv_media::{Decoder, FrameYuv420, HwDevice, HwPriority, RgbReader};

mod cuts;
pub mod stabilize;

pub use cuts::{FrameHistogram, find_cuts};

const MODEL: &[u8] = include_bytes!("../assets/face_detection_yunet_2023mar.onnx");
/// The model's fixed input; frames are letterboxed into it.
const INPUT: usize = 640;
const STRIDES: [usize; 3] = [8, 16, 32];
const SCORE: f32 = 0.6;
const NMS_IOU: f32 = 0.3;
/// Longest side frames are decoded at for the analysis.
pub const ANALYSIS_SIDE: u32 = 640;
const CACHE_VERSION: u32 = 1;

#[derive(Debug)]
pub enum Error {
    Cancelled,
    Model(String),
    Media(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str("cancelled"),
            Self::Model(e) | Self::Media(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for Error {}

/// Shared with the UI thread, which polls it.
#[derive(Default)]
pub struct Progress {
    done: AtomicU64,
    total: AtomicU64,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        (self.done.load(Ordering::Relaxed) as f64 / total as f64).min(1.0) as f32
    }
}

/// A face in the pixels of the frame it was found in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detection {
    /// x, y, width, height.
    pub rect: [f32; 4],
    pub score: f32,
    /// Right eye, left eye, nose tip, right and left mouth corner.
    pub landmarks: [[f32; 2]; 5],
}

type Plan = Arc<TypedSimplePlan>;

fn plan() -> Result<Plan, Error> {
    static PLAN: OnceLock<Result<Plan, String>> = OnceLock::new();
    let load = || -> TractResult<Plan> {
        tract_onnx::onnx()
            .model_for_read(&mut &*MODEL)?
            .with_input_fact(0, f32::fact([1, 3, INPUT, INPUT]).into())?
            .into_optimized()?
            .into_runnable()
    };
    PLAN.get_or_init(|| load().map_err(|e| e.to_string()))
        .clone()
        .map_err(Error::Model)
}

/// Output order of the model: cls, obj, bbox, kps, each for strides 8/16/32.
fn output_index(kind: usize, stride: usize) -> usize {
    kind * STRIDES.len() + stride
}

fn input_tensor(frame: &FrameYuv420, scale: f32) -> Tensor {
    let rgb = RgbReader::new(frame);
    let plane = INPUT * INPUT;
    let mut data = vec![0f32; 3 * plane];
    let w = ((frame.width as f32 * scale) as usize).min(INPUT);
    let h = ((frame.height as f32 * scale) as usize).min(INPUT);
    let (fw, fh) = (frame.width as usize, frame.height as usize);
    for py in 0..h {
        let y = ((py as f32 / scale) as usize).min(fh - 1);
        for px in 0..w {
            let x = ((px as f32 / scale) as usize).min(fw - 1);
            let [r, g, b] = rgb.at(x, y);
            let i = py * INPUT + px;
            // BGR, 0..255: what OpenCV's blobFromImage hands the model.
            data[i] = b * 255.0;
            data[plane + i] = g * 255.0;
            data[2 * plane + i] = r * 255.0;
        }
    }
    tract_ndarray::Array4::from_shape_vec((1, 3, INPUT, INPUT), data)
        .expect("the shape matches the data")
        .into_tensor()
}

fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let x1 = a[0].max(b[0]);
    let y1 = a[1].max(b[1]);
    let x2 = (a[0] + a[2]).min(b[0] + b[2]);
    let y2 = (a[1] + a[3]).min(b[1] + b[3]);
    let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let union = a[2] * a[3] + b[2] * b[3] - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

pub fn non_max_suppression(mut found: Vec<Detection>) -> Vec<Detection> {
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut kept: Vec<Detection> = Vec::new();
    for d in found {
        if kept.iter().all(|k| iou(&k.rect, &d.rect) < NMS_IOU) {
            kept.push(d);
        }
    }
    kept
}

/// The faces in `frame`, in its pixels.
pub fn detect(frame: &FrameYuv420) -> Result<Vec<Detection>, Error> {
    let plan = plan()?;
    let scale = INPUT as f32 / frame.width.max(frame.height).max(1) as f32;
    let outputs = plan
        .run(tvec![input_tensor(frame, scale).into()])
        .map_err(|e| Error::Model(e.to_string()))?;
    let view = |i: usize| {
        outputs[i]
            .to_plain_array_view::<f32>()
            .map_err(|e| Error::Model(e.to_string()))
    };
    let mut found = Vec::new();
    for (s, &stride) in STRIDES.iter().enumerate() {
        let cls = view(output_index(0, s))?;
        let obj = view(output_index(1, s))?;
        let bbox = view(output_index(2, s))?;
        let kps = view(output_index(3, s))?;
        let cols = INPUT / stride;
        let cells = cols * cols;
        let cls = cls.as_slice().unwrap_or_default();
        let obj = obj.as_slice().unwrap_or_default();
        let bbox = bbox.as_slice().unwrap_or_default();
        let kps = kps.as_slice().unwrap_or_default();
        if cls.len() < cells
            || obj.len() < cells
            || bbox.len() < 4 * cells
            || kps.len() < 10 * cells
        {
            return Err(Error::Model("unexpected YuNet output shape".into()));
        }
        for i in 0..cells {
            let score = (cls[i].clamp(0.0, 1.0) * obj[i].clamp(0.0, 1.0)).sqrt();
            if score < SCORE {
                continue;
            }
            let (col, row) = ((i % cols) as f32, (i / cols) as f32);
            let st = stride as f32;
            let b = &bbox[4 * i..4 * i + 4];
            let cx = (col + b[0]) * st;
            let cy = (row + b[1]) * st;
            let w = b[2].exp() * st;
            let h = b[3].exp() * st;
            let k = &kps[10 * i..10 * i + 10];
            let landmarks = std::array::from_fn(|j| {
                [
                    (col + k[2 * j]) * st / scale,
                    (row + k[2 * j + 1]) * st / scale,
                ]
            });
            found.push(Detection {
                rect: [
                    (cx - w / 2.0) / scale,
                    (cy - h / 2.0) / scale,
                    w / scale,
                    h / scale,
                ],
                score,
                landmarks,
            });
        }
    }
    Ok(non_max_suppression(found))
}

/// Mean luma change of a rectangle between two frames of the same size, 0..=1.
fn luma_change(now: &FrameYuv420, before: &FrameYuv420, rect: [f32; 4]) -> f32 {
    let (w, h) = (now.width as i64, now.height as i64);
    let x0 = (rect[0] as i64).clamp(0, w);
    let y0 = (rect[1] as i64).clamp(0, h);
    let x1 = ((rect[0] + rect[2]).ceil() as i64).clamp(0, w);
    let y1 = ((rect[1] + rect[3]).ceil() as i64).clamp(0, h);
    if x1 <= x0 || y1 <= y0 || before.y.len() != now.y.len() {
        return 0.0;
    }
    let mut sum = 0u64;
    for y in y0..y1 {
        let row = (y * w) as usize;
        for x in x0..x1 {
            let i = row + x as usize;
            sum += u64::from(now.y[i].abs_diff(before.y[i]));
        }
    }
    sum as f32 / ((x1 - x0) * (y1 - y0)) as f32 / 255.0
}

/// The face as the tracker wants it: fractions of the frame, and how much
/// the mouth moved compared with the rest of the face (camera motion and
/// head turns move both).
pub fn to_face(d: &Detection, now: &FrameYuv420, before: Option<&FrameYuv420>) -> Face {
    let (fw, fh) = (now.width as f32, now.height as f32);
    let mouth = before.map_or(0.0, |before| {
        let [r, l] = [d.landmarks[3], d.landmarks[4]];
        let pad = d.rect[3] * 0.12;
        let mouth = [
            r[0].min(l[0]) - pad * 0.5,
            r[1].min(l[1]) - pad,
            (r[0] - l[0]).abs() + pad,
            (r[1] - l[1]).abs() + 2.0 * pad,
        ];
        (luma_change(now, before, mouth) - luma_change(now, before, d.rect)).max(0.0)
    });
    Face {
        center: [
            (d.rect[0] + d.rect[2] / 2.0) / fw,
            (d.rect[1] + d.rect[3] / 2.0) / fh,
        ],
        size: [d.rect[2] / fw, d.rect[3] / fh],
        score: d.score,
        mouth,
    }
}

/// Size the frames are decoded at: within `ANALYSIS_SIDE`, aspect kept.
pub fn analysis_size(width: u32, height: u32) -> (u32, u32) {
    let scale = (ANALYSIS_SIDE as f64 / width.max(height).max(1) as f64).min(1.0);
    (
        ((width as f64 * scale).round() as u32).max(2),
        ((height as f64 * scale).round() as u32).max(2),
    )
}

pub fn cache_path(
    dir: &Path,
    content_hash: u64,
    range: (FrameIdx, FrameIdx),
    step: u32,
) -> PathBuf {
    dir.join(format!(
        "{content_hash:016x}-{}-{}-{step}-v{CACHE_VERSION}.json",
        range.0, range.1
    ))
}

pub fn load_cached(path: &Path) -> Option<Analysis> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

struct Stretch {
    samples: Vec<Sample>,
    histograms: Vec<(FrameIdx, FrameHistogram)>,
}

/// Faces every `step` frames and the shot cuts of media frames `range` of
/// the video at `path`, cached in `cache` when given.
pub fn analyze(
    path: &Path,
    media_size: (u32, u32),
    range: (FrameIdx, FrameIdx),
    fps: f64,
    step: u32,
    hw: &[HwDevice],
    cache: Option<&Path>,
    progress: &Progress,
    cancel: &AtomicBool,
) -> Result<Analysis, Error> {
    if let Some(cached) = cache.and_then(load_cached) {
        return Ok(cached);
    }
    plan()?;
    let step = step.max(1) as FrameIdx;
    let (start, end) = (range.0.max(0), range.1.max(range.0 + 1));
    let frames = end - start;
    progress.done.store(0, Ordering::Relaxed);
    progress.total.store(frames as u64, Ordering::Relaxed);
    let size = analysis_size(media_size.0, media_size.1);
    let workers = std::thread::available_parallelism()
        .map_or(4, |n| n.get() / 2)
        .clamp(1, 8) as FrameIdx;
    let stretch = ((frames + workers - 1) / workers).max(step);
    let bounds: Vec<(FrameIdx, FrameIdx)> = (start..end)
        .step_by(stretch as usize)
        .map(|s| (s, (s + stretch).min(end)))
        .collect();
    let failed = AtomicBool::new(false);
    let results: Vec<Result<Stretch, Error>> = std::thread::scope(|scope| {
        let handles: Vec<_> = bounds
            .iter()
            .map(|&(from, to)| {
                let failed = &failed;
                scope.spawn(move || {
                    let stop = || cancel.load(Ordering::Relaxed) || failed.load(Ordering::Relaxed);
                    let result = analyze_stretch(
                        path,
                        (from, to),
                        start,
                        step,
                        fps,
                        size,
                        hw,
                        progress,
                        &stop,
                    );
                    if result.is_err() {
                        failed.store(true, Ordering::Relaxed);
                    }
                    result
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or(Err(Error::Media("analysis thread panicked".into())))
            })
            .collect()
    });
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    let mut samples = Vec::new();
    let mut histograms = Vec::new();
    for r in results {
        let r = r?;
        samples.extend(r.samples);
        histograms.extend(r.histograms);
    }
    samples.sort_by_key(|s| s.frame);
    samples.dedup_by_key(|s| s.frame);
    histograms.sort_by_key(|h| h.0);
    histograms.dedup_by_key(|h| h.0);
    let analysis = Analysis {
        size: media_size,
        range: (start, end),
        step: step as u32,
        samples,
        cuts: find_cuts(&histograms, fps),
    };
    if let Some(path) = cache
        && let Some(dir) = path.parent()
        && std::fs::create_dir_all(dir).is_ok()
        && let Ok(json) = serde_json::to_vec(&analysis)
    {
        let _ = vv_core::write_atomic(path, &json);
    }
    Ok(analysis)
}

fn analyze_stretch(
    path: &Path,
    (from, to): (FrameIdx, FrameIdx),
    start: FrameIdx,
    step: FrameIdx,
    fps: f64,
    size: (u32, u32),
    hw: &[HwDevice],
    progress: &Progress,
    stop: &dyn Fn() -> bool,
) -> Result<Stretch, Error> {
    let media = |e: vv_media::MediaError| Error::Media(e.to_string());
    let mut decoder = Decoder::open_with(path, hw, HwPriority::Low).map_err(media)?;
    decoder.set_max_size(Some(size));
    // One frame before the stretch: its cut test and mouth motion need it.
    let warm = (from - 1).max(0);
    if warm > 0 {
        decoder
            .seek_to_time(warm as f64 / fps.max(1e-9))
            .map_err(media)?;
    }
    let mut out = Stretch {
        samples: Vec::new(),
        histograms: Vec::new(),
    };
    let mut previous: Option<Arc<FrameYuv420>> = None;
    let mut counted = from;
    while let Some((idx, frame)) = decoder.next_frame().map_err(media)? {
        if stop() {
            return Err(Error::Cancelled);
        }
        if idx >= to {
            break;
        }
        if idx < warm {
            continue;
        }
        out.histograms.push((idx, FrameHistogram::of(&frame)));
        if idx >= from && (idx - start) % step == 0 {
            let faces = detect(&frame)?
                .iter()
                .map(|d| to_face(d, &frame, previous.as_deref()))
                .collect();
            out.samples.push(Sample { frame: idx, faces });
        }
        if idx >= from {
            progress
                .done
                .fetch_add((idx + 1 - counted).max(0) as u64, Ordering::Relaxed);
            counted = idx + 1;
        }
        previous = Some(frame);
    }
    progress
        .done
        .fetch_add((to - counted).max(0) as u64, Ordering::Relaxed);
    Ok(out)
}

#[cfg(test)]
#[path = "tests/lib.rs"]
mod tests;
