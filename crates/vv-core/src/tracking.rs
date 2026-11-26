//! Point tracking: a small patch of the picture followed from frame to frame
//! by template matching, for overlays that stick to a moving object.

use crate::stabilize::Luma;

/// Half size of the followed patch, in analysis pixels.
pub const PATCH_RADIUS: i32 = 8;
/// How far the patch may move between two frames, in analysis pixels.
pub const SEARCH_RADIUS: i32 = 16;

fn sad(a: &Luma, (ax, ay): (i32, i32), b: &Luma, (bx, by): (i32, i32), r: i32, best: u32) -> u32 {
    let mut sum = 0u32;
    for dy in -r..=r {
        let (ya, yb) = (ay + dy, by + dy);
        if ya < 0 || yb < 0 || ya >= a.height as i32 || yb >= b.height as i32 {
            return u32::MAX;
        }
        for dx in -r..=r {
            let (xa, xb) = (ax + dx, bx + dx);
            if xa < 0 || xb < 0 || xa >= a.width as i32 || xb >= b.width as i32 {
                return u32::MAX;
            }
            let pa = a.data[ya as usize * a.width + xa as usize] as i32;
            let pb = b.data[yb as usize * b.width + xb as usize] as i32;
            sum += pa.abs_diff(pb);
        }
        if sum >= best {
            return sum;
        }
    }
    sum
}

/// Where the patch around `at` in `previous` went in `next`: the best match
/// within the search window, refined to a fraction of a pixel.
pub fn follow(previous: &Luma, next: &Luma, at: [f32; 2]) -> [f32; 2] {
    let (cx, cy) = (at[0].round() as i32, at[1].round() as i32);
    let mut best = (u32::MAX, 0, 0);
    for dy in -SEARCH_RADIUS..=SEARCH_RADIUS {
        for dx in -SEARCH_RADIUS..=SEARCH_RADIUS {
            let cost = sad(
                previous,
                (cx, cy),
                next,
                (cx + dx, cy + dy),
                PATCH_RADIUS,
                best.0,
            );
            if cost < best.0 {
                best = (cost, dx, dy);
            }
        }
    }
    if best.0 == u32::MAX {
        return at;
    }
    let (_, dx, dy) = best;
    let refine = |m: u32, c: u32, p: u32| {
        if m == u32::MAX || p == u32::MAX {
            return 0.0;
        }
        let (m, c, p) = (m as f32, c as f32, p as f32);
        let denom = m - 2.0 * c + p;
        if denom.abs() < 1e-3 {
            0.0
        } else {
            (0.5 * (m - p) / denom).clamp(-0.5, 0.5)
        }
    };
    let cost = |ox: i32, oy: i32| {
        sad(
            previous,
            (cx, cy),
            next,
            (cx + dx + ox, cy + dy + oy),
            PATCH_RADIUS,
            u32::MAX,
        )
    };
    let c = best.0;
    let fx = refine(cost(-1, 0), c, cost(1, 0));
    let fy = refine(cost(0, -1), c, cost(0, 1));
    [at[0] + dx as f32 + fx, at[1] + dy as f32 + fy]
}

#[cfg(test)]
#[path = "tests/tracking.rs"]
mod tests;
