//! Video stabilization: the camera motion between consecutive frames is
//! measured once per clip (`frame_motion`), then the path is smoothed and the
//! difference applied to the clip's transform, with a zoom hiding the edges.

use serde::{Deserialize, Serialize};

use crate::command::{Command, CommandLabel};
use crate::model::{ClipId, FrameIdx, Project, TimelineId, Transform};

/// A small grey picture motion is measured on.
#[derive(Debug, Clone)]
pub struct Luma {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

/// Width motion is measured at: enough for the shake of a hand-held shot.
pub const ANALYSIS_WIDTH: usize = 256;

impl Luma {
    /// Box-downscaled from a `width`×`height` luma plane with `stride` bytes per row.
    pub fn downscaled(plane: &[u8], width: usize, height: usize, stride: usize) -> Self {
        let scale = (width / ANALYSIS_WIDTH).max(1);
        let (w, h) = (width / scale, height / scale);
        let mut data = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0u32;
                for dy in 0..scale {
                    let row = &plane[(y * scale + dy) * stride..];
                    for dx in 0..scale {
                        sum += row[x * scale + dx] as u32;
                    }
                }
                data[y * w + x] = (sum / (scale * scale) as u32) as u8;
            }
        }
        Self {
            width: w,
            height: h,
            data,
        }
    }

    /// Column sums of rows `y0..y1` and row sums, each with its mean removed.
    fn profiles(&self, (x0, x1): (usize, usize), (y0, y1): (usize, usize)) -> (Vec<f32>, Vec<f32>) {
        let mut cols = vec![0.0f32; x1 - x0];
        let mut rows = vec![0.0f32; y1 - y0];
        for y in y0..y1 {
            for x in x0..x1 {
                let v = self.data[y * self.width + x] as f32;
                cols[x - x0] += v;
                rows[y - y0] += v;
            }
        }
        let center = |v: &mut Vec<f32>| {
            let mean = v.iter().sum::<f32>() / v.len().max(1) as f32;
            v.iter_mut().for_each(|x| *x -= mean);
        };
        center(&mut cols);
        center(&mut rows);
        (cols, rows)
    }
}

/// The shift of `b` against `a` (in samples) that best lines them up, with
/// sub-sample precision from a parabola through the best three.
fn best_shift(a: &[f32], b: &[f32], max: isize) -> f32 {
    let n = a.len() as isize;
    let cost = |s: isize| {
        let mut sum = 0.0;
        let mut count = 0;
        for i in max..n - max {
            let j = i + s;
            if j >= 0 && j < n {
                sum += (a[i as usize] - b[j as usize]).abs();
                count += 1;
            }
        }
        sum / count.max(1) as f32
    };
    let costs: Vec<f32> = (-max..=max).map(cost).collect();
    let (best, _) = costs
        .iter()
        .enumerate()
        .min_by(|x, y| x.1.total_cmp(y.1))
        .unwrap_or((max as usize, &0.0));
    let refined = if best > 0 && best + 1 < costs.len() {
        let (l, c, r) = (costs[best - 1], costs[best], costs[best + 1]);
        let denom = l - 2.0 * c + r;
        if denom.abs() > 1e-6 {
            (0.5 * (l - r) / denom).clamp(-0.5, 0.5)
        } else {
            0.0
        }
    } else {
        0.0
    };
    best as f32 - max as f32 + refined
}

/// Camera motion from `a` to `b`: x and y as fractions of the frame (x right,
/// y down in picture space) and the rotation in degrees, measured on the
/// left and right halves.
pub fn frame_motion(a: &Luma, b: &Luma) -> [f32; 3] {
    let (w, h) = (a.width.min(b.width), a.height.min(b.height));
    if w < 16 || h < 16 {
        return [0.0; 3];
    }
    let max_x = (w / 10) as isize;
    let max_y = (h / 10) as isize;
    let (ca, ra) = a.profiles((0, w), (0, h));
    let (cb, rb) = b.profiles((0, w), (0, h));
    let dx = best_shift(&ca, &cb, max_x);
    let dy = best_shift(&ra, &rb, max_y);
    let third = w / 3;
    let (_, left_a) = a.profiles((0, third), (0, h));
    let (_, left_b) = b.profiles((0, third), (0, h));
    let (_, right_a) = a.profiles((w - third, w), (0, h));
    let (_, right_b) = b.profiles((w - third, w), (0, h));
    let dy_left = best_shift(&left_a, &left_b, max_y);
    let dy_right = best_shift(&right_a, &right_b, max_y);
    // The outer thirds' centres are two thirds of the frame apart.
    let angle = ((dy_right - dy_left) / (w - third) as f32)
        .atan()
        .to_degrees();
    [dx / w as f32, dy / h as f32, angle]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stabilization {
    /// Source frame of `motion[0]`'s first picture.
    pub start: FrameIdx,
    /// Motion from each analysed frame to the next (`frame_motion`).
    pub motion: Vec<[f32; 3]>,
    /// 0..1: how much of the shake is taken out.
    pub strength: f32,
    /// Per analysed frame: what moves the picture back onto the smooth path.
    pub corrections: Vec<[f32; 3]>,
    /// Scale that keeps the moved picture over the whole frame.
    pub zoom: f32,
}

/// The most the edges may be hidden by.
const MAX_ZOOM: f32 = 1.35;

impl Stabilization {
    pub fn new(start: FrameIdx, motion: Vec<[f32; 3]>, strength: f32, fps: f64) -> Self {
        let strength = strength.clamp(0.0, 1.0);
        let mut path = vec![[0.0f32; 3]];
        for m in &motion {
            let last = path[path.len() - 1];
            path.push([last[0] + m[0], last[1] + m[1], last[2] + m[2]]);
        }
        // Up to a second and a half either side at full strength.
        let radius = (strength as f64 * fps * 1.5).round() as isize;
        let n = path.len() as isize;
        let corrections: Vec<[f32; 3]> = (0..n)
            .map(|i| {
                let mut sum = [0.0f32; 3];
                let mut weight = 0.0f32;
                for j in (i - radius).max(0)..=(i + radius).min(n - 1) {
                    let d = (j - i) as f32 / (radius.max(1) as f32);
                    let w = (-2.0 * d * d).exp();
                    for (k, s) in sum.iter_mut().enumerate() {
                        *s += w * path[j as usize][k];
                    }
                    weight += w;
                }
                let raw = path[i as usize];
                [0, 1, 2].map(|k| sum[k] / weight.max(1e-6) - raw[k])
            })
            .collect();
        let margin = corrections
            .iter()
            .map(|c| {
                let turn = c[2].to_radians().abs().sin();
                (2.0 * c[0].abs() + turn).max(2.0 * c[1].abs() + turn)
            })
            .fold(0.0f32, f32::max);
        Self {
            start,
            motion,
            strength,
            corrections,
            zoom: (1.0 + margin).min(MAX_ZOOM),
        }
    }

    /// Moves `transform` (of a picture fit into `fit` timeline pixels) onto
    /// the smooth path at `source_frame`.
    pub fn apply(&self, transform: &mut Transform, source_frame: FrameIdx, fit: [f32; 2]) {
        if self.corrections.is_empty() {
            return;
        }
        let i = (source_frame - self.start).clamp(0, self.corrections.len() as FrameIdx - 1);
        let [dx, dy, angle] = self.corrections[i as usize];
        let zoom = [transform.zoom[0] * self.zoom, transform.zoom[1] * self.zoom];
        // The picture moved by the motion: the correction is in picture
        // space, y down, while the position's y goes up.
        transform.position[0] += dx * fit[0] * zoom[0];
        transform.position[1] -= dy * fit[1] * zoom[1];
        transform.rotation += angle;
        transform.zoom = zoom;
    }
}

/// Sets or removes a clip's stabilization.
#[derive(Debug)]
pub struct SetStabilization {
    pub timeline: TimelineId,
    pub track_index: usize,
    pub clip_id: ClipId,
    pub value: Option<Stabilization>,
    old: Option<Option<Stabilization>>,
}

impl SetStabilization {
    pub fn new(
        timeline: TimelineId,
        track_index: usize,
        clip_id: ClipId,
        value: Option<Stabilization>,
    ) -> Self {
        Self {
            timeline,
            track_index,
            clip_id,
            value,
            old: None,
        }
    }
}

impl Command for SetStabilization {
    fn label(&self) -> CommandLabel {
        CommandLabel::Stabilize
    }

    fn apply(&mut self, project: &mut Project) {
        if let Some(clip) =
            project.timelines[self.timeline].clip_mut(self.track_index, self.clip_id)
        {
            self.old = Some(std::mem::replace(
                &mut clip.effects.stabilize,
                self.value.clone(),
            ));
        }
    }

    fn undo(&self, project: &mut Project) {
        if let (Some(old), Some(clip)) = (
            &self.old,
            project.timelines[self.timeline].clip_mut(self.track_index, self.clip_id),
        ) {
            clip.effects.stabilize = old.clone();
        }
    }
}

#[cfg(test)]
#[path = "tests/stabilize.rs"]
mod tests;
