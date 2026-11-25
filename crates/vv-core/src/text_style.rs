//! Decorative style of a title (`TitleParams::style`): fill, strokes,
//! shadows, glows and extrusion. Typography and the base colour stay on
//! `TitleParams`; everything here defaults to "plain text".

use crate::model::{FrameIdx, Keyframed, Rgba};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GradientKind {
    Linear,
    Radial,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    /// 0-1.
    pub pos: f32,
    pub color: Rgba,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub kind: GradientKind,
    /// At least two, sorted by `pos`.
    pub stops: Vec<GradientStop>,
    /// Degrees, 0 = left to right (linear only).
    pub angle: f32,
    /// Scrolls the gradient, in gradient lengths; wraps around.
    #[serde(default = "zero_offset")]
    pub offset: Keyframed<f32>,
}

fn zero_offset() -> Keyframed<f32> {
    Keyframed::constant(0.0)
}

impl Gradient {
    pub fn two_stops(kind: GradientKind, from: Rgba, to: Rgba) -> Self {
        Self {
            kind,
            stops: vec![
                GradientStop {
                    pos: 0.0,
                    color: from,
                },
                GradientStop {
                    pos: 1.0,
                    color: to,
                },
            ],
            angle: 90.0,
            offset: zero_offset(),
        }
    }

    /// The same gradient with the offset frozen at `frame`.
    pub fn at(&self, frame: FrameIdx) -> Self {
        Self {
            offset: Keyframed::constant(self.offset.value_at(frame)),
            ..self.clone()
        }
    }

    pub fn offset_now(&self) -> f32 {
        self.offset.default
    }

    /// Colour at `t` (0-1, clamped), interpolated between the stops.
    pub fn color_at(&self, t: f32) -> Rgba {
        let t = t.clamp(0.0, 1.0);
        let Some(first) = self.stops.first() else {
            return Rgba::WHITE;
        };
        if t <= first.pos {
            return first.color;
        }
        for pair in self.stops.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if t <= b.pos {
                let span = (b.pos - a.pos).max(1e-6);
                return lerp_rgba(a.color, b.color, (t - a.pos) / span);
            }
        }
        self.stops.last().map_or(first.color, |s| s.color)
    }
}

pub fn lerp_rgba(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let l = |x: f32, y: f32| x + (y - x) * t;
    Rgba {
        r: l(a.r, b.r),
        g: l(a.g, b.g),
        b: l(a.b, b.b),
        a: l(a.a, b.a),
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub enum TextFill {
    /// `TitleParams::color`.
    #[default]
    Solid,
    Gradient(Gradient),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StrokeJoin {
    #[default]
    Round,
    Miter,
    Bevel,
}

/// Outline drawn outside the glyphs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStroke {
    pub color: Rgba,
    /// Timeline pixels, measured from the glyph edge.
    pub width: f32,
    pub join: StrokeJoin,
}

impl Default for TextStroke {
    fn default() -> Self {
        Self {
            color: Rgba::BLACK,
            width: 6.0,
            join: StrokeJoin::Round,
        }
    }
}

/// Blur 0 is a hard shadow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextShadow {
    /// Timeline pixels, Y upwards.
    pub offset: [f32; 2],
    pub blur: f32,
    pub color: Rgba,
    /// 0-100.
    pub opacity: f32,
}

impl Default for TextShadow {
    fn default() -> Self {
        Self {
            offset: [6.0, -6.0],
            blur: 8.0,
            color: Rgba::BLACK,
            opacity: 70.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextGlow {
    pub radius: f32,
    /// 0-3: how fast the halo saturates.
    pub intensity: f32,
    pub color: Rgba,
}

impl Default for TextGlow {
    fn default() -> Self {
        Self {
            radius: 16.0,
            intensity: 1.0,
            color: Rgba {
                r: 0.2,
                g: 0.9,
                b: 1.0,
                a: 1.0,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextExtrude {
    pub depth: f32,
    /// Degrees, 0 = right, counter-clockwise.
    pub angle: f32,
    pub color: Rgba,
}

impl Default for TextExtrude {
    fn default() -> Self {
        Self {
            depth: 12.0,
            angle: -45.0,
            color: Rgba::gray(0.2),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct TextStyle {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub underlays: Vec<TextUnderlay>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sticker: Option<TextSticker>,
    #[serde(default)]
    pub fill: TextFill,
    #[serde(default)]
    pub strokes: Vec<TextStroke>,
    #[serde(default)]
    pub shadows: Vec<TextShadow>,
    #[serde(default)]
    pub glows: Vec<TextGlow>,
    #[serde(default)]
    pub extrude: Option<TextExtrude>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextUnderlay {
    pub color: Rgba,
    pub offset: [f32; 2],
    pub style: TextStyle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextSticker {
    pub asset: String,
    /// Size and offset in font sizes, Y upwards.
    pub size: [f32; 2],
    pub offset: [f32; 2],
    #[serde(default)]
    pub rotation: f32,
    #[serde(default)]
    pub start: f32,
}

impl TextStyle {
    pub fn at(&self, frame: FrameIdx) -> Self {
        let mut style = self.clone();
        if let TextFill::Gradient(g) = &mut style.fill {
            *g = g.at(frame);
        }
        for underlay in &mut style.underlays {
            underlay.style = underlay.style.at(frame);
        }
        style
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BackgroundShape {
    #[default]
    RoundedRect,
    Pill,
    Ellipse,
    /// Rounded box with a tail at the bottom left.
    Speech,
    /// Cloud-like ellipse with two bubbles trailing down to the left.
    Thought,
    /// A spiky comic-book shout.
    Burst,
}

impl BackgroundShape {
    /// The speech-bubble shapes of the text tab's quick picks.
    pub const BUBBLES: [Self; 4] = [Self::Speech, Self::Thought, Self::Burst, Self::Ellipse];

    /// Whether parts of it reach outside its box (tail, bubbles, spikes).
    pub fn overhang(self) -> f32 {
        match self {
            Self::RoundedRect | Self::Pill => 1.0,
            Self::Ellipse => 1.0,
            Self::Speech | Self::Thought => 2.0,
            Self::Burst => 1.3,
        }
    }
}
