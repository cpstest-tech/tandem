//! Stabilization: the camera motion measured by `vv-reframe`, smoothed per
//! shot, cancelled with plain position/rotation/zoom keyframes tagged
//! `Origin::Stabilize`.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::auto_zoom::{AutoKeyframe, Origin, strip};
use crate::{Clip, FrameIdx, Interpolation, TransformParam, TransformTracks};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Motion {
    /// Native size of the media: the pixels `steps` are fractions of.
    pub size: (u32, u32),
    /// Media frames, end excluded.
    pub range: (FrameIdx, FrameIdx),
    /// Per frame of `range`, how its content moved from the frame before:
    /// x and y in fractions of the frame (Y down), the angle in radians,
    /// clockwise.
    pub steps: Vec<[f32; 3]>,
    pub cuts: Vec<FrameIdx>,
}

impl Motion {
    pub fn shots(&self) -> Vec<Range<FrameIdx>> {
        let (start, end) = self.range;
        let mut bounds = vec![start];
        bounds.extend(self.cuts.iter().copied().filter(|c| *c > start && *c < end));
        bounds.push(end);
        bounds.windows(2).map(|w| w[0]..w[1]).collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// 0 = only the fastest shake, 1 = as steady as a tripod.
    pub strength: f32,
    /// The most the frame is enlarged to hide the moving edges.
    pub max_zoom: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            strength: 0.5,
            max_zoom: 1.15,
        }
    }
}

pub const STRENGTH: std::ops::RangeInclusive<f32> = 0.0..=1.0;
pub const MAX_ZOOM: std::ops::RangeInclusive<f32> = 1.02..=1.5;

/// Smoothing window, in seconds each side, at strength 0 and 1.
const WINDOW_SECS: (f64, f64) = (0.15, 3.0);

/// What to move one shot by, frame by frame.
#[derive(Clone, Debug, PartialEq)]
pub struct ShotPlan {
    pub range: Range<FrameIdx>,
    pub zoom: f32,
    /// Per frame of `range`: x, y in fractions of the frame (Y down), the
    /// angle in radians, clockwise.
    pub offsets: Vec<[f32; 3]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub shots: Vec<ShotPlan>,
}

/// Why a clip cannot be stabilized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocker {
    /// Zoom, position or rotation keyframes not written by the stabilizer.
    Keyframes,
    /// Rotated, offset-anchored or cropped: the correction assumes none.
    Transform,
}

const MOVED: [TransformParam; 5] = [
    TransformParam::ZoomX,
    TransformParam::ZoomY,
    TransformParam::PositionX,
    TransformParam::PositionY,
    TransformParam::Rotation,
];

pub fn blocker(clip: &Clip) -> Option<Blocker> {
    let base = strip(
        &clip.effects.transform,
        &clip.auto_keyframes,
        Origin::Stabilize,
    );
    if MOVED.iter().any(|&p| !base.track(p).is_constant()) {
        return Some(Blocker::Keyframes);
    }
    let still = [
        TransformParam::Rotation,
        TransformParam::AnchorX,
        TransformParam::AnchorY,
        TransformParam::CropLeft,
        TransformParam::CropTop,
        TransformParam::CropRight,
        TransformParam::CropBottom,
    ];
    if still.iter().any(|&p| {
        let track = base.track(p);
        !track.is_constant() || track.default.abs() > 1e-4
    }) {
        return Some(Blocker::Transform);
    }
    None
}

/// Mirrors around both ends, so a steady drift stays a straight line
/// instead of bending towards the edge.
fn smooth(values: &[f32], radius: usize) -> Vec<f32> {
    let n = values.len();
    if n < 3 || radius == 0 {
        return values.to_vec();
    }
    let radius = radius.min(n - 1);
    let sigma = radius as f32 / 2.0;
    let weights: Vec<f32> = (0..=radius)
        .map(|d| (-(d * d) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let at = |i: isize| -> f32 {
        let last = n as isize - 1;
        if i < 0 {
            2.0 * values[0] - values[(-i).min(last) as usize]
        } else if i > last {
            2.0 * values[n - 1] - values[(2 * last - i).max(0) as usize]
        } else {
            values[i as usize]
        }
    };
    (0..n as isize)
        .map(|i| {
            let mut sum = weights[0] * values[i as usize];
            let mut total = weights[0];
            for (d, w) in weights.iter().enumerate().skip(1) {
                sum += w * (at(i - d as isize) + at(i + d as isize));
                total += 2.0 * w;
            }
            sum / total
        })
        .collect()
}

/// Enlargement that keeps the frame covered once its content is moved by
/// `offset` (pixels, Y down, radians) in a `w`x`h` frame.
pub fn zoom_needed(offset: [f32; 3], w: f32, h: f32) -> f32 {
    let (sin, cos) = (-offset[2]).sin_cos();
    let unrotate = |x: f32, y: f32| [x * cos - y * sin, x * sin + y * cos];
    let shift = unrotate(offset[0], offset[1]);
    let room = [w / 2.0 - shift[0].abs(), h / 2.0 - shift[1].abs()];
    if room[0] <= 0.0 || room[1] <= 0.0 {
        return f32::INFINITY;
    }
    let mut needed = 1.0f32;
    for (x, y) in [(w, h), (w, -h), (-w, h), (-w, -h)] {
        let c = unrotate(x / 2.0, y / 2.0);
        needed = needed.max(c[0].abs() / room[0]).max(c[1].abs() / room[1]);
    }
    needed
}

fn sliding_min(values: &[f32], radius: usize) -> Vec<f32> {
    (0..values.len())
        .map(|i| {
            let from = i.saturating_sub(radius);
            let to = (i + radius + 1).min(values.len());
            values[from..to]
                .iter()
                .copied()
                .fold(f32::INFINITY, f32::min)
        })
        .collect()
}

fn sliding_mean(values: &[f32], radius: usize) -> Vec<f32> {
    (0..values.len())
        .map(|i| {
            let from = i.saturating_sub(radius);
            let to = (i + radius + 1).min(values.len());
            values[from..to].iter().sum::<f32>() / (to - from) as f32
        })
        .collect()
}

pub fn plan(motion: &Motion, config: &Config, fps: f64) -> Plan {
    let (w, h) = (motion.size.0.max(1) as f32, motion.size.1.max(1) as f32);
    let strength = config.strength.clamp(*STRENGTH.start(), *STRENGTH.end());
    let max_zoom = config.max_zoom.clamp(*MAX_ZOOM.start(), *MAX_ZOOM.end());
    let secs = WINDOW_SECS.0 + (WINDOW_SECS.1 - WINDOW_SECS.0) * f64::from(strength);
    let radius = (secs * fps.max(1.0)).round() as usize;
    let mut result = Plan::default();
    for shot in motion.shots() {
        let first = (shot.start - motion.range.0) as usize;
        let len = (shot.end - shot.start) as usize;
        let steps = motion.steps.get(first..first + len).unwrap_or(&[]);
        if steps.len() != len || len == 0 {
            continue;
        }
        let mut path = [vec![0.0f32; len], vec![0.0; len], vec![0.0; len]];
        let unit = [w, h, 1.0];
        for (i, step) in steps.iter().enumerate().skip(1) {
            for (axis, (s, u)) in path.iter_mut().zip(step.iter().zip(unit)) {
                axis[i] = axis[i - 1] + s * u;
            }
        }
        let steady = path.clone().map(|axis| smooth(&axis, radius));
        let raw: Vec<[f32; 3]> = (0..len)
            .map(|i| std::array::from_fn(|a| steady[a][i] - path[a][i]))
            .collect();
        let scale = |o: [f32; 3], k: f32| o.map(|v| v * k);
        let fits: Vec<f32> = raw
            .iter()
            .map(|&o| {
                if zoom_needed(o, w, h) <= max_zoom {
                    return 1.0;
                }
                let (mut lo, mut hi) = (0.0f32, 1.0f32);
                for _ in 0..14 {
                    let mid = (lo + hi) / 2.0;
                    if zoom_needed(scale(o, mid), w, h) <= max_zoom {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                lo
            })
            .collect();
        // Easing into a clamp instead of hitting it: the mean of minima
        // over the same window never exceeds a frame's own limit.
        let half = (radius / 2).max(1);
        let fits = sliding_mean(&sliding_min(&fits, half), half);
        let offsets: Vec<[f32; 3]> = raw.iter().zip(&fits).map(|(&o, &k)| scale(o, k)).collect();
        let zoom = offsets
            .iter()
            .map(|&o| zoom_needed(o, w, h))
            .fold(1.0f32, f32::max)
            .min(max_zoom);
        result.shots.push(ShotPlan {
            range: shot,
            zoom,
            offsets: offsets
                .into_iter()
                .map(|o| [o[0] / w, o[1] / h, o[2]])
                .collect(),
        });
    }
    result
}

/// Bounds the cost of `kept` on long still stretches.
const MAX_KEY_GAP: usize = 240;

/// Indices of `values` that linear interpolation between them keeps within
/// `tolerance`; the first and the last always stay.
fn kept(values: &[f32], tolerance: f32) -> Vec<usize> {
    let mut out = Vec::new();
    if values.is_empty() {
        return out;
    }
    out.push(0);
    let mut anchor = 0;
    for i in 1..values.len().saturating_sub(1) {
        let next = i + 1;
        let fits = next - anchor <= MAX_KEY_GAP
            && (anchor + 1..next).all(|j| {
                let t = (j - anchor) as f32 / (next - anchor) as f32;
                let line = values[anchor] + (values[next] - values[anchor]) * t;
                (values[j] - line).abs() <= tolerance
            });
        if !fits {
            out.push(i);
            anchor = i;
        }
    }
    if values.len() > 1 {
        out.push(values.len() - 1);
    }
    out
}

/// Timeline pixels a moving keyframe may drift from the exact path.
const POSITION_TOLERANCE: f32 = 0.1;
const ROTATION_TOLERANCE_DEG: f32 = 0.01;

/// The transform of `clip` with `plan` cancelling the shake, for a timeline
/// of `frame` pixels. The previous stabilization is replaced; check
/// `blocker` first.
pub fn keyframes(
    clip: &Clip,
    plan: &Plan,
    media_size: (u32, u32),
    frame: (u32, u32),
) -> (TransformTracks, Vec<AutoKeyframe>) {
    let mut transform = strip(
        &clip.effects.transform,
        &clip.auto_keyframes,
        Origin::Stabilize,
    );
    let mut auto: Vec<AutoKeyframe> = clip
        .auto_keyframes
        .iter()
        .filter(|k| k.origin != Origin::Stabilize)
        .copied()
        .collect();
    let zoom = [
        transform.track(TransformParam::ZoomX).default,
        transform.track(TransformParam::ZoomY).default,
    ];
    let position = [
        transform.track(TransformParam::PositionX).default,
        transform.track(TransformParam::PositionY).default,
    ];
    let (mw, mh) = (media_size.0.max(1) as f32, media_size.1.max(1) as f32);
    let fit = (frame.0.max(1) as f32 / mw).min(frame.1.max(1) as f32 / mh);
    let (first, end) = (clip.source_in(), clip.source_out());
    let mut put = |transform: &mut TransformTracks,
                   param: TransformParam,
                   frame: FrameIdx,
                   value: f32,
                   interpolation: Interpolation| {
        transform
            .track_mut(param)
            .upsert(frame, value, interpolation);
        auto.push(AutoKeyframe {
            param,
            frame,
            value,
            origin: Origin::Stabilize,
        });
    };
    for shot in &plan.shots {
        let range = shot.range.start.max(first)..shot.range.end.min(end);
        if range.is_empty() {
            continue;
        }
        let skip = (range.start - shot.range.start) as usize;
        let offsets = &shot.offsets[skip..skip + (range.end - range.start) as usize];
        let channels: [(TransformParam, Vec<f32>, f32); 3] = [
            (
                TransformParam::PositionX,
                offsets
                    .iter()
                    .map(|o| position[0] + zoom[0] * shot.zoom * fit * o[0] * mw)
                    .collect(),
                POSITION_TOLERANCE,
            ),
            (
                TransformParam::PositionY,
                offsets
                    .iter()
                    .map(|o| position[1] - zoom[1] * shot.zoom * fit * o[1] * mh)
                    .collect(),
                POSITION_TOLERANCE,
            ),
            (
                TransformParam::Rotation,
                offsets.iter().map(|o| o[2].to_degrees()).collect(),
                ROTATION_TOLERANCE_DEG,
            ),
        ];
        for (param, values, tolerance) in channels {
            let keep = kept(&values, tolerance);
            let last = keep.len().saturating_sub(1);
            for (n, i) in keep.into_iter().enumerate() {
                let interpolation = if n == last {
                    Interpolation::Hold
                } else {
                    Interpolation::Linear
                };
                put(
                    &mut transform,
                    param,
                    range.start + i as FrameIdx,
                    values[i],
                    interpolation,
                );
            }
        }
        for (param, base) in [
            (TransformParam::ZoomX, zoom[0]),
            (TransformParam::ZoomY, zoom[1]),
        ] {
            put(
                &mut transform,
                param,
                range.start,
                base * shot.zoom,
                Interpolation::Hold,
            );
        }
    }
    (transform, auto)
}

#[cfg(test)]
#[path = "tests/stabilize.rs"]
mod tests;
