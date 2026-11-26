use super::*;

#[test]
fn neutral_values_follow_the_params() {
    for param in GradeParam::ALL {
        assert_eq!(GradeValue::NEUTRAL.get(param), param.neutral(), "{param:?}");
        assert_eq!(GradeParam::ALL[param.index()], param);
    }
}

#[test]
fn black_and_white_only_drops_the_global_saturation() {
    let value = GradePreset::BlackAndWhite.value();
    for param in GradeParam::ALL {
        let expected = if param == GradeParam::Saturation {
            0.0
        } else {
            param.neutral()
        };
        assert_eq!(value.get(param), expected, "{param:?}");
    }
}

#[test]
fn tracks_survive_save_and_load() {
    let mut tracks = GradeTracks::default();
    tracks
        .track_mut(GradeParam::MidtonesX)
        .upsert(10, 0.5, crate::Interpolation::Linear);
    let text = ron::to_string(&tracks).unwrap();
    assert_eq!(ron::from_str::<GradeTracks>(&text).unwrap(), tracks);
}

#[test]
fn tracks_are_saved_by_name() {
    let mut tracks = GradeTracks::default();
    tracks.track_mut(GradeParam::HighRange).default = 0.75;
    let text = ron::to_string(&tracks).unwrap();
    assert!(
        text.contains("HighRange:(keyframes:[],default:0.75)"),
        "{text}"
    );
}

#[test]
fn params_missing_from_a_file_load_neutral() {
    let tracks: GradeTracks =
        ron::from_str("(tracks: {Saturation: (keyframes: [], default: 0.5)})").unwrap();
    let value = tracks.value_at(0);
    assert_eq!(value.get(GradeParam::Saturation), 0.5);
    for param in GradeParam::ALL
        .into_iter()
        .filter(|p| *p != GradeParam::Saturation)
    {
        assert_eq!(value.get(param), param.neutral(), "{param:?}");
    }
}

#[test]
fn black_and_white_keeps_the_rest_of_the_grade() {
    let mut tracks = GradeTracks::default();
    tracks.track_mut(GradeParam::MidtonesX).default = 0.3;
    tracks.apply_preset(GradePreset::BlackAndWhite);
    let value = tracks.value_at(0);
    assert_eq!(value.get(GradeParam::MidtonesX), 0.3);
    assert_eq!(value.get(GradeParam::Saturation), 0.0);
}

#[test]
fn the_neutral_preset_resets_every_param() {
    let mut tracks = GradeTracks::default();
    tracks
        .track_mut(GradeParam::MidtonesX)
        .upsert(10, 0.5, crate::Interpolation::Linear);
    tracks.track_mut(GradeParam::Saturation).default = 0.0;
    tracks.apply_preset(GradePreset::Neutral);
    assert_eq!(tracks, GradeTracks::default());
}

#[test]
fn a_keyframed_param_interpolates() {
    let mut tracks = GradeTracks::default();
    let track = tracks.track_mut(GradeParam::OffsetLuma);
    track.upsert(0, 0.0, crate::Interpolation::Linear);
    track.upsert(10, 1.0, crate::Interpolation::Linear);
    assert_eq!(tracks.value_at(5).get(GradeParam::OffsetLuma), 0.5);
    assert_eq!(tracks.value_at(5).get(GradeParam::Saturation), 1.0);
}

#[test]
fn a_chroma_shift_leaves_the_luma_unchanged() {
    for (x, y) in [(1.0, 0.0), (0.0, 1.0), (-0.6, 0.8)] {
        let shift = chroma_shift(x, y);
        let luma: f32 = shift.iter().zip(LUMA_WEIGHTS).map(|(c, w)| c * w).sum();
        assert!(luma.abs() < 1e-4, "({x}, {y}): {luma}");
        assert!(shift.iter().any(|c| c.abs() > 0.1));
    }
}

#[test]
fn cb_cr_reads_back_a_chroma_shift() {
    for (x, y) in [(1.0, 0.0), (0.0, 1.0), (-0.6, 0.8)] {
        let shifted = chroma_shift(x, y).map(|c| 0.5 + c);
        let [cb, cr] = cb_cr(shifted);
        let [want_cb, want_cr] = wheel_chroma(x, y);
        assert!((cb - want_cb).abs() < 1e-5 && (cr - want_cr).abs() < 1e-5);
    }
}

#[test]
fn the_chroma_shift_grows_with_the_square_of_the_distance() {
    let edge = chroma_shift(0.0, 1.0)[0];
    let half = chroma_shift(0.0, 0.5)[0];
    assert!((half - edge / 4.0).abs() < 1e-6, "{half} vs {edge}");
}

#[test]
fn wheel_for_chroma_inverts_wheel_chroma() {
    for (x, y) in [(0.3, -0.2), (-0.7, 0.1), (0.0, 0.9)] {
        let (bx, by) = wheel_for_chroma(wheel_chroma(x, y));
        assert!(
            (bx - x).abs() < 1e-5 && (by - y).abs() < 1e-5,
            "({x}, {y}) -> ({bx}, {by})"
        );
    }
    let (x, y) = wheel_for_chroma([10.0, 0.0]);
    assert_eq!((x, y), (1.0, 0.0), "out of reach: at the edge");
}

#[test]
fn the_range_weights_sum_to_one() {
    for luma in [0.0, 0.2, 1.0 / 3.0, 0.5, 0.7, 1.0] {
        let weights = range_weights(luma, 1.0 / 3.0, 2.0 / 3.0);
        assert!(
            (weights.iter().sum::<f32>() - 1.0).abs() < 1e-6,
            "{luma}: {weights:?}"
        );
    }
    assert!(range_weights(0.05, 1.0 / 3.0, 2.0 / 3.0)[0] > 0.99);
    assert!(range_weights(0.95, 1.0 / 3.0, 2.0 / 3.0)[2] > 0.99);
}

/// A blue cast in the midtones only: the midtones wheel moves against it,
/// the shadows and highlights ones stay.
#[test]
fn auto_balance_cancels_a_cast_in_its_range() {
    let cast = [0.47, 0.5, 0.6];
    let pixels = (0..100)
        .map(|_| cast)
        .chain((0..100).map(|_| [0.05, 0.05, 0.05]));
    let balanced = auto_balance(&GradeValue::NEUTRAL, pixels);
    let luma: f32 = cast.iter().zip(LUMA_WEIGHTS).map(|(c, w)| c * w).sum();
    let wanted = [-(cast[2] - luma) / 1.8556, -(cast[0] - luma) / 1.5748];
    let [cb, cr] = wheel_chroma(
        balanced.get(GradeParam::MidtonesX),
        balanced.get(GradeParam::MidtonesY),
    );
    let weight = range_weights(luma, 1.0 / 3.0, 2.0 / 3.0)[1];
    assert!(weight > 0.99);
    assert!(
        (cb - wanted[0]).abs() < 1e-3 && (cr - wanted[1]).abs() < 1e-3,
        "{cb} {cr} vs {wanted:?}"
    );
    assert_eq!(balanced.get(GradeParam::ShadowsX), 0.0, "neutral shadows");
    assert_eq!(balanced.get(GradeParam::HighlightsX), 0.0, "no highlights");
}

#[test]
fn auto_balance_ignores_black_and_white_pixels_and_leaves_the_rest_alone() {
    let pixels = (0..100)
        .map(|_| [1.0, 1.0, 0.99])
        .chain((0..100).map(|_| [0.0, 0.0, 0.03]));
    let mut grade = GradeValue::NEUTRAL;
    grade.set(GradeParam::OffsetX, 0.3);
    grade.set(GradeParam::MidtonesLuma, 0.2);
    assert_eq!(auto_balance(&grade, pixels), grade);
}

/// Each range's mean color after `grade`.
fn range_means(pixels: &[[f32; 3]], grade: &GradeValue) -> [[f32; 3]; 3] {
    let mut sums = [[0.0f64; 4]; 3];
    for pixel in pixels {
        let graded = apply_grade(*pixel, grade);
        for (sum, w) in sums
            .iter_mut()
            .zip(range_weights(luma(graded), 1.0 / 3.0, 2.0 / 3.0))
        {
            for c in 0..3 {
                sum[c] += (w * graded[c]) as f64;
            }
            sum[3] += w as f64;
        }
    }
    sums.map(|s| [0, 1, 2].map(|c| (s[c] / s[3]) as f32))
}

/// A blue cast in the shadows, a warm one in the highlights: the ranges
/// overlap, so balancing each on its own would overshoot its neighbours.
#[test]
fn auto_balance_neutralises_every_range_in_one_go() {
    let pixels: Vec<[f32; 3]> = (8..248)
        .map(|level| {
            let v = level as f32 / 255.0;
            let warmth = (v - 0.5) * 0.12;
            [v + warmth, v, v - warmth]
        })
        .collect();
    let balanced = auto_balance(&GradeValue::NEUTRAL, pixels.iter().copied());
    for (range, mean) in range_means(&pixels, &balanced).iter().enumerate() {
        let spread = mean.iter().cloned().fold(f32::MIN, f32::max)
            - mean.iter().cloned().fold(f32::MAX, f32::min);
        assert!(spread < 0.003, "range {range}: {mean:?}");
    }
}

#[test]
fn solve_finds_the_exact_moves() {
    let matrix = vec![
        vec![2.0, 1.0, 0.0],
        vec![1.0, 3.0, 1.0],
        vec![0.0, 1.0, 2.0],
    ];
    // Solutions 1 and -2 everywhere: the two columns are solved at once.
    let x = solve(matrix, vec![[3.0, -6.0], [5.0, -10.0], [3.0, -6.0]]).unwrap();
    for [a, b] in x {
        assert!((a - 1.0).abs() < 1e-9 && (b + 2.0).abs() < 1e-9);
    }
    let singular = vec![vec![1.0, 2.0], vec![2.0, 4.0]];
    assert!(solve(singular, vec![[1.0, 0.0], [2.0, 0.0]]).is_none());
}

/// A warm grey wall behind a large orange shirt: the wall turns grey, and
/// the shirt does not drag the picture towards blue.
#[test]
fn auto_balance_measures_the_neutral_surfaces_not_a_colored_object() {
    let wall = |v: f32| [v + 0.04, v, v - 0.02];
    let shirt = [0.85, 0.45, 0.12];
    let pixels: Vec<[f32; 3]> = (0..600)
        .map(|i| wall(0.3 + (i % 50) as f32 * 0.008))
        .chain(std::iter::repeat_n(shirt, 400))
        .collect();
    let balanced = auto_balance(&GradeValue::NEUTRAL, pixels.iter().copied());
    for v in [0.32, 0.5, 0.65] {
        let grey = apply_grade(wall(v), &balanced);
        let spread = grey.iter().cloned().fold(f32::MIN, f32::max)
            - grey.iter().cloned().fold(f32::MAX, f32::min);
        assert!(spread < 0.01, "wall at {v}: {grey:?}");
    }
    let orange = apply_grade(shirt, &balanced);
    assert!(
        orange[0] > orange[1] && orange[1] > orange[2],
        "still orange: {orange:?}"
    );
}

fn adjusted(params: &[(GradeParam, f32)], rgb: [f32; 3]) -> [f32; 3] {
    let mut grade = GradeValue::NEUTRAL;
    for (p, v) in params {
        grade.set(*p, *v);
    }
    apply_grade(rgb, &grade)
}

#[test]
fn neutral_adjust_controls_change_nothing() {
    assert!(!GradeValue::NEUTRAL.adjusts());
    let rgb = [0.3, 0.6, 0.2];
    let out = apply_adjust(rgb, &GradeValue::NEUTRAL, [0.1, 0.9]);
    assert!((0..3).all(|i| (out[i] - rgb[i]).abs() < 1e-5), "{out:?}");
}

#[test]
fn each_adjust_control_moves_the_right_way() {
    let grey = [0.5, 0.5, 0.5];
    let warm = adjusted(&[(GradeParam::Temperature, 1.0)], grey);
    assert!(warm[0] > warm[2]);
    assert!(adjusted(&[(GradeParam::Brightness, 0.5)], grey)[1] > 0.5);
    let contrast = adjusted(&[(GradeParam::Contrast, 0.5)], [0.8, 0.8, 0.8]);
    assert!(contrast[0] > 0.8);
    assert!(adjusted(&[(GradeParam::Shadows, 1.0)], [0.1, 0.1, 0.1])[0] > 0.2);
    let red = [0.8, 0.2, 0.2];
    let desaturated = adjusted(&[(GradeParam::HslRedSat, -1.0)], red);
    assert!(
        (desaturated[0] - desaturated[1]).abs() < 0.05,
        "{desaturated:?}"
    );
    let untouched = adjusted(&[(GradeParam::HslRedSat, -1.0)], [0.2, 0.2, 0.8]);
    assert!((untouched[2] - 0.8).abs() < 0.01, "blue is another band");
    let lifted = adjusted(&[(GradeParam::CurveMaster2, 0.75)], grey);
    assert!((lifted[0] - 0.75).abs() < 1e-4);
}

#[test]
fn the_vignette_darkens_the_corners_only() {
    let mut grade = GradeValue::NEUTRAL;
    grade.set(GradeParam::Vignette, 1.0);
    let centre = apply_grade_at([0.6; 3], &grade, [0.5, 0.5]);
    let corner = apply_grade_at([0.6; 3], &grade, [0.0, 0.0]);
    assert!((centre[0] - 0.6).abs() < 1e-5);
    assert!(corner[0] < 0.1);
}

#[test]
fn a_curve_through_its_neutral_knots_is_the_identity() {
    let knots = [0.0, 0.25, 0.5, 0.75, 1.0];
    for x in [0.0, 0.1, 0.33, 0.5, 0.9, 1.0] {
        assert!((curve_at(knots, x) - x).abs() < 1e-5);
    }
}

#[test]
fn auto_enhance_brightens_a_dull_dark_picture() {
    let pixels: Vec<[f32; 3]> = (0..1000)
        .map(|i| {
            let v = 0.1 + 0.25 * (i as f32 / 1000.0);
            [v, v * 0.95, v * 0.9]
        })
        .collect();
    let enhanced = auto_enhance(&GradeValue::NEUTRAL, pixels.iter().copied());
    assert!(enhanced.get(GradeParam::Brightness) > 0.0);
    assert!(enhanced.get(GradeParam::Contrast) > 0.0);
    assert!(enhanced.get(GradeParam::Saturation) > 1.0);
    assert!(enhanced.get(GradeParam::Sharpen) > 0.0);
    let before = luma(apply_grade(pixels[500], &GradeValue::NEUTRAL));
    let after = luma(apply_grade(pixels[500], &enhanced));
    assert!(after > before);
}

#[test]
fn a_dark_cool_shot_is_matched_to_a_warm_bright_one() {
    let shot = |gain: f32, tint: [f32; 3]| -> Vec<[f32; 3]> {
        (0..400)
            .map(|i| {
                let v = 0.1 + 0.8 * (i as f32 / 400.0);
                [v * gain * tint[0], v * gain * tint[1], v * gain * tint[2]]
                    .map(|c| c.clamp(0.0, 1.0))
            })
            .collect()
    };
    let reference = shot(1.0, [1.0, 0.92, 0.8]);
    let target = shot(0.6, [0.85, 0.95, 1.0]);
    let matched = match_color(&GradeValue::NEUTRAL, &target, &reference);
    let graded: Vec<[f32; 3]> = target.iter().map(|p| apply_grade(*p, &matched)).collect();
    let (want, got, before) = (
        color_stats(&reference),
        color_stats(&graded),
        color_stats(&target),
    );
    for i in 0..4 {
        assert!(
            (got[i] - want[i]).abs() < (before[i] - want[i]).abs() * 0.5 + 0.01,
            "stat {i}: {got:?} vs {want:?}"
        );
    }
}
