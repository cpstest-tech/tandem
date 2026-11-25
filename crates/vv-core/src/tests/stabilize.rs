use super::*;
use crate::{ClipId, ClipSource, Rational};

const FPS: f64 = 30.0;
const MEDIA: (u32, u32) = (1920, 1080);

fn motion(steps: Vec<[f32; 3]>, cuts: Vec<FrameIdx>) -> Motion {
    Motion {
        size: MEDIA,
        range: (0, steps.len() as FrameIdx),
        steps,
        cuts,
    }
}

fn clip(len: FrameIdx) -> Clip {
    Clip::from_source_range(
        ClipId(1),
        ClipSource::SolidColor,
        0,
        len,
        0,
        Rational::one(),
    )
}

fn jitter(len: usize, amount: f32) -> Vec<[f32; 3]> {
    (0..len)
        .map(|i| {
            let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
            [sign * amount, -sign * amount, sign * amount * 0.5]
        })
        .collect()
}

/// Where the content of each frame ends up once corrected, in pixels.
fn corrected_x(m: &Motion, shot: &ShotPlan) -> Vec<f32> {
    let w = m.size.0 as f32;
    let mut x = 0.0;
    shot.offsets
        .iter()
        .enumerate()
        .map(|(i, o)| {
            if i > 0 {
                x += m.steps[shot.range.start as usize + i][0] * w;
            }
            x + o[0] * w
        })
        .collect()
}

#[test]
fn shake_is_cancelled() {
    let m = motion(jitter(120, 0.004), vec![]);
    let p = plan(&m, &Config::default(), FPS);
    assert_eq!(p.shots.len(), 1);
    let x = corrected_x(&m, &p.shots[0]);
    let worst = x
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0, f32::max);
    let raw = 0.004 * MEDIA.0 as f32;
    assert!(worst < raw * 0.2, "moves {worst} px a frame, raw {raw}");
    assert!(p.shots[0].zoom > 1.0);
}

#[test]
fn a_steady_pan_is_left_alone() {
    let m = motion(vec![[0.002, 0.0, 0.0]; 90], vec![]);
    let p = plan(&m, &Config::default(), FPS);
    let shot = &p.shots[0];
    let worst = shot
        .offsets
        .iter()
        .map(|o| o[0].abs() * MEDIA.0 as f32)
        .fold(0.0, f32::max);
    assert!(worst < 0.5, "offset {worst} px");
    assert!((shot.zoom - 1.0).abs() < 0.01, "zoom {}", shot.zoom);
}

#[test]
fn every_shot_is_smoothed_on_its_own() {
    let m = motion(jitter(90, 0.003), vec![30, 60]);
    let p = plan(&m, &Config::default(), FPS);
    let ranges: Vec<_> = p.shots.iter().map(|s| s.range.clone()).collect();
    assert_eq!(ranges, [0..30, 30..60, 60..90]);
}

#[test]
fn the_zoom_never_passes_the_limit() {
    let m = motion(jitter(60, 0.05), vec![]);
    let config = Config {
        strength: 1.0,
        max_zoom: 1.1,
    };
    let p = plan(&m, &config, FPS);
    let (w, h) = (MEDIA.0 as f32, MEDIA.1 as f32);
    for o in &p.shots[0].offsets {
        let needed = zoom_needed([o[0] * w, o[1] * h, o[2]], w, h);
        assert!(needed <= 1.1 + 1e-3, "needs {needed}");
    }
    assert!(p.shots[0].zoom <= 1.1 + 1e-6);
}

#[test]
fn zoom_needed_covers_shift_and_rotation() {
    assert_eq!(zoom_needed([0.0; 3], 100.0, 50.0), 1.0);
    assert!((zoom_needed([10.0, 0.0, 0.0], 100.0, 50.0) - 1.25).abs() < 1e-4);
    assert!(zoom_needed([0.0, 0.0, 0.05], 100.0, 50.0) > 1.0);
    assert!(zoom_needed([60.0, 0.0, 0.0], 100.0, 50.0).is_infinite());
}

#[test]
fn keyframes_are_tagged_and_removable() {
    let mut c = clip(60);
    c.effects
        .transform
        .track_mut(TransformParam::PositionX)
        .default = 40.0;
    let original = c.effects.transform.clone();
    let m = motion(jitter(60, 0.004), vec![]);
    let p = plan(&m, &Config::default(), FPS);
    let (transform, auto) = keyframes(&c, &p, MEDIA, (1920, 1080));
    assert!(!transform.track(TransformParam::PositionX).is_constant());
    assert!(!transform.track(TransformParam::Rotation).is_constant());
    assert!(auto.iter().all(|k| k.origin == Origin::Stabilize));
    let first = transform.track(TransformParam::PositionX).keyframes()[0].1;
    assert!((first - 40.0).abs() < 40.0, "kept around the base: {first}");

    c.effects.transform = transform;
    c.auto_keyframes = auto;
    assert_eq!(blocker(&c), None, "its own keys do not block a new run");
    let stripped = strip(&c.effects.transform, &c.auto_keyframes, Origin::Stabilize);
    for p in TransformParam::ALL {
        assert_eq!(stripped.track(p), original.track(p), "{p:?}");
    }
}

#[test]
fn hand_made_keyframes_and_rotation_block_it() {
    let mut c = clip(30);
    c.effects
        .transform
        .track_mut(TransformParam::PositionY)
        .upsert(5, 10.0, Interpolation::Linear);
    assert_eq!(blocker(&c), Some(Blocker::Keyframes));

    let mut c = clip(30);
    c.effects
        .transform
        .track_mut(TransformParam::Rotation)
        .default = 12.0;
    assert_eq!(blocker(&c), Some(Blocker::Transform));
}

#[test]
fn a_still_path_keeps_only_its_ends() {
    assert_eq!(kept(&[1.0; 50], 0.1), [0, 49]);
    assert_eq!(kept(&[0.0, 1.0, 0.0], 0.1), [0, 1, 2]);
}
