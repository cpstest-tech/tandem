//! Parameters of the color correction filter (`FilterKind::ColorCorrection`):
//! one `Keyframed<f32>` each, like the transform, so every control animates
//! on its own and the generic keyframe code covers them all.

use serde::{Deserialize, Serialize};

use crate::{FrameIdx, Keyframed};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GradeParam {
    ShadowsX,
    ShadowsY,
    ShadowsLuma,
    ShadowsSat,
    MidtonesX,
    MidtonesY,
    MidtonesLuma,
    MidtonesSat,
    HighlightsX,
    HighlightsY,
    HighlightsLuma,
    HighlightsSat,
    OffsetX,
    OffsetY,
    OffsetLuma,
    Saturation,
    /// Luma where the shadows give way to the midtones.
    LowRange,
    /// Luma where the midtones give way to the highlights.
    HighRange,
    // The "Adjust" controls, applied before the wheels.
    Temperature,
    Tint,
    Brightness,
    Contrast,
    Highlights,
    Shadows,
    Vignette,
    HslRedHue,
    HslRedSat,
    HslRedLum,
    HslOrangeHue,
    HslOrangeSat,
    HslOrangeLum,
    HslYellowHue,
    HslYellowSat,
    HslYellowLum,
    HslGreenHue,
    HslGreenSat,
    HslGreenLum,
    HslCyanHue,
    HslCyanSat,
    HslCyanLum,
    HslBlueHue,
    HslBlueSat,
    HslBlueLum,
    HslPurpleHue,
    HslPurpleSat,
    HslPurpleLum,
    HslMagentaHue,
    HslMagentaSat,
    HslMagentaLum,
    CurveMaster0,
    CurveMaster1,
    CurveMaster2,
    CurveMaster3,
    CurveMaster4,
    CurveRed0,
    CurveRed1,
    CurveRed2,
    CurveRed3,
    CurveRed4,
    CurveGreen0,
    CurveGreen1,
    CurveGreen2,
    CurveGreen3,
    CurveGreen4,
    CurveBlue0,
    CurveBlue1,
    CurveBlue2,
    CurveBlue3,
    CurveBlue4,
    /// How much of the clip's LUT shows, 0..1.
    LutAmount,
    /// Unsharp mask on the luma of video and pictures, 0..1.
    Sharpen,
}

impl GradeParam {
    pub const COUNT: usize = 71;
    pub const ALL: [Self; Self::COUNT] = [
        Self::ShadowsX,
        Self::ShadowsY,
        Self::ShadowsLuma,
        Self::ShadowsSat,
        Self::MidtonesX,
        Self::MidtonesY,
        Self::MidtonesLuma,
        Self::MidtonesSat,
        Self::HighlightsX,
        Self::HighlightsY,
        Self::HighlightsLuma,
        Self::HighlightsSat,
        Self::OffsetX,
        Self::OffsetY,
        Self::OffsetLuma,
        Self::Saturation,
        Self::LowRange,
        Self::HighRange,
        Self::Temperature,
        Self::Tint,
        Self::Brightness,
        Self::Contrast,
        Self::Highlights,
        Self::Shadows,
        Self::Vignette,
        Self::HslRedHue,
        Self::HslRedSat,
        Self::HslRedLum,
        Self::HslOrangeHue,
        Self::HslOrangeSat,
        Self::HslOrangeLum,
        Self::HslYellowHue,
        Self::HslYellowSat,
        Self::HslYellowLum,
        Self::HslGreenHue,
        Self::HslGreenSat,
        Self::HslGreenLum,
        Self::HslCyanHue,
        Self::HslCyanSat,
        Self::HslCyanLum,
        Self::HslBlueHue,
        Self::HslBlueSat,
        Self::HslBlueLum,
        Self::HslPurpleHue,
        Self::HslPurpleSat,
        Self::HslPurpleLum,
        Self::HslMagentaHue,
        Self::HslMagentaSat,
        Self::HslMagentaLum,
        Self::CurveMaster0,
        Self::CurveMaster1,
        Self::CurveMaster2,
        Self::CurveMaster3,
        Self::CurveMaster4,
        Self::CurveRed0,
        Self::CurveRed1,
        Self::CurveRed2,
        Self::CurveRed3,
        Self::CurveRed4,
        Self::CurveGreen0,
        Self::CurveGreen1,
        Self::CurveGreen2,
        Self::CurveGreen3,
        Self::CurveGreen4,
        Self::CurveBlue0,
        Self::CurveBlue1,
        Self::CurveBlue2,
        Self::CurveBlue3,
        Self::CurveBlue4,
        Self::LutAmount,
        Self::Sharpen,
    ];

    /// Position in `GradeTracks::params` and `GradeValue`: the declaration
    /// order, which is that of `ALL`.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The value that leaves the image unchanged.
    pub const fn neutral(self) -> f32 {
        match self {
            Self::ShadowsSat | Self::MidtonesSat | Self::HighlightsSat | Self::Saturation => 1.0,
            Self::LowRange => 1.0 / 3.0,
            Self::HighRange => 2.0 / 3.0,
            Self::CurveMaster0 => 0.0,
            Self::CurveMaster1 => 0.25,
            Self::CurveMaster2 => 0.5,
            Self::CurveMaster3 => 0.75,
            Self::CurveMaster4 => 1.0,
            Self::CurveRed0 => 0.0,
            Self::CurveRed1 => 0.25,
            Self::CurveRed2 => 0.5,
            Self::CurveRed3 => 0.75,
            Self::CurveRed4 => 1.0,
            Self::CurveGreen0 => 0.0,
            Self::CurveGreen1 => 0.25,
            Self::CurveGreen2 => 0.5,
            Self::CurveGreen3 => 0.75,
            Self::CurveGreen4 => 1.0,
            Self::CurveBlue0 => 0.0,
            Self::CurveBlue1 => 0.25,
            Self::CurveBlue2 => 0.5,
            Self::CurveBlue3 => 0.75,
            Self::CurveBlue4 => 1.0,
            Self::LutAmount => 1.0,
            _ => 0.0,
        }
    }

    /// Bounds of the control. The wheel X/Y are further kept inside the
    /// unit disc by the wheel itself.
    pub fn range(self) -> std::ops::RangeInclusive<f32> {
        match self {
            Self::ShadowsSat | Self::MidtonesSat | Self::HighlightsSat | Self::Saturation => {
                0.0..=2.0
            }
            Self::LowRange | Self::HighRange => 0.0..=1.0,
            Self::CurveMaster0
            | Self::CurveMaster1
            | Self::CurveMaster2
            | Self::CurveMaster3
            | Self::CurveMaster4
            | Self::CurveRed0
            | Self::CurveRed1
            | Self::CurveRed2
            | Self::CurveRed3
            | Self::CurveRed4
            | Self::CurveGreen0
            | Self::CurveGreen1
            | Self::CurveGreen2
            | Self::CurveGreen3
            | Self::CurveGreen4
            | Self::CurveBlue0
            | Self::CurveBlue1
            | Self::CurveBlue2
            | Self::CurveBlue3
            | Self::CurveBlue4
            | Self::LutAmount
            | Self::Sharpen => 0.0..=1.0,
            _ => -1.0..=1.0,
        }
    }
}

/// The four wheels: a color shift (X/Y on the disc) and a luminance shift
/// each, plus the saturation shown with them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradeWheel {
    Shadows,
    Midtones,
    Highlights,
    /// Applies to the whole image, whatever its luma.
    Offset,
}

impl GradeWheel {
    pub const ALL: [Self; 4] = [
        Self::Shadows,
        Self::Midtones,
        Self::Highlights,
        Self::Offset,
    ];

    pub fn x(self) -> GradeParam {
        match self {
            Self::Shadows => GradeParam::ShadowsX,
            Self::Midtones => GradeParam::MidtonesX,
            Self::Highlights => GradeParam::HighlightsX,
            Self::Offset => GradeParam::OffsetX,
        }
    }

    pub fn y(self) -> GradeParam {
        match self {
            Self::Shadows => GradeParam::ShadowsY,
            Self::Midtones => GradeParam::MidtonesY,
            Self::Highlights => GradeParam::HighlightsY,
            Self::Offset => GradeParam::OffsetY,
        }
    }

    pub fn luma(self) -> GradeParam {
        match self {
            Self::Shadows => GradeParam::ShadowsLuma,
            Self::Midtones => GradeParam::MidtonesLuma,
            Self::Highlights => GradeParam::HighlightsLuma,
            Self::Offset => GradeParam::OffsetLuma,
        }
    }

    /// For `Offset`, the global saturation.
    pub fn saturation(self) -> GradeParam {
        match self {
            Self::Shadows => GradeParam::ShadowsSat,
            Self::Midtones => GradeParam::MidtonesSat,
            Self::Highlights => GradeParam::HighlightsSat,
            Self::Offset => GradeParam::Saturation,
        }
    }

    pub fn params(self) -> [GradeParam; 4] {
        [self.x(), self.y(), self.luma(), self.saturation()]
    }

    /// The wheel `param` belongs to; `None` for the ranges.
    pub fn of(param: GradeParam) -> Option<Self> {
        Self::ALL.into_iter().find(|w| w.params().contains(&param))
    }
}

/// Every `GradeParam` evaluated at one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradeValue(pub [f32; GradeParam::COUNT]);

impl GradeValue {
    pub const NEUTRAL: Self = {
        let mut values = [0.0; GradeParam::COUNT];
        let mut i = 0;
        while i < GradeParam::COUNT {
            values[i] = GradeParam::ALL[i].neutral();
            i += 1;
        }
        Self(values)
    };

    pub fn get(&self, param: GradeParam) -> f32 {
        self.0[param.index()]
    }

    pub fn set(&mut self, param: GradeParam, value: f32) {
        self.0[param.index()] = value;
    }
}

/// The eight hue bands of the HSL controls, red first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HslBand {
    Red,
    Orange,
    Yellow,
    Green,
    Cyan,
    Blue,
    Purple,
    Magenta,
}

impl HslBand {
    pub const ALL: [Self; 8] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Cyan,
        Self::Blue,
        Self::Purple,
        Self::Magenta,
    ];

    /// Centre hue, in turns (0..1).
    pub const fn hue(self) -> f32 {
        match self {
            Self::Red => 0.0,
            Self::Orange => 30.0 / 360.0,
            Self::Yellow => 60.0 / 360.0,
            Self::Green => 120.0 / 360.0,
            Self::Cyan => 180.0 / 360.0,
            Self::Blue => 240.0 / 360.0,
            Self::Purple => 270.0 / 360.0,
            Self::Magenta => 300.0 / 360.0,
        }
    }

    /// Hue shift, saturation and luminance of the band.
    pub const fn params(self) -> [GradeParam; 3] {
        match self {
            Self::Red => [
                GradeParam::HslRedHue,
                GradeParam::HslRedSat,
                GradeParam::HslRedLum,
            ],
            Self::Orange => [
                GradeParam::HslOrangeHue,
                GradeParam::HslOrangeSat,
                GradeParam::HslOrangeLum,
            ],
            Self::Yellow => [
                GradeParam::HslYellowHue,
                GradeParam::HslYellowSat,
                GradeParam::HslYellowLum,
            ],
            Self::Green => [
                GradeParam::HslGreenHue,
                GradeParam::HslGreenSat,
                GradeParam::HslGreenLum,
            ],
            Self::Cyan => [
                GradeParam::HslCyanHue,
                GradeParam::HslCyanSat,
                GradeParam::HslCyanLum,
            ],
            Self::Blue => [
                GradeParam::HslBlueHue,
                GradeParam::HslBlueSat,
                GradeParam::HslBlueLum,
            ],
            Self::Purple => [
                GradeParam::HslPurpleHue,
                GradeParam::HslPurpleSat,
                GradeParam::HslPurpleLum,
            ],
            Self::Magenta => [
                GradeParam::HslMagentaHue,
                GradeParam::HslMagentaSat,
                GradeParam::HslMagentaLum,
            ],
        }
    }
}

/// The curves of the "Adjust" tab: five handles each, at inputs 0, ¼, ½, ¾, 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveChannel {
    Master,
    Red,
    Green,
    Blue,
}

impl CurveChannel {
    pub const ALL: [Self; 4] = [Self::Master, Self::Red, Self::Green, Self::Blue];

    pub const fn params(self) -> [GradeParam; 5] {
        match self {
            Self::Master => [
                GradeParam::CurveMaster0,
                GradeParam::CurveMaster1,
                GradeParam::CurveMaster2,
                GradeParam::CurveMaster3,
                GradeParam::CurveMaster4,
            ],
            Self::Red => [
                GradeParam::CurveRed0,
                GradeParam::CurveRed1,
                GradeParam::CurveRed2,
                GradeParam::CurveRed3,
                GradeParam::CurveRed4,
            ],
            Self::Green => [
                GradeParam::CurveGreen0,
                GradeParam::CurveGreen1,
                GradeParam::CurveGreen2,
                GradeParam::CurveGreen3,
                GradeParam::CurveGreen4,
            ],
            Self::Blue => [
                GradeParam::CurveBlue0,
                GradeParam::CurveBlue1,
                GradeParam::CurveBlue2,
                GradeParam::CurveBlue3,
                GradeParam::CurveBlue4,
            ],
        }
    }
}

/// The basic "Adjust" sliders, in the order they are shown.
pub const BASIC_ADJUSTMENTS: [GradeParam; 8] = [
    GradeParam::Temperature,
    GradeParam::Tint,
    GradeParam::Brightness,
    GradeParam::Contrast,
    GradeParam::Highlights,
    GradeParam::Shadows,
    GradeParam::Vignette,
    GradeParam::Sharpen,
];

impl GradeValue {
    /// Whether any "Adjust" control moved: the shader skips them otherwise.
    pub fn adjusts(&self) -> bool {
        GradeParam::ALL[GradeParam::Temperature.index()..]
            .iter()
            .any(|p| self.get(*p) != p.neutral())
    }
}

/// Uniform Catmull-Rom through five values at 0, ¼, ½, ¾, 1. Mirrored in transform.wgsl.
pub fn curve_at(knots: [f32; 5], x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0) * 4.0;
    let i = (x.floor() as usize).min(3);
    let t = x - i as f32;
    // Linear extrapolation past the ends: straight knots give a straight line.
    let k = |j: isize| match j {
        -1 => 2.0 * knots[0] - knots[1],
        5 => 2.0 * knots[4] - knots[3],
        j => knots[j as usize],
    };
    let (p0, p1, p2, p3) = (
        k(i as isize - 1),
        k(i as isize),
        k(i as isize + 1),
        k(i as isize + 2),
    );
    let t2 = t * t;
    let t3 = t2 * t;
    let v = 0.5
        * (2.0 * p1
            + (p2 - p0) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
            + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3);
    v.clamp(0.0, 1.0)
}

fn rgb_to_hsv([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= 1e-6 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let s = if max <= 1e-6 { 0.0 } else { d / max };
    [h, s, max]
}

fn hsv_to_rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    let f = |n: f32| {
        let k = (n + h * 6.0).rem_euclid(6.0);
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [f(5.0), f(3.0), f(1.0)]
}

/// Weight of `band` for a pixel of hue `hue` (turns): 1 at its centre,
/// fading to 0 at the neighbouring centres.
pub fn band_weight(band: HslBand, hue: f32) -> f32 {
    let bands = HslBand::ALL;
    let i = bands.iter().position(|b| *b == band).unwrap_or(0);
    let center = band.hue();
    let next = bands[(i + 1) % 8].hue() + if i == 7 { 1.0 } else { 0.0 };
    let prev = bands[(i + 7) % 8].hue() - if i == 0 { 1.0 } else { 0.0 };
    let mut d = hue - center;
    d -= d.round();
    if d >= 0.0 {
        (1.0 - d / (next - center)).max(0.0)
    } else {
        (1.0 + d / (center - prev)).max(0.0)
    }
}

/// The "Adjust" controls on one pixel at `uv` (0..1 over the clip). Mirrored
/// by `apply_adjust` in transform.wgsl.
pub fn apply_adjust(rgb: [f32; 3], grade: &GradeValue, uv: [f32; 2]) -> [f32; 3] {
    let g = |p: GradeParam| grade.get(p);
    let mut c = rgb;
    let temperature = g(GradeParam::Temperature);
    let tint = g(GradeParam::Tint);
    c[0] *= 1.0 + 0.2 * temperature;
    c[2] *= 1.0 - 0.2 * temperature;
    c[1] *= 1.0 - 0.2 * tint;
    let gamma = (-g(GradeParam::Brightness)).exp2();
    c = c.map(|v| v.clamp(0.0, 1.0).powf(gamma));
    let contrast = 1.0 + g(GradeParam::Contrast);
    c = c.map(|v| (v - 0.5) * contrast + 0.5);
    let smooth = |e0: f32, e1: f32, x: f32| {
        let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let y = luma(c);
    let lift = 0.25
        * (g(GradeParam::Highlights) * smooth(0.5, 1.0, y)
            + g(GradeParam::Shadows) * (1.0 - smooth(0.0, 0.5, y)));
    c = c.map(|v| (v + lift).clamp(0.0, 1.0));

    let [h, s, v] = rgb_to_hsv(c);
    let (mut hue_shift, mut sat, mut lum) = (0.0, 0.0, 0.0);
    for band in HslBand::ALL {
        // Greys have no hue: the bands fade in over the first fifth of saturation.
        let w = band_weight(band, h) * (s * 5.0).min(1.0);
        let [ph, ps, pl] = band.params();
        hue_shift += w * g(ph);
        sat += w * g(ps);
        lum += w * g(pl);
    }
    let shifted = hsv_to_rgb([
        (h + hue_shift / 12.0).rem_euclid(1.0),
        (s * (1.0 + sat)).clamp(0.0, 1.0),
        v,
    ]);
    c = shifted.map(|v| (v + 0.3 * lum).clamp(0.0, 1.0));

    let master = CurveChannel::Master.params().map(g);
    for (i, channel) in [CurveChannel::Red, CurveChannel::Green, CurveChannel::Blue]
        .into_iter()
        .enumerate()
    {
        c[i] = curve_at(channel.params().map(g), curve_at(master, c[i]));
    }

    let vignette = g(GradeParam::Vignette);
    let d =
        ((uv[0] - 0.5).powi(2) + (uv[1] - 0.5).powi(2)).sqrt() / std::f32::consts::FRAC_1_SQRT_2;
    let edge = smooth(0.3, 1.0, d);
    if vignette >= 0.0 {
        c = c.map(|v| v * (1.0 - vignette * edge));
    } else {
        c = c.map(|v| v + (-vignette) * edge * (1.0 - v));
    }
    c.map(|v| v.clamp(0.0, 1.0))
}

/// `grade` with brightness, contrast, saturation and sharpen moved so the
/// picture of `pixels` (RGB 0–1, measured with `grade` applied) fills the
/// range with a mid-grey median: a one-click starting point.
pub fn auto_enhance(grade: &GradeValue, pixels: impl Iterator<Item = [f32; 3]>) -> GradeValue {
    let mut lumas = Vec::new();
    let mut chroma = 0.0f64;
    for p in pixels {
        lumas.push(luma(p));
        let max = p[0].max(p[1]).max(p[2]);
        let min = p[0].min(p[1]).min(p[2]);
        chroma += (max - min) as f64;
    }
    let mut out = *grade;
    if lumas.len() < 16 {
        return out;
    }
    let mean_chroma = (chroma / lumas.len() as f64) as f32;
    lumas.sort_by(f32::total_cmp);
    let at = |q: f32| lumas[((lumas.len() - 1) as f32 * q) as usize];
    let (low, median, high) = (at(0.01), at(0.5), at(0.99));
    let add = |out: &mut GradeValue, p: GradeParam, delta: f32| {
        let range = p.range();
        out.set(p, (out.get(p) + delta).clamp(*range.start(), *range.end()));
    };
    // Gamma that takes the median to 0.45: brightness b is a gamma of 2^-b.
    if median > 0.02 && median < 0.98 {
        let gamma = 0.45f32.ln() / median.ln();
        add(
            &mut out,
            GradeParam::Brightness,
            (-gamma.log2()).clamp(-0.5, 0.5),
        );
    }
    let spread = (high - low).max(0.05);
    add(
        &mut out,
        GradeParam::Contrast,
        (0.9 / spread - 1.0).clamp(0.0, 0.4),
    );
    if mean_chroma < 0.25 {
        let saturation = out.get(GradeParam::Saturation);
        out.set(GradeParam::Saturation, (saturation * 1.15).min(2.0));
    }
    add(&mut out, GradeParam::Sharpen, 0.25);
    out
}

/// RGB shift of a wheel's puck at the edge of the disc, and of its
/// luminance slider at the end of its travel.
pub const WHEEL_CHROMA_STRENGTH: f32 = 0.2;
pub const WHEEL_LUMA_STRENGTH: f32 = 0.5;

/// Rec.709 luma weights, the luma the ranges and the saturation are measured on.
const LUMA_WEIGHTS: [f32; 3] = [0.2126, 0.7152, 0.0722];
/// Rec.709: Cb = (B − Y) / CB_SCALE, Cr = (R − Y) / CR_SCALE, each in
/// -0.5..0.5. Mirrored in color.wgsl.
const CB_SCALE: f32 = 1.8556;
const CR_SCALE: f32 = 1.5748;

/// The Cb/Cr of `rgb`.
pub fn cb_cr(rgb: [f32; 3]) -> [f32; 2] {
    let y = luma(rgb);
    [(rgb[2] - y) / CB_SCALE, (rgb[0] - y) / CR_SCALE]
}

/// The color a puck at `(x, y)` adds, with no change in luma: X is Cb and Y
/// is Cr, as on a vectorscope, so the wheel reads like one. The strength
/// grows with the square of the distance from the center, for fine control
/// where the small corrections are.
pub fn chroma_shift(x: f32, y: f32) -> [f32; 3] {
    let [cb, cr] = wheel_chroma(x, y);
    let green = |k: f32, scale: f32| -k * scale / LUMA_WEIGHTS[1];
    [
        CR_SCALE * cr,
        green(LUMA_WEIGHTS[2], CB_SCALE) * cb + green(LUMA_WEIGHTS[0], CR_SCALE) * cr,
        CB_SCALE * cb,
    ]
}

/// The Cb/Cr a puck at `(x, y)` adds.
pub fn wheel_chroma(x: f32, y: f32) -> [f32; 2] {
    let gain = (x * x + y * y).sqrt() * WHEEL_CHROMA_STRENGTH;
    [x * gain, y * gain]
}

/// The puck adding `[cb, cr]`, the inverse of `wheel_chroma`: at the edge of
/// the disc if that much is out of its reach.
pub fn wheel_for_chroma([cb, cr]: [f32; 2]) -> (f32, f32) {
    let length = (cb * cb + cr * cr).sqrt();
    if length < 1e-9 {
        return (0.0, 0.0);
    }
    let radius = (length / WHEEL_CHROMA_STRENGTH).sqrt().min(1.0);
    (cb / length * radius, cr / length * radius)
}

/// Shadows, midtones and highlights weights of a pixel of luma `luma`: soft,
/// summing to 1. Mirrors `range_weights` in transform.wgsl.
pub fn range_weights(luma: f32, low: f32, high: f32) -> [f32; 3] {
    let high = high.max(low);
    let soft = (0.5 * low.min(high - low).min(1.0 - high)).max(0.001);
    let smoothstep = |e0: f32, e1: f32, x: f32| {
        let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let shadows = 1.0 - smoothstep(low - soft, low + soft, luma);
    let highlights = smoothstep(high - soft, high + soft, luma);
    [shadows, 1.0 - shadows - highlights, highlights]
}

/// Rec.709 luma of `rgb`.
pub fn luma(rgb: [f32; 3]) -> f32 {
    rgb.iter().zip(LUMA_WEIGHTS).map(|(c, w)| c * w).sum()
}

/// `grade` applied to one pixel on the CPU: the same as `apply_grade` in
/// transform.wgsl, which the compositor tests check it against.
pub fn apply_grade(rgb: [f32; 3], grade: &GradeValue) -> [f32; 3] {
    apply_grade_at(rgb, grade, [0.5, 0.5])
}

/// `apply_grade` at `uv` of the clip, where the vignette is measured.
pub fn apply_grade_at(rgb: [f32; 3], grade: &GradeValue, uv: [f32; 2]) -> [f32; 3] {
    let rgb = if grade.adjusts() {
        apply_adjust(rgb, grade, uv)
    } else {
        rgb
    };
    let [shadows, midtones, highlights] = range_weights(
        luma(rgb),
        grade.get(GradeParam::LowRange),
        grade.get(GradeParam::HighRange),
    );
    let weights = [shadows, midtones, highlights, 1.0];
    let mut c = rgb;
    for (wheel, weight) in GradeWheel::ALL.iter().zip(weights) {
        let shift = grade.wheel_shift(*wheel);
        for i in 0..3 {
            c[i] += weight * shift[i];
        }
    }
    let saturation = (0..3)
        .map(|i| weights[i] * grade.get(GradeWheel::ALL[i].saturation()))
        .sum::<f32>()
        * grade.get(GradeParam::Saturation);
    let y = luma(c);
    c.map(|v| (y + (v - y) * saturation).clamp(0.0, 1.0))
}

/// `grade` with the shadows, midtones and highlights wheels moved so that
/// the surfaces of `pixels` (RGB 0–1, measured with `grade` already applied)
/// that should be neutral become grey. Those are found per range as the
/// pixels close in color to the range's median: a large colored object (a
/// shirt, a poster) is left out instead of pulling the whole picture towards
/// its opposite, as a plain mean would. The ranges overlap, so a wheel also
/// moves its neighbours: the three moves are solved together. Near-black and
/// near-white pixels do not count; a range without enough neutral pixels
/// keeps its wheel. Measured after the grade, it corrects what is left.
pub fn auto_balance(grade: &GradeValue, pixels: impl Iterator<Item = [f32; 3]>) -> GradeValue {
    const CLIPPED: f32 = 1.0 / 255.0;
    // How far in Cb/Cr from the cast a pixel may be and still count as a
    // neutral surface: a grey wall's noise, not skin or a colored object.
    const NEUTRAL: f32 = 0.035;
    const RANGES: [GradeWheel; 3] = [
        GradeWheel::Shadows,
        GradeWheel::Midtones,
        GradeWheel::Highlights,
    ];
    struct Sample {
        chroma: [f32; 2],
        weights: [f32; 3],
        saturation: f32,
    }
    let (low, high) = (
        grade.get(GradeParam::LowRange),
        grade.get(GradeParam::HighRange),
    );
    let saturations = RANGES.map(|w| grade.get(w.saturation()));
    let global_saturation = grade.get(GradeParam::Saturation);
    let samples: Vec<Sample> = pixels
        .filter_map(|rgb| {
            let y = luma(rgb);
            if y <= CLIPPED || y >= 1.0 - CLIPPED {
                return None;
            }
            let weights = range_weights(y, low, high);
            let saturation = weights
                .iter()
                .zip(saturations)
                .map(|(w, s)| w * s)
                .sum::<f32>()
                * global_saturation;
            Some(Sample {
                chroma: cb_cr(rgb),
                weights,
                saturation,
            })
        })
        .collect();

    // The cast of each range: first the median color of its pixels, then
    // the mean of the pixels near it, twice.
    let mut casts = [[0.0f32; 2]; 3];
    for (r, cast) in casts.iter_mut().enumerate() {
        for (axis, value) in cast.iter_mut().enumerate() {
            let mut values: Vec<f32> = samples
                .iter()
                .filter(|s| s.weights[r] >= 0.5)
                .map(|s| s.chroma[axis])
                .collect();
            if !values.is_empty() {
                let middle = values.len() / 2;
                *value = *values.select_nth_unstable_by(middle, f32::total_cmp).1;
            }
        }
    }
    // Per range r, over the pixels neutral around `casts`: Σ w_r, Σ w_r·Cb,
    // Σ w_r·Cr, and how much a move of each wheel t shows in it:
    // Σ w_r·w_t·saturation.
    struct Sums {
        weight: [f64; 3],
        chroma: [[f64; 2]; 3],
        effect: [[f64; 3]; 3],
    }
    let sums_around = |casts: &[[f32; 2]; 3]| {
        let mut sums = Sums {
            weight: [0.0; 3],
            chroma: [[0.0; 2]; 3],
            effect: [[0.0; 3]; 3],
        };
        for s in &samples {
            let expected =
                [0, 1].map(|axis| (0..3).map(|r| s.weights[r] * casts[r][axis]).sum::<f32>());
            let d = [s.chroma[0] - expected[0], s.chroma[1] - expected[1]];
            if d[0] * d[0] + d[1] * d[1] >= NEUTRAL * NEUTRAL {
                continue;
            }
            let w = s.weights.map(f64::from);
            for r in 0..3 {
                sums.weight[r] += w[r];
                for axis in 0..2 {
                    sums.chroma[r][axis] += w[r] * s.chroma[axis] as f64;
                }
                for t in 0..3 {
                    sums.effect[r][t] += w[r] * w[t] * s.saturation as f64;
                }
            }
        }
        sums
    };
    for _ in 0..2 {
        let sums = sums_around(&casts);
        for (r, cast) in casts.iter_mut().enumerate() {
            if sums.weight[r] >= 1.0 {
                *cast = sums.chroma[r].map(|c| (c / sums.weight[r]) as f32);
            }
        }
    }
    let Sums {
        weight,
        chroma,
        effect,
    } = sums_around(&casts);

    // Ranges with (almost) no neutral pixels, or desaturated, are left alone.
    let solved: Vec<usize> = (0..3)
        .filter(|&r| weight[r] >= 1.0 && effect[r][r] / weight[r] >= 0.01)
        .collect();
    if solved.is_empty() {
        return *grade;
    }
    let matrix: Vec<Vec<f64>> = solved
        .iter()
        .map(|&r| solved.iter().map(|&t| effect[r][t] / weight[r]).collect())
        .collect();
    let targets = solved
        .iter()
        .map(|&r| chroma[r].map(|c| -c / weight[r]))
        .collect();
    let Some(moves) = solve(matrix, targets) else {
        return *grade;
    };
    let mut balanced = *grade;
    for (&r, [cb_move, cr_move]) in solved.iter().zip(moves) {
        let wheel = RANGES[r];
        let [cb, cr] = wheel_chroma(grade.get(wheel.x()), grade.get(wheel.y()));
        // Out of the wheel's reach, `wheel_for_chroma` stops at its edge.
        let (x, y) = wheel_for_chroma([cb + cb_move as f32, cr + cr_move as f32]);
        balanced.set(wheel.x(), x);
        balanced.set(wheel.y(), y);
    }
    balanced
}

/// `matrix · x = target` for both columns of `target`, by Gaussian
/// elimination with partial pivoting; `None` if singular.
fn solve(mut matrix: Vec<Vec<f64>>, mut target: Vec<[f64; 2]>) -> Option<Vec<[f64; 2]>> {
    let n = target.len();
    for col in 0..n {
        let pivot =
            (col..n).max_by(|&a, &b| matrix[a][col].abs().total_cmp(&matrix[b][col].abs()))?;
        if matrix[pivot][col].abs() < 1e-9 {
            return None;
        }
        matrix.swap(col, pivot);
        target.swap(col, pivot);
        for row in col + 1..n {
            let factor = matrix[row][col] / matrix[col][col];
            let (upper, lower) = matrix.split_at_mut(row);
            for (cell, pivot) in lower[0].iter_mut().zip(&upper[col]).skip(col) {
                *cell -= factor * pivot;
            }
            let pivot_target = target[col];
            for (value, pivot) in target[row].iter_mut().zip(pivot_target) {
                *value -= factor * pivot;
            }
        }
    }
    let mut x = vec![[0.0; 2]; n];
    for row in (0..n).rev() {
        x[row] = [0, 1].map(|k| {
            let rest: f64 = (row + 1..n).map(|j| matrix[row][j] * x[j][k]).sum();
            (target[row][k] - rest) / matrix[row][row]
        });
    }
    Some(x)
}

impl GradeValue {
    /// What `wheel` adds to a pixel fully in its range.
    pub fn wheel_shift(&self, wheel: GradeWheel) -> [f32; 3] {
        let chroma = chroma_shift(self.get(wheel.x()), self.get(wheel.y()));
        let luma = self.get(wheel.luma()) * WHEEL_LUMA_STRENGTH;
        chroma.map(|c| c + luma)
    }
}

impl Default for GradeValue {
    fn default() -> Self {
        Self::NEUTRAL
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradePreset {
    Neutral,
    BlackAndWhite,
}

impl GradePreset {
    pub const ALL: [Self; 2] = [Self::Neutral, Self::BlackAndWhite];

    pub const fn value(self) -> GradeValue {
        let mut value = GradeValue::NEUTRAL;
        if matches!(self, Self::BlackAndWhite) {
            value.0[GradeParam::Saturation.index()] = 0.0;
        }
        value
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GradeTracks {
    /// One per `GradeParam`, in the order of `GradeParam::ALL`.
    params: Vec<Keyframed<f32>>,
}

/// Saved by name, so the order of `GradeParam` is free to change.
impl Serialize for GradeTracks {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Tracks<'a>(&'a [Keyframed<f32>]);
        impl Serialize for Tracks<'_> {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_map(GradeParam::ALL.iter().zip(self.0))
            }
        }
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("GradeTracks", 1)?;
        state.serialize_field("tracks", &Tracks(&self.params))?;
        state.end()
    }
}

/// A param missing from the file (saved before it existed) is neutral.
impl<'de> Deserialize<'de> for GradeTracks {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Repr {
            tracks: std::collections::HashMap<GradeParam, Keyframed<f32>>,
        }
        let mut tracks = Repr::deserialize(deserializer)?.tracks;
        Ok(Self {
            params: GradeParam::ALL
                .iter()
                .map(|p| {
                    tracks
                        .remove(p)
                        .unwrap_or_else(|| Keyframed::constant(p.neutral()))
                })
                .collect(),
        })
    }
}

impl Default for GradeTracks {
    fn default() -> Self {
        Self::constant(GradeValue::NEUTRAL)
    }
}

impl GradeTracks {
    pub fn constant(value: GradeValue) -> Self {
        Self {
            params: value.0.iter().map(|v| Keyframed::constant(*v)).collect(),
        }
    }

    pub fn track(&self, param: GradeParam) -> &Keyframed<f32> {
        &self.params[param.index()]
    }

    pub fn track_mut(&mut self, param: GradeParam) -> &mut Keyframed<f32> {
        &mut self.params[param.index()]
    }

    /// `Neutral` resets every param; another preset sets only the params it
    /// moves from neutral, so black and white on a graded clip only
    /// desaturates it.
    pub fn apply_preset(&mut self, preset: GradePreset) {
        let value = preset.value();
        for (param, track) in GradeParam::ALL.iter().zip(&mut self.params) {
            if preset == GradePreset::Neutral || value.get(*param) != param.neutral() {
                *track = Keyframed::constant(value.get(*param));
            }
        }
    }

    pub fn tracks_mut(&mut self) -> impl Iterator<Item = &mut Keyframed<f32>> {
        self.params.iter_mut()
    }

    pub fn value_at(&self, frame: FrameIdx) -> GradeValue {
        let mut value = GradeValue::NEUTRAL;
        for (slot, track) in value.0.iter_mut().zip(&self.params) {
            *slot = track.value_at(frame);
        }
        value
    }
}

#[cfg(test)]
#[path = "tests/grade.rs"]
mod tests;

/// Mean and spread of the picture's luma and chroma: what a match compares.
fn color_stats(pixels: &[[f32; 3]]) -> [f32; 5] {
    let n = pixels.len().max(1) as f32;
    let mut sum = [0.0f32; 5];
    for p in pixels {
        let y = luma(*p);
        let (cb, cr) = ((p[2] - y) / CB_SCALE, (p[0] - y) / CR_SCALE);
        sum[0] += y;
        sum[1] += y * y;
        sum[2] += cb;
        sum[3] += cr;
        sum[4] += cb.hypot(cr);
    }
    let mean = sum[0] / n;
    [
        mean,
        (sum[1] / n - mean * mean).max(0.0).sqrt(),
        sum[2] / n,
        sum[3] / n,
        sum[4] / n,
    ]
}

/// `grade` changed so that `target` (RGB 0–1 of the clip without `grade`'s
/// matched params) looks like `reference`: brightness, contrast, saturation
/// and the offset wheel's colour, found by descent on the CPU reference.
pub fn match_color(grade: &GradeValue, target: &[[f32; 3]], reference: &[[f32; 3]]) -> GradeValue {
    const PARAMS: [GradeParam; 5] = [
        GradeParam::Brightness,
        GradeParam::Contrast,
        GradeParam::Saturation,
        GradeParam::OffsetX,
        GradeParam::OffsetY,
    ];
    let want = color_stats(reference);
    let weights = [4.0, 3.0, 6.0, 6.0, 3.0];
    let cost = |g: &GradeValue| {
        let graded: Vec<[f32; 3]> = target.iter().map(|p| apply_grade(*p, g)).collect();
        let got = color_stats(&graded);
        (0..5)
            .map(|i| weights[i] * (got[i] - want[i]).powi(2))
            .sum::<f32>()
    };
    let mut best = *grade;
    for p in PARAMS {
        best.set(p, p.neutral());
    }
    let mut best_cost = cost(&best);
    let mut step = 0.2f32;
    while step > 0.002 {
        let mut improved = false;
        for p in PARAMS {
            for dir in [-1.0, 1.0] {
                let mut trial = best;
                let range = p.range();
                trial.set(
                    p,
                    (best.get(p) + dir * step).clamp(*range.start(), *range.end()),
                );
                let c = cost(&trial);
                if c < best_cost {
                    best = trial;
                    best_cost = c;
                    improved = true;
                }
            }
        }
        if !improved {
            step *= 0.5;
        }
    }
    best
}
