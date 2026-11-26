//! Automatic reframe: a framing that follows a face through the clip, from
//! the detections of `vv-reframe`, written as plain zoom/position keyframes
//! tagged `Origin::Reframe`.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::auto_zoom::{AutoKeyframe, Origin};
use crate::{Clip, FrameIdx, Interpolation, Keyframed, TransformParam, TransformTracks};

/// One detected face, in fractions of the media frame (Y down).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub center: [f32; 2],
    pub size: [f32; 2],
    pub score: f32,
    /// Mean luma change of the mouth area from the previous frame, 0..=1.
    pub mouth: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Media frame.
    pub frame: FrameIdx,
    pub faces: Vec<Face>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    /// Media size in pixels.
    pub size: (u32, u32),
    /// Media frames analysed, end exclusive.
    pub range: (FrameIdx, FrameIdx),
    pub step: u32,
    pub samples: Vec<Sample>,
    /// First frame of every shot after the first.
    pub cuts: Vec<FrameIdx>,
}

impl Analysis {
    pub fn scenes(&self) -> Vec<Range<FrameIdx>> {
        let (start, end) = self.range;
        let mut bounds: Vec<FrameIdx> = std::iter::once(start)
            .chain(self.cuts.iter().copied().filter(|&c| c > start && c < end))
            .collect();
        bounds.push(end);
        bounds.windows(2).map(|w| w[0]..w[1]).collect()
    }

    /// The detection shown at `frame`: the last sample at or before it in the same shot.
    pub fn sample_at(&self, frame: FrameIdx) -> Option<&Sample> {
        let scene = self.scenes().into_iter().find(|s| s.contains(&frame))?;
        let i = self.samples.partition_point(|s| s.frame <= frame);
        self.samples[..i]
            .last()
            .filter(|s| scene.contains(&s.frame))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Placement {
    Center,
    UpperThird,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// A face detection every `step` frames.
    pub step: u32,
    /// 0 = lazy, 1 = tight.
    pub follow: f32,
    /// Fraction of the visible width/height the face may drift before the
    /// framing moves.
    pub dead_zone: f32,
    /// Free space around the face, in face heights per side.
    pub margin: f32,
    pub placement: Placement,
    /// Multiplies the zoom that fills the frame.
    pub min_zoom: f32,
    pub follow_speaker: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            step: 5,
            follow: 0.5,
            dead_zone: 0.08,
            margin: 2.0,
            placement: Placement::UpperThird,
            min_zoom: 1.0,
            follow_speaker: true,
        }
    }
}

pub const STEP: std::ops::RangeInclusive<u32> = 1..=30;
pub const DEAD_ZONE: std::ops::RangeInclusive<f32> = 0.0..=0.3;
pub const MARGIN: std::ops::RangeInclusive<f32> = 0.5..=4.0;
pub const MIN_ZOOM: std::ops::RangeInclusive<f32> = 1.0..=2.0;

/// Past the fill zoom, the margin can zoom in at most this much more.
const MAX_OVER_FILL: f32 = 2.0;
/// Fractions of the frame: a face closer than this to the last position is
/// the same person.
const SAME_FACE: f32 = 0.18;
const LOST_SECS: f64 = 1.0;
const SPEAKER_LEAD_SECS: f64 = 1.0;
const SPEAKER_LEAD: f32 = 1.3;
const ACTIVITY_SECS: f64 = 0.7;
/// Timeline pixels a simplified path may stray from the smoothed one.
const PATH_TOLERANCE: f32 = 1.5;

/// "Follow this face from here to the end of its shot", from a click.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pin {
    pub frame: FrameIdx,
    pub at: [f32; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScenePlan {
    pub range: Range<FrameIdx>,
    pub zoom: f32,
    /// Media frame and clip position in timeline pixels.
    pub points: Vec<(FrameIdx, [f32; 2])>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Plan {
    pub scenes: Vec<ScenePlan>,
    /// Media frames where no face was found, merged.
    pub missing: Vec<Range<FrameIdx>>,
}

struct Person {
    center: [f32; 2],
    height: f32,
    activity: f32,
    seen: FrameIdx,
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Faces of one shot followed as people; returns, per sample, the subject.
fn follow_subject(
    samples: &[&Sample],
    pin: Option<&Pin>,
    speech: &dyn Fn(FrameIdx) -> bool,
    follow_speaker: bool,
    fps: f64,
) -> Vec<Option<Face>> {
    let mut people: Vec<Person> = Vec::new();
    let mut subject: Option<usize> = None;
    let mut lost_since: Option<FrameIdx> = None;
    let mut challenger: Option<(usize, FrameIdx)> = None;
    let mut pinned = false;
    let mut last_frame: Option<FrameIdx> = None;
    let lost = (LOST_SECS * fps).round() as FrameIdx;
    let lead = (SPEAKER_LEAD_SECS * fps).round() as FrameIdx;
    samples
        .iter()
        .map(|sample| {
            let dt = last_frame.map_or(0.0, |f| (sample.frame - f) as f64 / fps.max(1e-6));
            last_frame = Some(sample.frame);
            let decay = (-dt / ACTIVITY_SECS).exp() as f32;
            let mut matched: Vec<Option<usize>> = vec![None; sample.faces.len()];
            let mut taken = vec![false; people.len()];
            let mut pairs: Vec<(f32, usize, usize)> = sample
                .faces
                .iter()
                .enumerate()
                .flat_map(|(f, face)| {
                    people
                        .iter()
                        .enumerate()
                        .map(move |(p, person)| (distance(face.center, person.center), f, p))
                })
                .filter(|(d, _, _)| *d < SAME_FACE)
                .collect();
            pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, f, p) in pairs {
                if matched[f].is_none() && !taken[p] {
                    matched[f] = Some(p);
                    taken[p] = true;
                }
            }
            for (f, face) in sample.faces.iter().enumerate() {
                let p = *matched[f].get_or_insert_with(|| {
                    people.push(Person {
                        center: face.center,
                        height: face.size[1],
                        activity: 0.0,
                        seen: sample.frame,
                    });
                    people.len() - 1
                });
                let person = &mut people[p];
                person.center = face.center;
                person.height = face.size[1];
                person.activity = person.activity * decay + face.mouth * (1.0 - decay);
                person.seen = sample.frame;
            }
            let visible = |p: usize, people: &[Person]| people[p].seen == sample.frame;
            let largest = (0..people.len())
                .filter(|&p| visible(p, &people))
                .max_by(|&a, &b| people[a].height.total_cmp(&people[b].height));

            if let Some(pin) = pin.filter(|p| p.frame <= sample.frame && !pinned) {
                pinned = true;
                subject = (0..people.len())
                    .filter(|&p| visible(p, &people))
                    .min_by(|&a, &b| {
                        distance(people[a].center, pin.at)
                            .total_cmp(&distance(people[b].center, pin.at))
                    })
                    .or(subject);
                challenger = None;
            }
            match subject {
                None => subject = largest,
                Some(s) if visible(s, &people) => lost_since = None,
                Some(_) => {
                    let since = *lost_since.get_or_insert(sample.frame);
                    if sample.frame - since >= lost && largest.is_some() {
                        subject = largest;
                        lost_since = None;
                    }
                }
            }
            if follow_speaker && !pinned && speech(sample.frame) {
                let loudest = (0..people.len())
                    .filter(|&p| visible(p, &people))
                    .max_by(|&a, &b| people[a].activity.total_cmp(&people[b].activity));
                match (loudest, subject) {
                    (Some(l), Some(s))
                        if l != s
                            && people[l].activity > people[s].activity * SPEAKER_LEAD + 0.002 =>
                    {
                        match challenger {
                            Some((c, since)) if c == l => {
                                if sample.frame - since >= lead {
                                    subject = Some(l);
                                    challenger = None;
                                }
                            }
                            _ => challenger = Some((l, sample.frame)),
                        }
                    }
                    _ => challenger = None,
                }
            }
            subject.filter(|&s| visible(s, &people)).map(|s| {
                let f = matched.iter().position(|m| *m == Some(s)).unwrap();
                sample.faces[f]
            })
        })
        .collect()
}

/// Ramer–Douglas–Peucker on the position path; keeps both ends.
fn simplify(points: &[(FrameIdx, [f32; 2])], tolerance: f32) -> Vec<(FrameIdx, [f32; 2])> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        let (fa, pa) = points[a];
        let (fb, pb) = points[b];
        let span = (fb - fa).max(1) as f32;
        let mut worst = (0.0f32, a);
        for (i, &(f, p)) in points.iter().enumerate().take(b).skip(a + 1) {
            let t = (f - fa) as f32 / span;
            let lerp = [pa[0] + (pb[0] - pa[0]) * t, pa[1] + (pb[1] - pa[1]) * t];
            let d = (p[0] - lerp[0]).abs().max((p[1] - lerp[1]).abs());
            if d > worst.0 {
                worst = (d, i);
            }
        }
        if worst.0 > tolerance {
            keep[worst.1] = true;
            stack.push((a, worst.1));
            stack.push((worst.1, b));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(p, _)| *p)
        .collect()
}

fn median(mut values: Vec<f32>) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    Some(values[values.len() / 2])
}

/// Framing of every shot for a timeline of `frame` pixels. `speech` tells
/// whether someone is talking at a media frame; `fps` is the media's.
pub fn plan(
    analysis: &Analysis,
    config: &Config,
    pins: &[Pin],
    speech: &dyn Fn(FrameIdx) -> bool,
    fps: f64,
    frame: (u32, u32),
) -> Plan {
    let (sw, sh) = (analysis.size.0.max(1) as f32, analysis.size.1.max(1) as f32);
    let (tw, th) = (frame.0.max(1) as f32, frame.1.max(1) as f32);
    let fit = (tw / sw).min(th / sh);
    let fill = (tw / (sw * fit)).max(th / (sh * fit));
    let rate = 0.6 + f64::from(config.follow.clamp(0.0, 1.0)) * 6.0;
    let mut result = Plan::default();

    for scene in analysis.scenes() {
        let samples: Vec<&Sample> = analysis
            .samples
            .iter()
            .filter(|s| scene.contains(&s.frame))
            .collect();
        let pin = pins
            .iter()
            .filter(|p| scene.contains(&p.frame))
            .max_by_key(|p| p.frame);
        let subjects = follow_subject(&samples, pin, speech, config.follow_speaker, fps);
        for (s, face) in samples.iter().zip(&subjects) {
            if face.is_none() {
                let end = s.frame + FrameIdx::from(analysis.step.max(1));
                match result.missing.last_mut() {
                    Some(last) if last.end >= s.frame => last.end = end.min(scene.end),
                    _ => result.missing.push(s.frame..end.min(scene.end)),
                }
            }
        }
        let height = median(subjects.iter().flatten().map(|f| f.size[1]).collect());
        let mut zoom = fill * config.min_zoom.clamp(*MIN_ZOOM.start(), *MIN_ZOOM.end());
        if let Some(h) = height.filter(|h| *h > 0.0) {
            let margin = config.margin.clamp(*MARGIN.start(), *MARGIN.end());
            let to_fit = th / (h * sh * fit * (1.0 + 2.0 * margin));
            zoom = zoom.max(to_fit.min(fill * MAX_OVER_FILL));
        }
        let visible = [tw / (zoom * sw * fit), th / (zoom * sh * fit)];
        let lift = match config.placement {
            Placement::Center => 0.0,
            Placement::UpperThird => visible[1] / 6.0,
        };
        let clamp_center = |c: [f32; 2]| -> [f32; 2] {
            let axis = |v: f32, size: f32| {
                if size >= 1.0 {
                    0.5
                } else {
                    v.clamp(size / 2.0, 1.0 - size / 2.0)
                }
            };
            [axis(c[0], visible[0]), axis(c[1], visible[1])]
        };
        let position = |c: [f32; 2]| -> [f32; 2] {
            [
                -zoom * (c[0] - 0.5) * sw * fit,
                -zoom * (0.5 - c[1]) * sh * fit,
            ]
        };

        let first_known = subjects.iter().flatten().next().map(|f| f.center);
        let mut last_known = first_known.unwrap_or([0.5, 0.5]);
        let dead = config.dead_zone.clamp(*DEAD_ZONE.start(), *DEAD_ZONE.end());
        let mut goal: Option<[f32; 2]> = None;
        let mut smooth = [0.0f32; 2];
        let mut previous: Option<FrameIdx> = None;
        let mut points = Vec::with_capacity(samples.len() + 2);
        for (s, face) in samples.iter().zip(&subjects) {
            if let Some(f) = face {
                last_known = f.center;
            }
            let target = clamp_center([last_known[0], last_known[1] + lift]);
            match goal.as_mut() {
                None => {
                    goal = Some(target);
                    smooth = target;
                }
                Some(g) => {
                    for axis in 0..2 {
                        let band = dead * visible[axis].min(1.0);
                        let off = target[axis] - g[axis];
                        if off.abs() > band {
                            g[axis] = target[axis] - band * off.signum();
                        }
                    }
                    let dt = (s.frame - previous.unwrap_or(s.frame)) as f64 / fps.max(1e-6);
                    let k = (1.0 - (-dt * rate).exp()) as f32;
                    smooth[0] += (g[0] - smooth[0]) * k;
                    smooth[1] += (g[1] - smooth[1]) * k;
                }
            }
            previous = Some(s.frame);
            points.push((s.frame, position(clamp_center(smooth))));
        }
        if points.is_empty() {
            points.push((scene.start, position(clamp_center([0.5, 0.5 + lift]))));
        }
        if points[0].0 > scene.start {
            let first = points[0].1;
            points.insert(0, (scene.start, first));
        }
        let last = points.last().unwrap().1;
        if points.last().unwrap().0 < scene.end - 1 {
            points.push((scene.end - 1, last));
        }
        result.scenes.push(ScenePlan {
            range: scene,
            zoom,
            points: simplify(&points, PATH_TOLERANCE),
        });
    }
    result
}

const PARAMS: [TransformParam; 4] = [
    TransformParam::ZoomX,
    TransformParam::ZoomY,
    TransformParam::PositionX,
    TransformParam::PositionY,
];

/// Zoom/position keyframes of `clip` not written by the reframe.
pub fn has_other_keyframes(clip: &Clip) -> bool {
    PARAMS.iter().any(|&p| {
        clip.effects.transform.track(p).keyframes().iter().any(|k| {
            !clip
                .auto_keyframes
                .iter()
                .any(|a| a.origin == Origin::Reframe && a.param == p && a.frame == k.0)
        })
    })
}

/// Rotated, offset-anchored or cropped clips: the framing maths assumes none.
pub fn unsupported_transform(clip: &Clip) -> bool {
    let t = &clip.effects.transform;
    [
        TransformParam::Rotation,
        TransformParam::AnchorX,
        TransformParam::AnchorY,
        TransformParam::CropLeft,
        TransformParam::CropTop,
        TransformParam::CropRight,
        TransformParam::CropBottom,
    ]
    .iter()
    .any(|&p| {
        let track = t.track(p);
        !track.is_constant() || track.default.abs() > 1e-4
    })
}

/// The transform of `clip` with its zoom and position replaced by `plan`
/// (every earlier key of those four is dropped; undo brings them back).
pub fn keyframes(clip: &Clip, plan: &Plan) -> (TransformTracks, Vec<AutoKeyframe>) {
    let mut transform = clip.effects.transform.clone();
    for p in PARAMS {
        let rest = if matches!(p, TransformParam::ZoomX | TransformParam::ZoomY) {
            1.0
        } else {
            0.0
        };
        *transform.track_mut(p) = Keyframed::constant(rest);
    }
    let (first, end) = (clip.source_in(), clip.source_out());
    let mut auto = Vec::new();
    let mut put = |transform: &mut TransformTracks,
                   p: TransformParam,
                   frame: FrameIdx,
                   value: f32,
                   interpolation: Interpolation| {
        transform.track_mut(p).upsert(frame, value, interpolation);
        auto.retain(|k: &AutoKeyframe| k.param != p || k.frame != frame);
        auto.push(AutoKeyframe {
            param: p,
            frame,
            value,
            origin: Origin::Reframe,
        });
    };
    for scene in &plan.scenes {
        let range = scene.range.start.max(first)..scene.range.end.min(end);
        if range.is_empty() {
            continue;
        }
        for p in [TransformParam::ZoomX, TransformParam::ZoomY] {
            put(
                &mut transform,
                p,
                range.start,
                scene.zoom,
                Interpolation::Hold,
            );
        }
        let inside: Vec<_> = scene
            .points
            .iter()
            .filter(|(f, _)| range.contains(f))
            .copied()
            .collect();
        let at = |f: FrameIdx| -> [f32; 2] {
            let i = scene.points.partition_point(|(pf, _)| *pf <= f);
            match (
                i.checked_sub(1).map(|i| scene.points[i]),
                scene.points.get(i),
            ) {
                (Some((fa, a)), Some(&(fb, b))) => {
                    let t = (f - fa) as f32 / (fb - fa).max(1) as f32;
                    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
                }
                (Some((_, a)), None) => a,
                (None, Some(&(_, b))) => b,
                (None, None) => [0.0, 0.0],
            }
        };
        let mut path = inside;
        if path.first().is_none_or(|(f, _)| *f > range.start) {
            path.insert(0, (range.start, at(range.start)));
        }
        if path.last().is_none_or(|(f, _)| *f < range.end - 1) {
            path.push((range.end - 1, at(range.end - 1)));
        }
        let count = path.len();
        for (i, (f, pos)) in path.into_iter().enumerate() {
            let interpolation = if i + 1 == count {
                Interpolation::Hold
            } else {
                Interpolation::Linear
            };
            put(
                &mut transform,
                TransformParam::PositionX,
                f,
                pos[0],
                interpolation,
            );
            put(
                &mut transform,
                TransformParam::PositionY,
                f,
                pos[1],
                interpolation,
            );
        }
    }
    (transform, auto)
}

#[cfg(test)]
#[path = "tests/reframe.rs"]
mod tests;

/// Most faces a blur follows at once.
pub const MAX_BLURRED_FACES: usize = 4;

/// Ellipse masks following the faces of `analysis` on a layer of
/// `layer_size` pixels, one per face track: hidden (opacity 0) where its
/// face is not found. Keyframes are at the analysis' media frames.
pub fn face_blur_masks(analysis: &Analysis, layer_size: (u32, u32)) -> Vec<crate::mask::ClipMask> {
    use crate::mask::{ClipMask, MaskParam, MaskShape};
    let (lw, lh) = (layer_size.0 as f32, layer_size.1 as f32);
    // Each track: its last face and the samples it has.
    let mut tracks: Vec<(Face, Vec<(FrameIdx, Face)>)> = Vec::new();
    for sample in &analysis.samples {
        let mut used = vec![false; tracks.len()];
        let mut faces = sample.faces.clone();
        faces.sort_by(|a, b| (b.size[0] * b.size[1]).total_cmp(&(a.size[0] * a.size[1])));
        for face in faces {
            let nearest = tracks
                .iter()
                .enumerate()
                .filter(|(i, _)| !used[*i])
                .map(|(i, (last, _))| {
                    let d =
                        (face.center[0] - last.center[0]).hypot(face.center[1] - last.center[1]);
                    (i, d / last.size[0].max(last.size[1]).max(1e-3))
                })
                .filter(|(_, d)| *d < 1.5)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            match nearest {
                Some((i, _)) => {
                    used[i] = true;
                    tracks[i].0 = face;
                    tracks[i].1.push((sample.frame, face));
                }
                None if tracks.len() < MAX_BLURRED_FACES => {
                    tracks.push((face, vec![(sample.frame, face)]));
                    used.push(true);
                }
                None => {}
            }
        }
    }
    let frames: Vec<FrameIdx> = analysis.samples.iter().map(|s| s.frame).collect();
    tracks
        .into_iter()
        .map(|(_, seen)| {
            let mut mask = ClipMask::new(MaskShape::Ellipse, layer_size);
            *mask.track_mut(MaskParam::Feather) = Keyframed::constant(12.0);
            for &frame in &frames {
                let face = seen
                    .iter()
                    .find(|(f, _)| *f == frame)
                    .map(|(_, face)| *face);
                let at = |track: &mut Keyframed<f32>, v: f32| {
                    track.upsert(frame, v, Interpolation::Linear)
                };
                match face {
                    Some(face) => {
                        at(
                            mask.track_mut(MaskParam::CenterX),
                            (face.center[0] - 0.5) * lw,
                        );
                        at(
                            mask.track_mut(MaskParam::CenterY),
                            (0.5 - face.center[1]) * lh,
                        );
                        at(mask.track_mut(MaskParam::Width), face.size[0] * lw * 1.3);
                        at(mask.track_mut(MaskParam::Height), face.size[1] * lh * 1.5);
                        at(mask.track_mut(MaskParam::Opacity), 100.0);
                    }
                    None => at(mask.track_mut(MaskParam::Opacity), 0.0),
                }
            }
            mask
        })
        .collect()
}
