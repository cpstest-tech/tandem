use super::*;

/// A textured picture moved by (`dx`, `dy`) pixels.
fn picture(dx: f32, dy: f32) -> Luma {
    let (w, h) = (200, 120);
    let mut data = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 - dx, y as f32 - dy);
            let v = 128.0
                + 60.0 * (fx * 0.11).sin() * (fy * 0.07).cos()
                + 40.0 * ((fx * 0.031 + fy * 0.053).sin());
            data[y * w + x] = v.clamp(0.0, 255.0) as u8;
        }
    }
    Luma {
        width: w,
        height: h,
        data,
    }
}

#[test]
fn the_motion_between_two_frames_is_the_shift_of_the_picture() {
    let a = picture(0.0, 0.0);
    let b = picture(6.0, -4.0);
    let [dx, dy, angle] = frame_motion(&a, &b);
    assert!((dx * 200.0 - 6.0).abs() < 0.6, "{dx}");
    assert!((dy * 120.0 + 4.0).abs() < 0.6, "{dy}");
    assert!(angle.abs() < 0.5, "{angle}");
}

#[test]
fn a_turned_picture_measures_its_angle() {
    let (w, h) = (240usize, 160usize);
    let turned = |deg: f32| {
        let (sin, cos) = deg.to_radians().sin_cos();
        let mut data = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let (cx, cy) = (x as f32 - w as f32 / 2.0, y as f32 - h as f32 / 2.0);
                let (fx, fy) = (cx * cos + cy * sin, -cx * sin + cy * cos);
                let v = 128.0
                    + 60.0 * (fx * 0.11).sin() * (fy * 0.07).cos()
                    + 40.0 * ((fx * 0.031 + fy * 0.053).sin());
                data[y * w + x] = v.clamp(0.0, 255.0) as u8;
            }
        }
        Luma {
            width: w,
            height: h,
            data,
        }
    };
    // `turned(d)` is the pattern turned clockwise on screen, as `Transform::rotation`.
    let angle = frame_motion(&turned(0.0), &turned(2.0))[2];
    assert!((angle - 2.0).abs() < 0.6, "{angle}");
}

#[test]
fn a_still_picture_does_not_move() {
    let a = picture(0.0, 0.0);
    let [dx, dy, angle] = frame_motion(&a, &a);
    assert!(dx.abs() < 1e-4 && dy.abs() < 1e-4 && angle.abs() < 0.05);
}

#[test]
fn smoothing_takes_out_the_shake_and_keeps_the_pan() {
    // A steady pan to the right with a jitter on top.
    let motion: Vec<[f32; 3]> = (0..120)
        .map(|i| [0.002 + if i % 2 == 0 { 0.01 } else { -0.01 }, 0.0, 0.0])
        .collect();
    let stab = Stabilization::new(0, motion.clone(), 1.0, 30.0);
    let mut raw = 0.0;
    let mut steadied = Vec::new();
    for i in 0..=motion.len() {
        steadied.push(raw + stab.corrections[i][0]);
        if i < motion.len() {
            raw += motion[i][0];
        }
    }
    let jitter = |path: &[f32]| {
        path.windows(3)
            .map(|w| (w[0] - 2.0 * w[1] + w[2]).abs())
            .sum::<f32>()
    };
    let mut raw_path = vec![0.0];
    for m in &motion {
        raw_path.push(raw_path[raw_path.len() - 1] + m[0]);
    }
    assert!(jitter(&steadied[30..90]) < jitter(&raw_path[30..90]) * 0.2);
    assert!(steadied[90] > steadied[30], "the pan goes on");
    assert!(stab.zoom > 1.0 && stab.zoom <= 1.35);
}

#[test]
fn zero_strength_changes_nothing() {
    let motion = vec![[0.01, -0.02, 0.5]; 10];
    let stab = Stabilization::new(0, motion, 0.0, 30.0);
    assert!(
        stab.corrections
            .iter()
            .all(|c| c.iter().all(|v| v.abs() < 1e-6))
    );
    assert_eq!(stab.zoom, 1.0);
}
