use super::*;

#[test]
fn media_and_timeline_points_are_inverse() {
    let t = vv_core::Transform {
        zoom: [1.5, 1.5],
        position: [100.0, -40.0],
        ..vv_core::Transform::default()
    };
    let media = (3840, 2160);
    let timeline = (1920, 1080);
    let p = [250.0, 80.0];
    let m = media_point(p, &t, media, timeline);
    let back = timeline_point(m, &t, media, timeline);
    assert!((back[0] - p[0]).abs() < 1e-3 && (back[1] - p[1]).abs() < 1e-3);
    // The centre of an untransformed picture is the centre of the media.
    let centre = media_point([0.0, 0.0], &vv_core::Transform::default(), media, timeline);
    assert_eq!(centre, [1920.0, 1080.0]);
}
