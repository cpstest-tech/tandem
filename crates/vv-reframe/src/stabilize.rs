//! Camera motion between consecutive frames for the stabilizer: blocks of
//! the luma matched coarse to fine, then one shift and rotation fitted to
//! them, outliers (things moving on their own) left out.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use vv_core::FrameIdx;
use vv_core::stabilize::Motion;
use vv_media::{Decoder, FrameYuv420, HwDevice, HwPriority};

use crate::{Error, FrameHistogram, Progress, find_cuts};

/// Longest side frames are decoded at for the measure.
pub const MOTION_SIDE: u32 = 480;
const CACHE_VERSION: u32 = 1;

const LEVELS: usize = 3;
/// Block side at full measure resolution, halved at each level.
const BLOCK: usize = 32;
const GRID: (usize, usize) = (8, 6);
/// Search reach at the coarsest level, in its pixels.
const COARSE_REACH: isize = 8;
/// Below this luma variance a block has no texture to match.
const MIN_VARIANCE: f32 = 30.0;
/// Mean absolute difference above which a match is not trusted.
const MAX_MEAN_DIFF: f32 = 24.0;
const MIN_INLIERS: usize = 5;

pub fn motion_size(width: u32, height: u32) -> (u32, u32) {
    let scale = (MOTION_SIDE as f64 / width.max(height).max(1) as f64).min(1.0);
    (
        ((width as f64 * scale).round() as u32).max(2),
        ((height as f64 * scale).round() as u32).max(2),
    )
}

pub fn motion_cache_path(dir: &Path, content_hash: u64, range: (FrameIdx, FrameIdx)) -> PathBuf {
    dir.join(format!(
        "{content_hash:016x}-{}-{}-motion-v{CACHE_VERSION}.json",
        range.0, range.1
    ))
}

pub fn load_cached_motion(path: &Path) -> Option<Motion> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

#[derive(Clone)]
pub(crate) struct Gray {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) px: Vec<u8>,
}

impl Gray {
    fn at(&self, x: usize, y: usize) -> u8 {
        self.px[y * self.width + x]
    }

    fn half(&self) -> Gray {
        let (width, height) = ((self.width / 2).max(1), (self.height / 2).max(1));
        let mut px = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let (x2, y2) = ((2 * x).min(self.width - 1), (2 * y).min(self.height - 1));
                let (x3, y3) = ((x2 + 1).min(self.width - 1), (y2 + 1).min(self.height - 1));
                let sum = u16::from(self.at(x2, y2))
                    + u16::from(self.at(x3, y2))
                    + u16::from(self.at(x2, y3))
                    + u16::from(self.at(x3, y3));
                px.push((sum / 4) as u8);
            }
        }
        Gray { width, height, px }
    }
}

pub(crate) struct Pyramid {
    levels: Vec<Gray>,
}

impl Pyramid {
    pub(crate) fn new(base: Gray) -> Self {
        let mut levels = vec![base];
        for _ in 1..LEVELS {
            let next = levels.last().unwrap().half();
            levels.push(next);
        }
        Self { levels }
    }

    pub(crate) fn of(frame: &FrameYuv420) -> Self {
        let (width, height) = (frame.width as usize, frame.height as usize);
        Self::new(Gray {
            width,
            height,
            px: frame.y[..width * height].to_vec(),
        })
    }

    fn base(&self) -> &Gray {
        &self.levels[0]
    }
}

/// Sum of absolute differences of the `size` block at `a` in `from` and at
/// `b` in `to`; `None` if either leaves its image.
fn sad(from: &Gray, a: (isize, isize), to: &Gray, b: (isize, isize), size: usize) -> Option<u32> {
    let inside = |g: &Gray, (x, y): (isize, isize)| {
        x >= 0 && y >= 0 && x as usize + size <= g.width && y as usize + size <= g.height
    };
    if !inside(from, a) || !inside(to, b) {
        return None;
    }
    let mut sum = 0u32;
    for row in 0..size {
        let ra = (a.1 as usize + row) * from.width + a.0 as usize;
        let rb = (b.1 as usize + row) * to.width + b.0 as usize;
        for (p, q) in from.px[ra..ra + size].iter().zip(&to.px[rb..rb + size]) {
            sum += u32::from(p.abs_diff(*q));
        }
    }
    Some(sum)
}

fn variance(g: &Gray, (x, y): (usize, usize), size: usize) -> f32 {
    let n = (size * size) as f32;
    let (mut sum, mut sq) = (0.0f32, 0.0f32);
    for row in y..y + size {
        for &p in &g.px[row * g.width + x..row * g.width + x + size] {
            let v = f32::from(p);
            sum += v;
            sq += v * v;
        }
    }
    sq / n - (sum / n) * (sum / n)
}

/// Best displacement of the block at `at` (full resolution) from `prev`
/// to `next`, in full-resolution pixels with sub-pixel refinement.
fn match_block(prev: &Pyramid, next: &Pyramid, at: (usize, usize)) -> Option<[f32; 2]> {
    let mut guess = (0isize, 0isize);
    let mut best_sad = 0;
    for level in (0..LEVELS).rev() {
        let scale = 1 << level;
        let size = BLOCK / scale;
        let origin = ((at.0 / scale) as isize, (at.1 / scale) as isize);
        let reach = if level == LEVELS - 1 { COARSE_REACH } else { 2 };
        let (from, to) = (&prev.levels[level], &next.levels[level]);
        let mut best: Option<((isize, isize), u32)> = None;
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let d = (guess.0 + dx, guess.1 + dy);
                let Some(s) = sad(from, origin, to, (origin.0 + d.0, origin.1 + d.1), size) else {
                    continue;
                };
                if best.is_none_or(|(_, b)| s < b) {
                    best = Some((d, s));
                }
            }
        }
        let (d, s) = best?;
        best_sad = s;
        guess = if level == 0 { d } else { (d.0 * 2, d.1 * 2) };
    }
    if best_sad as f32 / (BLOCK * BLOCK) as f32 > MAX_MEAN_DIFF {
        return None;
    }
    let (from, to) = (prev.base(), next.base());
    let origin = (at.0 as isize, at.1 as isize);
    let cost = |d: (isize, isize)| {
        sad(from, origin, to, (origin.0 + d.0, origin.1 + d.1), BLOCK).map(|s| s as f32)
    };
    let refine = |minus: Option<f32>, plus: Option<f32>| -> f32 {
        match (minus, plus) {
            (Some(m), Some(p)) => {
                let curve = m + p - 2.0 * best_sad as f32;
                if curve > 0.0 {
                    ((m - p) / (2.0 * curve)).clamp(-0.5, 0.5)
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    };
    let fx = refine(cost((guess.0 - 1, guess.1)), cost((guess.0 + 1, guess.1)));
    let fy = refine(cost((guess.0, guess.1 - 1)), cost((guess.0, guess.1 + 1)));
    Some([guess.0 as f32 + fx, guess.1 as f32 + fy])
}

/// Shift and small rotation (clockwise, Y down) taking `points` (centred
/// positions) along `moves`, least squares.
fn fit(points: &[[f32; 2]], moves: &[[f32; 2]]) -> [f32; 3] {
    let n = points.len() as f32;
    let (mut spx, mut spy, mut svx, mut svy, mut sr, mut scross) =
        (0.0f32, 0.0, 0.0, 0.0, 0.0, 0.0);
    for (p, v) in points.iter().zip(moves) {
        spx += p[0];
        spy += p[1];
        svx += v[0];
        svy += v[1];
        sr += p[0] * p[0] + p[1] * p[1];
        scross += p[0] * v[1] - p[1] * v[0];
    }
    let den = sr - (spx * spx + spy * spy) / n;
    let angle = if den > 1e-3 {
        (scross + spy * svx / n - spx * svy / n) / den
    } else {
        0.0
    };
    [(svx + angle * spy) / n, (svy - angle * spx) / n, angle]
}

fn residual(p: [f32; 2], v: [f32; 2], m: [f32; 3]) -> f32 {
    let ex = m[0] - m[2] * p[1] - v[0];
    let ey = m[1] + m[2] * p[0] - v[1];
    (ex * ex + ey * ey).sqrt()
}

/// How the content of `next` moved from `prev`: x, y in its pixels (Y
/// down) and the angle in radians, clockwise. `None` without enough
/// trustworthy blocks.
pub(crate) fn estimate(prev: &Pyramid, next: &Pyramid) -> Option<[f32; 3]> {
    let g = prev.base();
    if g.width < BLOCK * 2 || g.height < BLOCK * 2 {
        return None;
    }
    let center = [g.width as f32 / 2.0, g.height as f32 / 2.0];
    let margin = (BLOCK as f32).max(g.width.min(g.height) as f32 * 0.08) as usize;
    let span = |len: usize, count: usize, i: usize| {
        let usable = len.saturating_sub(2 * margin + BLOCK);
        margin + usable * i / (count - 1).max(1)
    };
    let mut points = Vec::new();
    let mut moves = Vec::new();
    for row in 0..GRID.1 {
        for col in 0..GRID.0 {
            let at = (span(g.width, GRID.0, col), span(g.height, GRID.1, row));
            if variance(g, at, BLOCK) < MIN_VARIANCE {
                continue;
            }
            if let Some(v) = match_block(prev, next, at) {
                let half = BLOCK as f32 / 2.0;
                points.push([
                    at.0 as f32 + half - center[0],
                    at.1 as f32 + half - center[1],
                ]);
                moves.push(v);
            }
        }
    }
    let mut inliers: Vec<usize> = (0..points.len()).collect();
    for _ in 0..3 {
        if inliers.len() < MIN_INLIERS {
            return None;
        }
        let p: Vec<_> = inliers.iter().map(|&i| points[i]).collect();
        let v: Vec<_> = inliers.iter().map(|&i| moves[i]).collect();
        let model = fit(&p, &v);
        let errors: Vec<f32> = (0..points.len())
            .map(|i| residual(points[i], moves[i], model))
            .collect();
        let limit = {
            let mut sorted = errors.clone();
            sorted.sort_by(f32::total_cmp);
            (sorted[sorted.len() / 2] * 2.5).max(1.0)
        };
        let next_inliers: Vec<usize> = (0..points.len()).filter(|&i| errors[i] <= limit).collect();
        if next_inliers == inliers {
            return Some(model);
        }
        inliers = next_inliers;
    }
    (inliers.len() >= MIN_INLIERS).then(|| {
        let p: Vec<_> = inliers.iter().map(|&i| points[i]).collect();
        let v: Vec<_> = inliers.iter().map(|&i| moves[i]).collect();
        fit(&p, &v)
    })
}

struct Stretch {
    steps: Vec<(FrameIdx, [f32; 3])>,
    histograms: Vec<(FrameIdx, FrameHistogram)>,
}

/// The camera motion over media frames `range` of the video at `path`,
/// cached in `cache` when given.
pub fn analyze_motion(
    path: &Path,
    media_size: (u32, u32),
    range: (FrameIdx, FrameIdx),
    fps: f64,
    hw: &[HwDevice],
    cache: Option<&Path>,
    progress: &Progress,
    cancel: &AtomicBool,
) -> Result<Motion, Error> {
    if let Some(cached) = cache.and_then(load_cached_motion) {
        return Ok(cached);
    }
    let (start, end) = (range.0.max(0), range.1.max(range.0 + 1));
    let frames = end - start;
    progress.done.store(0, Ordering::Relaxed);
    progress.total.store(frames as u64, Ordering::Relaxed);
    let size = motion_size(media_size.0, media_size.1);
    let workers = std::thread::available_parallelism()
        .map_or(4, |n| n.get() / 2)
        .clamp(1, 8) as FrameIdx;
    let stretch = ((frames + workers - 1) / workers).max(1);
    let bounds: Vec<(FrameIdx, FrameIdx)> = (start..end)
        .step_by(stretch as usize)
        .map(|s| (s, (s + stretch).min(end)))
        .collect();
    let failed = AtomicBool::new(false);
    let results: Vec<Result<Stretch, Error>> = std::thread::scope(|scope| {
        let handles: Vec<_> = bounds
            .iter()
            .map(|&bound| {
                let failed = &failed;
                scope.spawn(move || {
                    let stop = || cancel.load(Ordering::Relaxed) || failed.load(Ordering::Relaxed);
                    let result = measure_stretch(path, bound, fps, size, hw, progress, &stop);
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
                    .unwrap_or(Err(Error::Media("motion thread panicked".into())))
            })
            .collect()
    });
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    let mut steps = vec![[0.0f32; 3]; frames as usize];
    let mut histograms = Vec::new();
    for r in results {
        let r = r?;
        for (idx, step) in r.steps {
            if let Some(slot) = steps.get_mut((idx - start) as usize) {
                *slot = step;
            }
        }
        histograms.extend(r.histograms);
    }
    histograms.sort_by_key(|h| h.0);
    histograms.dedup_by_key(|h| h.0);
    let cuts = find_cuts(&histograms, fps);
    for &cut in &cuts {
        if let Some(slot) = steps.get_mut((cut - start).max(0) as usize) {
            *slot = [0.0; 3];
        }
    }
    let motion = Motion {
        size: media_size,
        range: (start, end),
        steps,
        cuts,
    };
    if let Some(path) = cache
        && let Some(dir) = path.parent()
        && std::fs::create_dir_all(dir).is_ok()
        && let Ok(json) = serde_json::to_vec(&motion)
    {
        let _ = vv_core::write_atomic(path, &json);
    }
    Ok(motion)
}

fn measure_stretch(
    path: &Path,
    (from, to): (FrameIdx, FrameIdx),
    fps: f64,
    size: (u32, u32),
    hw: &[HwDevice],
    progress: &Progress,
    stop: &dyn Fn() -> bool,
) -> Result<Stretch, Error> {
    let media = |e: vv_media::MediaError| Error::Media(e.to_string());
    let mut decoder = Decoder::open_with(path, hw, HwPriority::Low).map_err(media)?;
    decoder.set_max_size(Some(size));
    // One frame before the stretch: the motion into its first frame.
    let warm = (from - 1).max(0);
    if warm > 0 {
        decoder
            .seek_to_time(warm as f64 / fps.max(1e-9))
            .map_err(media)?;
    }
    let mut out = Stretch {
        steps: Vec::new(),
        histograms: Vec::new(),
    };
    let mut previous: Option<Pyramid> = None;
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
        let pyramid = Pyramid::of(&frame);
        out.histograms.push((idx, FrameHistogram::of(&frame)));
        if idx >= from {
            if let Some(prev) = previous.as_ref()
                && let Some([dx, dy, angle]) = estimate(prev, &pyramid)
            {
                let (w, h) = (pyramid.base().width as f32, pyramid.base().height as f32);
                out.steps.push((idx, [dx / w, dy / h, angle]));
            }
            progress
                .done
                .fetch_add((idx + 1 - counted).max(0) as u64, Ordering::Relaxed);
            counted = idx + 1;
        }
        previous = Some(pyramid);
    }
    progress
        .done
        .fetch_add((to - counted).max(0) as u64, Ordering::Relaxed);
    Ok(out)
}

#[cfg(test)]
#[path = "tests/stabilize.rs"]
mod tests;
