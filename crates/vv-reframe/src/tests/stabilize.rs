use super::*;

const W: usize = 320;
const H: usize = 180;
const CELL: f32 = 5.0;

/// Smooth value noise, defined at any real position.
fn texture(x: f32, y: f32) -> f32 {
    let lattice = |i: i64, j: i64| -> f32 {
        let mut h = (i.wrapping_mul(73_856_093) ^ j.wrapping_mul(19_349_663)) as u64;
        h ^= h >> 13;
        h = h.wrapping_mul(0x5bd1_e995);
        h ^= h >> 15;
        (h % 256) as f32
    };
    let (gx, gy) = (x / CELL, y / CELL);
    let (i, j) = (gx.floor() as i64, gy.floor() as i64);
    let (fx, fy) = (gx - gx.floor(), gy - gy.floor());
    let top = lattice(i, j) * (1.0 - fx) + lattice(i + 1, j) * fx;
    let bottom = lattice(i, j + 1) * (1.0 - fx) + lattice(i + 1, j + 1) * fx;
    top * (1.0 - fy) + bottom * fy
}

fn image(sample: impl Fn(f32, f32) -> f32) -> Pyramid {
    let mut px = Vec::with_capacity(W * H);
    for y in 0..H {
        for x in 0..W {
            px.push(sample(x as f32, y as f32).clamp(0.0, 255.0) as u8);
        }
    }
    Pyramid::new(Gray {
        width: W,
        height: H,
        px,
    })
}

fn moved(dx: f32, dy: f32, angle: f32) -> Pyramid {
    let (cx, cy) = (W as f32 / 2.0, H as f32 / 2.0);
    let (sin, cos) = (-angle).sin_cos();
    image(|x, y| {
        let (px, py) = (x - cx, y - cy);
        let (sx, sy) = (px - dx, py - dy);
        texture(sx * cos - sy * sin + cx, sx * sin + sy * cos + cy)
    })
}

#[test]
fn a_shift_is_measured() {
    let before = image(texture);
    let after = moved(6.0, -3.0, 0.0);
    let [dx, dy, angle] = estimate(&before, &after).expect("enough texture");
    assert!((dx - 6.0).abs() < 0.3, "dx {dx}");
    assert!((dy + 3.0).abs() < 0.3, "dy {dy}");
    assert!(angle.abs() < 0.002, "angle {angle}");
}

#[test]
fn a_small_rotation_is_measured() {
    let before = image(texture);
    let after = moved(0.0, 0.0, 0.01);
    let [dx, dy, angle] = estimate(&before, &after).expect("enough texture");
    assert!((angle - 0.01).abs() < 0.003, "angle {angle}");
    assert!(dx.abs() < 0.5 && dy.abs() < 0.5, "shift {dx} {dy}");
}

#[test]
fn a_flat_frame_gives_no_motion() {
    let flat = image(|_, _| 128.0);
    assert_eq!(estimate(&flat, &flat), None);
}

#[test]
fn something_moving_on_its_own_is_left_out() {
    let before = image(texture);
    let after = image(|x, y| {
        if x < 90.0 && y < 70.0 {
            texture(x - 15.0, y - 10.0)
        } else {
            texture(x - 2.0, y)
        }
    });
    let [dx, dy, _] = estimate(&before, &after).expect("enough texture");
    assert!((dx - 2.0).abs() < 0.5, "dx {dx}");
    assert!(dy.abs() < 0.5, "dy {dy}");
}

#[test]
fn the_motion_size_keeps_the_aspect() {
    assert_eq!(motion_size(1920, 1080), (480, 270));
    assert_eq!(motion_size(320, 240), (320, 240));
}
