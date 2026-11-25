use super::*;
use crate::History;
use crate::model::{Clip, ClipSource, MediaItem, MediaMeta, Timeline, Track, TrackKind};

fn project_with_clip(source: (FrameIdx, FrameIdx)) -> (Project, TimelineId, ClipId, ClipId) {
    let mut project = Project::default();
    let timeline = project.timelines.insert(Timeline {
        name: "Timeline 1".into(),
        fps: Rational::new(25, 1),
        resolution: (1920, 1080),
        tracks: vec![Track::new(TrackKind::Video), Track::new(TrackKind::Video)],
        markers: Vec::new(),
        master: Default::default(),
    });
    let media = project.media_pool.insert(MediaItem {
        path: "a.mp4".into(),
        meta: MediaMeta {
            duration_frames: 400,
            fps: Rational::new(25, 1),
            width: 1920,
            height: 1080,
            has_video: true,
            has_audio: false,
            sample_rate: 0,
            channels: 0,
            audio_streams: 0,
            file: Default::default(),
        },
        content_hash: 7,
        compound: None,
        folder: None,
    });
    let id = project.alloc_clip_id();
    let clip = Clip::from_source_range(
        id,
        ClipSource::Media(media),
        source.0,
        source.1,
        0,
        Rational::one(),
    );
    project.timelines[timeline].tracks[0].insert_sorted(clip);
    let after = project.alloc_clip_id();
    let next = Clip::from_source_range(
        after,
        ClipSource::Media(media),
        0,
        10,
        source.1 - source.0,
        Rational::one(),
    );
    project.timelines[timeline].tracks[1].insert_sorted(next);
    (project, timeline, id, after)
}

#[test]
fn a_flat_curve_is_the_identity() {
    let flat = normalized(&[[0.0, 1.0], [1.0, 1.0]]);
    assert!((mean(&flat) - 1.0).abs() < 1e-9);
    for x in [0.0, 0.25, 0.5, 1.0] {
        assert!((progress(&flat, x) - x).abs() < 1e-9);
    }
}

#[test]
fn a_ramp_up_shows_less_source_at_the_start() {
    let ramp = CurvePreset::RampUp.points();
    assert!((mean(&ramp) - 1.75).abs() < 1e-6);
    assert!(progress(&ramp, 0.5) < 0.5);
    assert!((progress(&ramp, 1.0) - 1.0).abs() < 1e-9);
}

#[test]
fn normalizing_adds_the_ends_and_clamps_the_speeds() {
    let points = normalized(&[[0.5, 50.0], [0.2, 0.0]]);
    assert_eq!(
        points,
        vec![
            [0.0, MIN_SPEED],
            [0.2, MIN_SPEED],
            [0.5, MAX_SPEED],
            [1.0, MAX_SPEED]
        ]
    );
}

#[test]
fn a_curve_sets_the_average_speed_ripples_and_undoes() {
    let (mut project, tl, clip, after) = project_with_clip((0, 175));
    let mut history = History::default();
    history.do_command(
        &mut project,
        Box::new(SetClipSpeedCurve::new(
            tl,
            vec![(0, clip)],
            Some(CurvePreset::RampUp.points()),
        )),
    );
    let c = project.timelines[tl].clip(0, clip).unwrap();
    assert_eq!(c.timeline_len, 100, "175 source frames at 1.75x");
    assert!(c.active_curve().is_some());
    assert!(c.is_silent());
    assert_eq!(c.picture_frame_at(0), 0);
    assert!(
        (172..=174).contains(&c.picture_frame_at(99)),
        "near the end at 3x"
    );
    assert!(c.picture_frame_at(50) < 87, "slow start");
    let next = project.timelines[tl].clip(1, after).unwrap();
    assert_eq!(next.timeline_start, 100, "the rest moves back");

    history.do_command(
        &mut project,
        Box::new(SetClipSpeedCurve::new(tl, vec![(0, clip)], None)),
    );
    let c = project.timelines[tl].clip(0, clip).unwrap();
    assert_eq!(c.timeline_len, 175, "back to the base speed");
    assert!(c.speed_curve.is_none());

    history.undo(&mut project);
    history.undo(&mut project);
    let c = project.timelines[tl].clip(0, clip).unwrap();
    assert_eq!(c.timeline_len, 175);
    assert!(c.speed_curve.is_none());
}

#[test]
fn a_trim_turns_the_curve_off() {
    let (mut project, tl, clip, _) = project_with_clip((0, 175));
    SetClipSpeedCurve::new(tl, vec![(0, clip)], Some(CurvePreset::Pulse.points()))
        .apply(&mut project);
    let c = project.timelines[tl].tracks[0].clip_mut(clip).unwrap();
    c.timeline_len -= 10;
    assert!(c.active_curve().is_none());
    assert!(!c.is_silent());
}
