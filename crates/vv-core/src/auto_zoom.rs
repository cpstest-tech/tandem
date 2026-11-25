//! Automatic punch-in zooms from a transcript and its loudness: the result is
//! plain transform keyframes, tagged in `Clip::auto_keyframes`.

use serde::{Deserialize, Serialize};

use crate::auto_cut::{Energy, Word};
use crate::easing::Easing;
use crate::{
    Clip, ClipId, Command, CommandLabel, FrameIdx, Interpolation, Project, TimelineId,
    TransformParam, TransformTracks,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frequency {
    Low,
    Medium,
    High,
}

impl Frequency {
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];

    pub fn default_gap(self) -> f64 {
        match self {
            Self::Low => 4.0,
            Self::Medium => 2.5,
            Self::High => 1.5,
        }
    }

    fn threshold(self) -> f32 {
        match self {
            Self::Low => 0.6,
            Self::Medium => 0.45,
            Self::High => 0.3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Speed {
    Slow,
    Normal,
    Fast,
}

impl Speed {
    pub const ALL: [Self; 3] = [Self::Slow, Self::Normal, Self::Fast];

    /// Seconds of the punch-in and of the return.
    fn ramps(self) -> (f64, f64) {
        match self {
            Self::Slow => (0.25, 0.7),
            Self::Normal => (0.15, 0.45),
            Self::Fast => (0.08, 0.3),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Peak scale, 1.05..=1.25.
    pub intensity: f32,
    pub speed: Speed,
    pub frequency: Frequency,
    /// Seconds between two punch-ins, against the jitter of close zooms.
    pub min_gap: f64,
    /// Every other zoom uses a softer level.
    pub alternate: bool,
    /// Nudges the centre a little with each zoom.
    pub shift: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            intensity: 1.15,
            speed: Speed::Normal,
            frequency: Frequency::Medium,
            min_gap: Frequency::Medium.default_gap(),
            alternate: true,
            shift: false,
        }
    }
}

pub const INTENSITY: std::ops::RangeInclusive<f32> = 1.05..=1.25;
pub const MIN_GAP: std::ops::RangeInclusive<f64> = 0.5..=10.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutoKeyframe {
    pub param: TransformParam,
    pub frame: FrameIdx,
    pub value: f32,
    #[serde(default, skip_serializing_if = "Origin::is_zoom")]
    pub origin: Origin,
}

/// Which generator wrote a keyframe: each one removes only its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    #[default]
    Zoom,
    Reframe,
    Stabilize,
}

impl Origin {
    fn is_zoom(&self) -> bool {
        *self == Self::Zoom
    }
}

/// One punch-in, in seconds from the start of the clip.
#[derive(Clone, Debug, PartialEq)]
pub struct Zoom {
    pub at: f64,
    pub hold_end: f64,
    pub scale: f32,
    /// Fraction of the free margin, -1..=1 per axis.
    pub shift: [f32; 2],
    pub word: String,
}

const SENTENCE_GAP: f64 = 0.6;
const PHRASE_GAP: f64 = 0.35;
const MIN_HOLD: f64 = 0.6;
const MAX_HOLD: f64 = 2.5;
const CONTEXT: f64 = 4.0;
const SOFT_LEVEL: f32 = 0.55;
const SHIFT_SHARE: f32 = 0.4;

fn peak_db(energy: &Energy, start: f64, end: f64) -> f32 {
    if energy.step <= 0.0 {
        return -120.0;
    }
    let from = (start / energy.step).floor().max(0.0) as usize;
    let to = ((end / energy.step).ceil() as usize).max(from + 1);
    energy
        .db
        .get(from.min(energy.db.len())..to.min(energy.db.len()))
        .and_then(|s| s.iter().copied().reduce(f32::max))
        .unwrap_or(-120.0)
}

fn median(mut values: Vec<f32>) -> f32 {
    if values.is_empty() {
        return -120.0;
    }
    values.sort_by(f32::total_cmp);
    values[values.len() / 2]
}

/// 0..=1 per word: louder than the words around it, sentence start, pause before.
pub fn emphasis(words: &[Word], energy: &Energy) -> Vec<f32> {
    let peaks: Vec<f32> = words
        .iter()
        .map(|w| peak_db(energy, w.start, w.end))
        .collect();
    words
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let context: Vec<f32> = words
                .iter()
                .zip(&peaks)
                .filter(|(o, _)| (o.start - w.start).abs() <= CONTEXT)
                .map(|(_, p)| *p)
                .collect();
            let loud = ((peaks[i] - median(context)) / 6.0).clamp(0.0, 1.0);
            let gap = i
                .checked_sub(1)
                .map_or(f64::INFINITY, |p| w.start - words[p].end);
            let sentence = i == 0
                || gap >= SENTENCE_GAP
                || words[i - 1].text.trim_end().ends_with(['.', '!', '?']);
            let pause = ((gap - 0.25) / 0.5).clamp(0.0, 1.0) as f32;
            let letters = w.text.chars().filter(|c| c.is_alphanumeric()).count();
            let short = if letters < 2 { 0.5 } else { 1.0 };
            (0.5 * loud + 0.3 * f32::from(u8::from(sentence)) + 0.2 * pause) * short
        })
        .collect()
}

/// The punch-ins for a clip `duration` seconds long; words and energy in clip seconds.
pub fn plan(words: &[Word], energy: &Energy, duration: f64, config: &Config) -> Vec<Zoom> {
    let (ramp_in, ramp_out) = config.speed.ramps();
    let scores = emphasis(words, energy);
    let mut order: Vec<usize> = (0..words.len())
        .filter(|&i| {
            scores[i] >= config.frequency.threshold()
                && words[i].start >= 0.0
                && words[i].start + MIN_HOLD <= duration
        })
        .collect();
    order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]).then(a.cmp(&b)));
    let gap = config.min_gap.max(ramp_in + ramp_out);
    let mut chosen: Vec<usize> = Vec::new();
    for i in order {
        if chosen
            .iter()
            .all(|&c| (words[c].start - words[i].start).abs() >= gap)
        {
            chosen.push(i);
        }
    }
    chosen.sort_unstable();
    let soft = 1.0 + (config.intensity - 1.0) * SOFT_LEVEL;
    chosen
        .iter()
        .enumerate()
        .map(|(k, &i)| {
            let at = words[i].start.max(ramp_in.min(duration));
            let mut end = words[i].end;
            for pair in words[i..].windows(2) {
                if pair[1].start - pair[0].end >= PHRASE_GAP || pair[1].end - at > MAX_HOLD {
                    break;
                }
                end = pair[1].end;
            }
            let hold_end = end
                .clamp(at + MIN_HOLD, at + MAX_HOLD)
                .min(duration - ramp_out)
                .max(at);
            let scale = if config.alternate && k % 2 == 1 {
                soft
            } else {
                config.intensity
            };
            let shift = if config.shift {
                const PATTERN: [[f32; 2]; 4] = [[1.0, 0.6], [-1.0, 0.6], [0.8, -0.8], [-0.8, -0.8]];
                PATTERN[k % PATTERN.len()]
            } else {
                [0.0, 0.0]
            };
            Zoom {
                at,
                hold_end,
                scale,
                shift,
                word: words[i].text.trim().to_owned(),
            }
        })
        .collect()
}

const ZOOM_PARAMS: [TransformParam; 2] = [TransformParam::ZoomX, TransformParam::ZoomY];
const SHIFT_PARAMS: [TransformParam; 2] = [TransformParam::PositionX, TransformParam::PositionY];

/// The transform with the keyframes `origin` generated taken out: a
/// keyframe the user moved or changed no longer matches and stays.
pub fn strip(
    transform: &TransformTracks,
    auto: &[AutoKeyframe],
    origin: Origin,
) -> TransformTracks {
    let mut out = transform.clone();
    for k in auto.iter().filter(|k| k.origin == origin) {
        let track = out.track_mut(k.param);
        if track
            .keyframe_at(k.frame)
            .is_some_and(|(v, _)| (v - k.value).abs() < 1e-4)
        {
            track.remove_at(k.frame);
        }
    }
    out
}

/// Keyframes for `zooms` on `clip`, built on its transform without the old
/// automatic ones. A zoom whose span already holds a hand-made keyframe of
/// the same parameters is skipped. `frame` is the timeline size.
pub fn keyframes(
    clip: &Clip,
    zooms: &[Zoom],
    config: &Config,
    fps: f64,
    frame: (u32, u32),
) -> (TransformTracks, Vec<AutoKeyframe>) {
    let mut transform = strip(&clip.effects.transform, &clip.auto_keyframes, Origin::Zoom);
    let base = transform.clone();
    let reframed = |p: TransformParam, f: FrameIdx| {
        clip.auto_keyframes
            .iter()
            .any(|k| k.origin == Origin::Reframe && k.param == p && k.frame == f)
    };
    let (ramp_in, ramp_out) = config.speed.ramps();
    let last = clip.timeline_len - 1;
    let source = |secs: f64| {
        let local = ((secs * fps).round() as FrameIdx).clamp(0, last.max(0));
        clip.source_frame_at(clip.timeline_start + local)
    };
    let params: Vec<TransformParam> = ZOOM_PARAMS
        .into_iter()
        .chain(config.shift.then_some(SHIFT_PARAMS).into_iter().flatten())
        .collect();
    let busy = |from: FrameIdx, to: FrameIdx| {
        params.iter().any(|p| {
            base.track(*p)
                .keyframes()
                .iter()
                .any(|k| (from..=to).contains(&k.0) && !reframed(*p, k.0))
        })
    };
    let zooms: Vec<&Zoom> = zooms
        .iter()
        .filter(|z| !busy(source(z.at - ramp_in), source(z.hold_end + ramp_out)))
        .collect();

    let mut auto: Vec<AutoKeyframe> = clip
        .auto_keyframes
        .iter()
        .filter(|k| k.origin != Origin::Zoom)
        .copied()
        .collect();
    let mut put = |transform: &mut TransformTracks,
                   secs: f64,
                   z: Option<&Zoom>,
                   interpolation: Interpolation| {
        let f = source(secs);
        for &p in &params {
            let rest = base.track(p).value_at(f);
            let value = match (z, p) {
                (None, _) => rest,
                (Some(z), TransformParam::ZoomX | TransformParam::ZoomY) => rest * z.scale,
                (Some(z), p) => {
                    let axis = usize::from(p == TransformParam::PositionY);
                    let size = if axis == 0 { frame.0 } else { frame.1 } as f32;
                    rest + z.shift[axis] * SHIFT_SHARE * (z.scale - 1.0) * size / 2.0
                }
            };
            transform.track_mut(p).upsert(f, value, interpolation);
            auto.retain(|k: &AutoKeyframe| k.param != p || k.frame != f);
            auto.push(AutoKeyframe {
                param: p,
                frame: f,
                value,
                origin: Origin::Zoom,
            });
        }
    };
    let punch = Interpolation::Curve(Easing::CubicOut);
    let mut chained = false;
    for (i, z) in zooms.iter().enumerate() {
        if !chained {
            put(&mut transform, (z.at - ramp_in).max(0.0), None, punch);
        }
        put(&mut transform, z.at, Some(z), Interpolation::Linear);
        let next = zooms.get(i + 1);
        chained = next.is_some_and(|n| z.hold_end + ramp_out > n.at - ramp_in);
        match next.filter(|_| chained) {
            Some(n) => put(&mut transform, n.at - ramp_in, Some(z), punch),
            None => {
                put(
                    &mut transform,
                    z.hold_end,
                    Some(z),
                    Interpolation::Curve(Easing::CubicInOut),
                );
                put(
                    &mut transform,
                    z.hold_end + ramp_out,
                    None,
                    Interpolation::Linear,
                );
            }
        }
    }
    (transform, auto)
}

/// Replaces the transform and the generated-keyframe tags of one clip.
#[derive(Debug)]
pub struct SetAutoZoom {
    pub timeline: TimelineId,
    pub track_index: usize,
    pub clip_id: ClipId,
    value: (TransformTracks, Vec<AutoKeyframe>),
    old: Option<(TransformTracks, Vec<AutoKeyframe>)>,
    label: CommandLabel,
}

impl SetAutoZoom {
    pub fn new(
        timeline: TimelineId,
        track_index: usize,
        clip_id: ClipId,
        transform: TransformTracks,
        auto: Vec<AutoKeyframe>,
    ) -> Self {
        Self {
            timeline,
            track_index,
            clip_id,
            value: (transform, auto),
            old: None,
            label: CommandLabel::AutoZoom,
        }
    }

    /// Takes the keyframes `origin` generated out of the clip; the
    /// hand-made ones and those of the other generator stay.
    pub fn remove(timeline: TimelineId, track_index: usize, clip: &Clip, origin: Origin) -> Self {
        let kept = clip
            .auto_keyframes
            .iter()
            .filter(|k| k.origin != origin)
            .copied()
            .collect();
        Self {
            label: match origin {
                Origin::Zoom => CommandLabel::RemoveAutoZoom,
                Origin::Reframe => CommandLabel::RemoveReframe,
                Origin::Stabilize => CommandLabel::RemoveStabilize,
            },
            ..Self::new(
                timeline,
                track_index,
                clip.id,
                strip(&clip.effects.transform, &clip.auto_keyframes, origin),
                kept,
            )
        }
    }

    pub fn with_label(mut self, label: CommandLabel) -> Self {
        self.label = label;
        self
    }
}

impl Command for SetAutoZoom {
    fn label(&self) -> CommandLabel {
        self.label
    }

    fn apply(&mut self, project: &mut Project) {
        if let Some(clip) =
            project.timelines[self.timeline].clip_mut(self.track_index, self.clip_id)
        {
            let old_transform =
                std::mem::replace(&mut clip.effects.transform, self.value.0.clone());
            let old_auto = std::mem::replace(&mut clip.auto_keyframes, self.value.1.clone());
            self.old = Some((old_transform, old_auto));
        }
    }

    fn undo(&self, project: &mut Project) {
        if let (Some((transform, auto)), Some(clip)) = (
            &self.old,
            project.timelines[self.timeline].clip_mut(self.track_index, self.clip_id),
        ) {
            clip.effects.transform = transform.clone();
            clip.auto_keyframes = auto.clone();
        }
    }
}

#[cfg(test)]
#[path = "tests/auto_zoom.rs"]
mod tests;
