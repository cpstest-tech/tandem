use super::*;
use vv_core::{Track, TrackKind};

fn app_with_clip(start: FrameIdx, len: FrameIdx) -> (VenturiApp, ClipId) {
    let mut app = VenturiApp::default();
    let tl = app.session.project.timelines.insert(vv_core::Timeline {
        name: "T".into(),
        fps: vv_core::Rational::new(25, 1),
        resolution: (1920, 1080),
        tracks: vec![Track::new(TrackKind::Video), Track::new(TrackKind::Audio)],
        markers: Vec::new(),
        master: Default::default(),
    });
    app.timeline_id = Some(tl);
    let id = app.session.project.alloc_clip_id();
    let clip = vv_core::Clip::from_source_range(
        id,
        ClipSource::SolidColor,
        0,
        len,
        start,
        vv_core::Rational::one(),
    );
    app.session.project.timelines[tl].tracks[0].insert_sorted(clip);
    (app, id)
}

fn start_of(app: &VenturiApp, id: ClipId) -> FrameIdx {
    app.session.project.timelines[app.timeline_id.unwrap()]
        .clip(0, id)
        .unwrap()
        .timeline_start
}

#[test]
fn nudging_moves_the_selection_and_stops_at_zero() {
    let (mut app, id) = app_with_clip(5, 20);
    app.timeline_state.set_single_selection(Some((0, id)));
    app.nudge_selected(1);
    assert_eq!(start_of(&app, id), 6);
    app.nudge_selected(-10);
    assert_eq!(start_of(&app, id), 0, "clamped at the start");
    app.undo();
    assert_eq!(start_of(&app, id), 6);
}

#[test]
fn append_puts_the_pool_selection_after_the_last_clip() {
    let (mut app, _) = app_with_clip(0, 50);
    let media = app.session.project.media_pool.insert(vv_core::MediaItem {
        path: "b.mp4".into(),
        meta: vv_core::MediaMeta {
            duration_frames: 40,
            fps: vv_core::Rational::new(25, 1),
            width: 1920,
            height: 1080,
            has_video: true,
            has_audio: false,
            sample_rate: 0,
            channels: 0,
            audio_streams: 0,
            file: Default::default(),
        },
        content_hash: 3,
        compound: None,
        folder: None,
    });
    app.media_pool_state.selected.insert(media);
    app.three_point_edit(EditMode::Append);
    let tl = &app.session.project.timelines[app.timeline_id.unwrap()];
    let spans: Vec<_> = tl.tracks[0]
        .clips
        .iter()
        .map(|c| (c.timeline_start, c.timeline_len))
        .collect();
    assert_eq!(spans, vec![(0, 50), (50, 40)]);
    assert_eq!(
        app.timeline_state.playhead, 90,
        "the playhead follows the edit"
    );
}
