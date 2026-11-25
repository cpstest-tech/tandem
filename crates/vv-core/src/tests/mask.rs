use super::*;

#[test]
fn a_new_mask_is_centered_and_half_the_layer() {
    let mask = ClipMask::new(MaskShape::Rectangle, (1920, 1080)).value_at(0);
    assert_eq!(mask.center, [0.0, 0.0]);
    assert_eq!(mask.size, [960.0, 540.0]);
    assert_eq!(mask.opacity, 1.0);
    assert!(mask.polygon.is_empty());
}

#[test]
fn a_path_without_handles_flattens_to_its_corners() {
    let mask = ClipMask::new(MaskShape::Path, (400, 200)).value_at(0);
    assert_eq!(
        mask.polygon,
        [
            [-100.0, 50.0],
            [100.0, 50.0],
            [100.0, -50.0],
            [-100.0, -50.0]
        ]
    );
}

#[test]
fn the_path_follows_center_and_clockwise_rotation() {
    let mut mask = ClipMask::new(MaskShape::Path, (400, 200));
    mask.path = Keyframed::constant(MaskPath {
        points: vec![PathPoint::corner([0.0, 10.0])],
    });
    mask.track_mut(MaskParam::CenterX).default = 5.0;
    mask.track_mut(MaskParam::Rotation).default = 90.0;
    let [x, y] = mask.value_at(0).polygon[0];
    assert!((x - 15.0).abs() < 1e-4, "up turned clockwise points right");
    assert!(y.abs() < 1e-4);
}

#[test]
fn a_curved_segment_adds_points_on_the_bezier() {
    let path = MaskPath {
        points: vec![
            PathPoint {
                out_handle: [0.0, 10.0],
                ..PathPoint::corner([0.0, 0.0])
            },
            PathPoint {
                in_handle: [0.0, 10.0],
                ..PathPoint::corner([10.0, 0.0])
            },
        ],
    };
    let polygon = path.flatten();
    assert_eq!(polygon.len(), 1 + (CURVE_SEGMENTS - 1) + 1);
    let mid = polygon[CURVE_SEGMENTS / 2];
    assert!((mid[0] - 5.0).abs() < 1e-4 && (mid[1] - 7.5).abs() < 1e-4);
}

#[test]
fn paths_interpolate_point_by_point_only_when_they_match() {
    let a = MaskPath {
        points: vec![PathPoint::corner([0.0, 0.0])],
    };
    let b = MaskPath {
        points: vec![PathPoint::corner([10.0, 20.0])],
    };
    assert_eq!(MaskPath::lerp(&a, &b, 0.5).points[0].point, [5.0, 10.0]);
    let c = MaskPath {
        points: vec![PathPoint::corner([1.0, 1.0]); 2],
    };
    assert_eq!(
        MaskPath::lerp(&a, &c, 0.5),
        a,
        "different vertex counts hold"
    );
}

fn inside(polygon: &[[f32; 2]], [x, y]: [f32; 2]) -> bool {
    let mut odd = false;
    for i in 0..polygon.len() {
        let [x0, y0] = polygon[i];
        let [x1, y1] = polygon[(i + 1) % polygon.len()];
        if (y0 > y) != (y1 > y) && x < x0 + (y - y0) / (y1 - y0) * (x1 - x0) {
            odd = !odd;
        }
    }
    odd
}

#[test]
fn the_star_and_heart_presets_are_closed_paths_around_the_centre() {
    for preset in [MaskPreset::Star, MaskPreset::Heart] {
        let mask = ClipMask::preset(preset, (1920, 1080));
        assert!(mask.is_active());
        let value = mask.value_at(0);
        assert!(inside(&value.polygon, [0.0, 0.0]), "{preset:?}");
        assert!(!inside(&value.polygon, [900.0, 0.0]), "{preset:?}");
    }
    let heart = ClipMask::preset(MaskPreset::Heart, (1920, 1080)).value_at(0);
    assert!(
        !inside(&heart.polygon, [0.0, 0.3 * 1080.0 * 0.62]),
        "the notch between the lobes"
    );
}

#[test]
fn the_split_preset_covers_the_upper_half() {
    let split = ClipMask::preset(MaskPreset::Split, (1920, 1080)).value_at(0);
    assert_eq!(split.shape, MaskShape::Rectangle);
    let bottom = split.center[1] - split.size[1] / 2.0;
    assert!(bottom.abs() < 1e-3);
    assert!(split.size[0] > 1920.0 * 2.0);
}
