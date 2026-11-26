use super::*;
use crate::auto_zoom;
use crate::{ClipId, ClipSource, Rational};

const FPS: f64 = 30.0;
const MEDIA: (u32, u32) = (1920, 1080);
const VERTICAL: (u32, u32) = (1080, 1920);

fn face(x: f32, y: f32, h: f32) -> Face {
    Face {
        center: [x, y],
        size: [h * 9.0 / 16.0, h],
        score: 0.9,
        mouth: 0.0,
    }
}

fn analysis(len: FrameIdx, cuts: Vec<FrameIdx>, faces: impl Fn(FrameIdx) -> Vec<Face>) -> Analysis {
    Analysis {
        size: MEDIA,
        range: (0, len),
        step: 5,
        samples: (0..len)
            .step_by(5)
            .map(|frame| Sample {
                frame,
                faces: faces(frame),
            })
            .collect(),
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

fn silent(_: FrameIdx) -> bool {
    false
}

fn talking(_: FrameIdx) -> bool {
    true
}

fn plan_of(a: &Analysis, config: &Config) -> Plan {
    plan(a, config, &[], &silent, FPS, VERTICAL)
}

/// 16:9 into 9:16 at zoom 1 is letterboxed; filling the height takes 16/9 · 16/9.
const FILL: f32 = (16.0 / 9.0) * (16.0 / 9.0);

#[test]
fn a_centred_face_fills_the_vertical_frame_without_moving() {
    let a = analysis(90, vec![], |_| vec![face(0.5, 0.4, 0.25)]);
    let p = plan_of(&a, &Config::default());
    assert_eq!(p.scenes.len(), 1);
    let scene = &p.scenes[0];
    assert!((scene.zoom - FILL).abs() < 0.01, "zoom {}", scene.zoom);
    let first = scene.points[0].1;
    assert!(scene.points.iter().all(|(_, pos)| *pos == first));
    assert_eq!(scene.points.len(), 2, "a still path keeps only its ends");
    assert!(p.missing.is_empty());
}

#[test]
fn jitter_inside_the_dead_zone_does_not_move_the_frame() {
    let a = analysis(150, vec![], |f| {
        let wobble = if (f / 5) % 2 == 0 { 0.004 } else { -0.004 };
        vec![face(0.5 + wobble, 0.4, 0.25)]
    });
    let p = plan_of(&a, &Config::default());
    let xs: Vec<f32> = p.scenes[0].points.iter().map(|(_, pos)| pos[0]).collect();
    let spread =
        xs.iter().copied().fold(f32::MIN, f32::max) - xs.iter().copied().fold(f32::MAX, f32::min);
    assert!(spread < 1.0, "moved by {spread} px");
}

#[test]
fn a_face_walking_right_is_followed() {
    let a = analysis(150, vec![], |f| {
        vec![face(0.35 + f as f32 * 0.002, 0.4, 0.25)]
    });
    let p = plan_of(&a, &Config::default());
    let xs: Vec<f32> = p.scenes[0].points.iter().map(|(_, pos)| pos[0]).collect();
    assert!(xs.windows(2).all(|w| w[1] <= w[0] + 1e-3), "{xs:?}");
    assert!(xs.last().unwrap() < &(xs[0] - 100.0));
}

#[test]
fn the_frame_never_shows_the_letterbox() {
    let a = analysis(60, vec![], |_| vec![face(0.98, 0.05, 0.1)]);
    let p = plan_of(&a, &Config::default());
    let scene = &p.scenes[0];
    let fit = 1080.0 / 1920.0;
    let max_x = (scene.zoom * 1920.0 * fit - 1080.0) / 2.0;
    let max_y = (scene.zoom * 1080.0 * fit - 1920.0) / 2.0;
    for (_, pos) in &scene.points {
        assert!(
            pos[0].abs() <= max_x + 0.01 && pos[1].abs() <= max_y + 0.01,
            "{pos:?}"
        );
    }
}

#[test]
fn a_cut_restarts_the_tracking_instead_of_gliding() {
    let a = analysis(120, vec![60], |f| {
        vec![face(if f < 60 { 0.3 } else { 0.7 }, 0.4, 0.25)]
    });
    let p = plan_of(&a, &Config::default());
    assert_eq!(p.scenes.len(), 2);
    let end_of_first = p.scenes[0].points.last().unwrap();
    let start_of_second = p.scenes[1].points[0];
    assert_eq!(end_of_first.0, 59);
    assert_eq!(start_of_second.0, 60);
    assert!(end_of_first.1[0] > 0.0 && start_of_second.1[0] < 0.0);

    let (t, _) = keyframes(&clip(120), &p);
    let x = t.track(TransformParam::PositionX);
    assert_eq!(x.keyframe_at(59).unwrap().1, Interpolation::Hold);
    assert_eq!(x.value_at(59), end_of_first.1[0]);
    assert_eq!(x.value_at(60), start_of_second.1[0]);
}

#[test]
fn without_faces_the_frame_stays_centred_and_says_where() {
    let a = analysis(90, vec![], |f| {
        if (30..60).contains(&f) {
            vec![]
        } else {
            vec![face(0.3, 0.4, 0.25)]
        }
    });
    let p = plan_of(&a, &Config::default());
    assert_eq!(p.missing, vec![30..60]);
    let at_45 = p.scenes[0]
        .points
        .iter()
        .rfind(|(f, _)| *f <= 45)
        .unwrap()
        .1;
    assert!(at_45[0] > 0.0, "keeps the last point of interest");

    let empty = analysis(60, vec![], |_| vec![]);
    let p = plan_of(&empty, &Config::default());
    assert_eq!(p.missing, vec![0..60]);
    assert!(p.scenes[0].points.iter().all(|(_, pos)| pos[0] == 0.0));
}

#[test]
fn the_largest_face_is_the_first_subject() {
    let a = analysis(60, vec![], |_| {
        vec![face(0.25, 0.4, 0.1), face(0.75, 0.4, 0.3)]
    });
    let p = plan_of(&a, &Config::default());
    assert!(p.scenes[0].points[0].1[0] < 0.0, "framed on the right face");
}

#[test]
fn the_speaking_face_takes_over_after_a_while() {
    let a = analysis(150, vec![], |f| {
        let mut quiet = face(0.75, 0.4, 0.3);
        let mut speaker = face(0.25, 0.4, 0.2);
        if f >= 30 {
            speaker.mouth = 0.05;
        }
        quiet.mouth = 0.0;
        vec![speaker, quiet]
    });
    let config = Config {
        follow: 1.0,
        ..Config::default()
    };
    let p = plan(&a, &config, &[], &talking, FPS, VERTICAL);
    let last = p.scenes[0].points.last().unwrap().1;
    assert!(last[0] > 0.0, "ends on the left speaker: {last:?}");

    let muted = plan(&a, &config, &[], &silent, FPS, VERTICAL);
    assert!(muted.scenes[0].points.last().unwrap().1[0] < 0.0);
}

#[test]
fn a_pin_chooses_the_face_until_the_shot_ends() {
    let a = analysis(120, vec![60], |_| {
        vec![face(0.25, 0.4, 0.2), face(0.75, 0.4, 0.3)]
    });
    let pins = [Pin {
        frame: 10,
        at: [0.24, 0.42],
    }];
    let p = plan(&a, &Config::default(), &pins, &silent, FPS, VERTICAL);
    assert!(p.scenes[0].points.last().unwrap().1[0] > 0.0);
    assert!(
        p.scenes[1].points[0].1[0] < 0.0,
        "the next shot starts over"
    );
}

#[test]
fn keyframes_are_tagged_and_removed_without_touching_the_rest() {
    let a = analysis(90, vec![], |f| {
        vec![face(0.3 + f as f32 * 0.003, 0.4, 0.25)]
    });
    let p = plan_of(&a, &Config::default());
    let mut c = clip(90);
    c.effects
        .transform
        .track_mut(TransformParam::Opacity)
        .upsert(10, 50.0, Interpolation::Linear);
    let (t, auto) = keyframes(&c, &p);
    assert!(auto.iter().all(|k| k.origin == Origin::Reframe));
    c.effects.transform = t;
    c.auto_keyframes = auto;
    assert!(!has_other_keyframes(&c));

    let stripped = auto_zoom::strip(&c.effects.transform, &c.auto_keyframes, Origin::Reframe);
    assert!(stripped.track(TransformParam::PositionX).is_constant());
    assert!(!stripped.track(TransformParam::Opacity).is_constant());
}

#[test]
fn auto_zoom_still_works_on_a_reframed_clip() {
    let a = analysis(300, vec![], |_| vec![face(0.3, 0.4, 0.25)]);
    let p = plan_of(&a, &Config::default());
    let mut c = clip(300);
    let (t, auto) = keyframes(&c, &p);
    c.effects.transform = t;
    c.auto_keyframes = auto;
    let zoom = auto_zoom::Zoom {
        at: 3.0,
        hold_end: 4.0,
        scale: 1.2,
        shift: [0.0, 0.0],
        word: "parola".into(),
    };
    let (t, auto) = auto_zoom::keyframes(&c, &[zoom], &auto_zoom::Config::default(), FPS, VERTICAL);
    let peak = t.track(TransformParam::ZoomX).value_at(100);
    assert!((peak - p.scenes[0].zoom * 1.2).abs() < 0.01, "{peak}");
    assert!(auto.iter().any(|k| k.origin == Origin::Reframe));
    assert!(auto.iter().any(|k| k.origin == Origin::Zoom));
}

#[test]
fn scenes_come_from_the_cuts_inside_the_range() {
    let a = Analysis {
        size: MEDIA,
        range: (10, 100),
        step: 5,
        samples: vec![],
        cuts: vec![5, 40, 100],
    };
    assert_eq!(a.scenes(), vec![10..40, 40..100]);
}

#[test]
fn keyframe_tags_of_older_projects_read_as_auto_zoom() {
    let old: AutoKeyframe =
        serde_json::from_str(r#"{"param":"ZoomX","frame":3,"value":1.2}"#).unwrap();
    assert_eq!(old.origin, Origin::Zoom);
    let json = serde_json::to_string(&old).unwrap();
    assert!(
        !json.contains("origin"),
        "auto zoom tags are written as before: {json}"
    );
}

#[test]
fn the_blur_masks_follow_each_face_and_hide_when_it_is_gone() {
    use crate::mask::MaskParam;
    let face = |x: f32| Face {
        center: [x, 0.5],
        size: [0.1, 0.2],
        score: 0.9,
        mouth: 0.0,
    };
    let analysis = Analysis {
        size: (1920, 1080),
        range: (0, 30),
        step: 10,
        samples: vec![
            Sample {
                frame: 0,
                faces: vec![face(0.25), face(0.75)],
            },
            Sample {
                frame: 10,
                faces: vec![face(0.27), face(0.73)],
            },
            Sample {
                frame: 20,
                faces: vec![face(0.29)],
            },
        ],
        cuts: Vec::new(),
    };
    let masks = face_blur_masks(&analysis, (1920, 1080));
    assert_eq!(masks.len(), 2);
    let left = masks
        .iter()
        .find(|m| m.track(MaskParam::CenterX).value_at(0) < 0.0)
        .unwrap();
    assert!((left.track(MaskParam::CenterX).value_at(20) - (0.29 - 0.5) * 1920.0).abs() < 1e-3);
    let right = masks
        .iter()
        .find(|m| m.track(MaskParam::CenterX).value_at(0) > 0.0)
        .unwrap();
    assert_eq!(
        right.track(MaskParam::Opacity).value_at(20),
        0.0,
        "gone at frame 20"
    );
    assert_eq!(right.track(MaskParam::Opacity).value_at(10), 100.0);
}
