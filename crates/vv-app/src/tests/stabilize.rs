use super::*;
use vv_core::TransformParam;

fn app_with_clip() -> (VenturiApp, ClipKey) {
    let mut app = VenturiApp::default();
    let timeline = app.ensure_timeline();
    let id = app.session.project.alloc_clip_id();
    app.session.project.timelines[timeline].tracks[0]
        .clips
        .push(vv_core::Clip::from_source_range(
            id,
            ClipSource::SolidColor,
            0,
            60,
            0,
            vv_core::Rational::one(),
        ));
    (app, (timeline, 0, id))
}

fn shaky() -> Motion {
    Motion {
        size: (1920, 1080),
        range: (0, 60),
        steps: (0..60)
            .map(|i| {
                let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
                [sign * 0.004, sign * 0.002, 0.0]
            })
            .collect(),
        cuts: vec![],
    }
}

fn position_x(app: &VenturiApp, key: ClipKey) -> vv_core::Keyframed<f32> {
    app.session.project.timelines[key.0]
        .clip(key.1, key.2)
        .unwrap()
        .effects
        .transform
        .track(TransformParam::PositionX)
        .clone()
}

#[test]
fn a_new_setting_replaces_the_last_stabilization() {
    let (mut app, key) = app_with_clip();
    let original = position_x(&app, key);
    app.apply_stabilize(key, (1920, 1080), 30.0, &shaky());
    let first = position_x(&app, key);
    assert_ne!(first, original);
    app.stabilize.config.strength = 1.0;
    app.apply_stabilize(key, (1920, 1080), 30.0, &shaky());
    assert_ne!(position_x(&app, key), first);
    app.undo();
    assert_eq!(position_x(&app, key), original, "one step, not two");
}

#[test]
fn a_motion_of_another_range_is_not_applied() {
    let (mut app, key) = app_with_clip();
    let original = position_x(&app, key);
    let motion = Motion {
        range: (0, 30),
        steps: shaky().steps[..30].to_vec(),
        ..shaky()
    };
    app.apply_stabilize(key, (1920, 1080), 30.0, &motion);
    assert_eq!(position_x(&app, key), original);
    assert!(app.stabilize.applied.is_none());
}
