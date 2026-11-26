use super::*;

fn picture(dx: f32, dy: f32) -> Luma {
    let (w, h) = (160, 120);
    let mut data = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 - dx, y as f32 - dy);
            let v = 128.0
                + 60.0 * (fx * 0.21).sin() * (fy * 0.17).cos()
                + 40.0 * ((fx * 0.071 + fy * 0.113).sin());
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
fn a_patch_is_followed_across_frames() {
    let mut at = [80.0, 60.0];
    let mut previous = picture(0.0, 0.0);
    for i in 1..=10 {
        let next = picture(i as f32 * 3.0, i as f32 * -2.0);
        at = follow(&previous, &next, at);
        previous = next;
    }
    assert!((at[0] - 110.0).abs() < 1.5, "{at:?}");
    assert!((at[1] - 40.0).abs() < 1.5, "{at:?}");
}

#[test]
fn a_patch_near_the_edge_stays_put_instead_of_failing() {
    let a = picture(0.0, 0.0);
    assert_eq!(follow(&a, &a, [2.0, 2.0]), [2.0, 2.0]);
}
