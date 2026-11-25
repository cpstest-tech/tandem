//! Variable speed inside a clip (speed ramp). The clip keeps a constant
//! average speed, so its source range and length stay those of the linear
//! model; the curve only redistributes the source frames over that length.

use serde::{Deserialize, Serialize};

use crate::command::{Command, CommandLabel, SetClipSpeed, SpeedFit};
use crate::model::{ClipId, FrameIdx, Project, Rational, TimelineId, Track};

pub const MIN_SPEED: f32 = 0.1;
pub const MAX_SPEED: f32 = 10.0;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeedCurve {
    /// `[x, speed]`: x from 0 to 1 over the clip's length, sorted, the first
    /// at 0 and the last at 1. Linear between points.
    pub points: Vec<[f32; 2]>,
    /// The speed the clip had without the curve: removing it goes back there.
    pub base_speed: Rational,
    /// The source range the curve was laid on; a trim or a split changes it
    /// and the curve stops applying (`Clip::curve_active`).
    pub range: (FrameIdx, FrameIdx),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurvePreset {
    RampUp,
    RampDown,
    SlowMiddle,
    FastMiddle,
    FlashIn,
    FlashOut,
    Pulse,
}

impl CurvePreset {
    pub const ALL: [CurvePreset; 7] = [
        CurvePreset::RampUp,
        CurvePreset::RampDown,
        CurvePreset::SlowMiddle,
        CurvePreset::FastMiddle,
        CurvePreset::FlashIn,
        CurvePreset::FlashOut,
        CurvePreset::Pulse,
    ];

    pub fn points(self) -> Vec<[f32; 2]> {
        match self {
            CurvePreset::RampUp => vec![[0.0, 0.5], [1.0, 3.0]],
            CurvePreset::RampDown => vec![[0.0, 3.0], [1.0, 0.5]],
            CurvePreset::SlowMiddle => {
                vec![[0.0, 2.5], [0.35, 2.5], [0.5, 0.3], [0.65, 2.5], [1.0, 2.5]]
            }
            CurvePreset::FastMiddle => {
                vec![[0.0, 0.6], [0.4, 0.6], [0.5, 4.0], [0.6, 0.6], [1.0, 0.6]]
            }
            CurvePreset::FlashIn => vec![[0.0, 4.0], [0.35, 4.0], [0.6, 1.0], [1.0, 1.0]],
            CurvePreset::FlashOut => vec![[0.0, 1.0], [0.4, 1.0], [0.65, 4.0], [1.0, 4.0]],
            CurvePreset::Pulse => {
                vec![[0.0, 1.0], [0.25, 3.0], [0.5, 1.0], [0.75, 3.0], [1.0, 1.0]]
            }
        }
    }
}

/// Sorted, clamped, with points at 0 and 1: what any edit is normalised to.
pub fn normalized(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = points
        .iter()
        .filter(|p| p[0].is_finite() && p[1].is_finite())
        .map(|p| [p[0].clamp(0.0, 1.0), p[1].clamp(MIN_SPEED, MAX_SPEED)])
        .collect();
    out.sort_by(|a, b| a[0].total_cmp(&b[0]));
    if out.is_empty() {
        return vec![[0.0, 1.0], [1.0, 1.0]];
    }
    if out[0][0] > 0.0 {
        out.insert(0, [0.0, out[0][1]]);
    }
    let last = out[out.len() - 1];
    if last[0] < 1.0 {
        out.push([1.0, last[1]]);
    }
    out
}

/// Speed at `x` (0..1), linear between points.
pub fn speed_at(points: &[[f32; 2]], x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    let Some(first) = points.first() else {
        return 1.0;
    };
    if x <= first[0] as f64 {
        return first[1] as f64;
    }
    for pair in points.windows(2) {
        let ([x0, s0], [x1, s1]) = (pair[0], pair[1]);
        let (x0, x1) = (x0 as f64, x1 as f64);
        if x <= x1 {
            let t = if x1 > x0 { (x - x0) / (x1 - x0) } else { 1.0 };
            return s0 as f64 + (s1 as f64 - s0 as f64) * t;
        }
    }
    points[points.len() - 1][1] as f64
}

/// Source covered from 0 to `x`, in "speed × length" units: the integral of
/// the piecewise linear speed.
fn integral(points: &[[f32; 2]], x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    let mut sum = 0.0;
    for pair in points.windows(2) {
        let (x0, x1) = (pair[0][0] as f64, pair[1][0] as f64);
        if x0 >= x {
            break;
        }
        let end = x1.min(x);
        let (s0, s_end) = (speed_at(points, x0), speed_at(points, end));
        sum += (end - x0) * (s0 + s_end) / 2.0;
    }
    sum
}

/// The average speed of the curve: the clip's speed relative to `base_speed`.
pub fn mean(points: &[[f32; 2]]) -> f64 {
    integral(points, 1.0).max(1e-6)
}

/// Share of the source range shown at `x` of the clip's length.
pub fn progress(points: &[[f32; 2]], x: f64) -> f64 {
    (integral(points, x) / mean(points)).clamp(0.0, 1.0)
}

/// Clips sharing a new speed and pitch setting.
type SpeedGroup = (Rational, bool, Vec<(usize, ClipId)>);

/// Lays `points` (or removes the curve, `None`) on `Media` clips: their speed
/// becomes the base one times the curve's average, rippling as a speed
/// change does.
#[derive(Debug)]
pub struct SetClipSpeedCurve {
    pub timeline: TimelineId,
    pub clips: Vec<(usize, ClipId)>,
    pub points: Option<Vec<[f32; 2]>>,
    before: Vec<Track>,
}

impl SetClipSpeedCurve {
    pub fn new(
        timeline: TimelineId,
        clips: Vec<(usize, ClipId)>,
        points: Option<Vec<[f32; 2]>>,
    ) -> Self {
        Self {
            timeline,
            clips,
            points: points.map(|p| normalized(&p)),
            before: Vec::new(),
        }
    }
}

impl Command for SetClipSpeedCurve {
    fn label(&self) -> CommandLabel {
        CommandLabel::SpeedCurve
    }

    fn apply(&mut self, project: &mut Project) {
        self.before = project.timelines[self.timeline].tracks.clone();
        let relative = self.points.as_deref().map_or(1.0, mean);
        // One speed command per speed: linked clips ripple once, together.
        let mut groups: Vec<SpeedGroup> = Vec::new();
        let mut bases = Vec::new();
        for &(track, id) in &self.clips {
            let Some(clip) = project.timelines[self.timeline].clip(track, id) else {
                continue;
            };
            let base = clip
                .speed_curve
                .as_ref()
                .map_or(clip.speed(), |c| c.base_speed);
            let speed =
                Rational::from_percent((base.as_percent() * relative).clamp(1.0, 100_000.0));
            let pitch = clip.pitch_correction;
            match groups
                .iter_mut()
                .find(|(s, p, _)| *s == speed && *p == pitch)
            {
                Some((_, _, clips)) => clips.push((track, id)),
                None => groups.push((speed, pitch, vec![(track, id)])),
            }
            bases.push((track, id, base));
        }
        for (speed, pitch, clips) in groups {
            SetClipSpeed::new(self.timeline, clips, speed, pitch, SpeedFit::Ripple).apply(project);
        }
        for (track, id, base) in bases {
            if let Some(clip) = project.timelines[self.timeline].tracks[track].clip_mut(id) {
                clip.speed_curve = self.points.clone().map(|points| SpeedCurve {
                    points,
                    base_speed: base,
                    range: (clip.source_in(), clip.source_out()),
                });
            }
        }
    }

    fn undo(&self, project: &mut Project) {
        project.timelines[self.timeline].tracks = self.before.clone();
    }
}

#[cfg(test)]
#[path = "tests/speed_curve.rs"]
mod tests;
