//! Lining up two recordings of the same moment (camera audio and an
//! external microphone) by the cross-correlation of their loudness envelopes.

/// Envelope samples per second: 5 ms resolution, enough for lip sync.
pub const ENVELOPE_RATE: u32 = 200;

/// Loudness envelope of interleaved `samples` at `rate`: mean absolute value
/// per window, compressed and with its slow trend removed, so two
/// microphones at different distances still look alike.
pub fn envelope(samples: &[f32], channels: u16, rate: u32) -> Vec<f32> {
    let channels = channels.max(1) as usize;
    let window = (rate / ENVELOPE_RATE).max(1) as usize * channels;
    let raw: Vec<f32> = samples
        .chunks(window)
        .map(|w| (w.iter().map(|s| s.abs()).sum::<f32>() / w.len() as f32 * 50.0).ln_1p())
        .collect();
    // Minus a half-second moving average: what is left are the onsets.
    let half = (ENVELOPE_RATE / 4) as usize;
    let mut prefix = vec![0.0f64; raw.len() + 1];
    for (i, v) in raw.iter().enumerate() {
        prefix[i + 1] = prefix[i] + *v as f64;
    }
    (0..raw.len())
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(half), (i + half + 1).min(raw.len()));
            let mean = (prefix[hi] - prefix[lo]) / (hi - lo) as f64;
            (raw[i] as f64 - mean).max(0.0) as f32
        })
        .collect()
}

fn fft(re: &mut [f64], im: &mut [f64], inverse: bool) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = 2.0 * std::f64::consts::PI / len as f64 * if inverse { 1.0 } else { -1.0 };
        let (wr, wi) = (angle.cos(), angle.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let next = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = next;
            }
        }
        len <<= 1;
    }
    if inverse {
        for v in re.iter_mut().chain(im.iter_mut()) {
            *v /= n as f64;
        }
    }
}

/// The lag `L` (envelope samples) with `b[i] ≈ a[i + L]`, and a 0..1
/// confidence: the peak's correlation coefficient.
pub fn best_lag(a: &[f32], b: &[f32]) -> Option<(i64, f32)> {
    if a.len() < 8 || b.len() < 8 {
        return None;
    }
    let normalized = |v: &[f32]| {
        let mean = v.iter().map(|x| *x as f64).sum::<f64>() / v.len() as f64;
        let centred: Vec<f64> = v.iter().map(|x| *x as f64 - mean).collect();
        let norm = centred.iter().map(|x| x * x).sum::<f64>().sqrt().max(1e-12);
        centred.into_iter().map(|x| x / norm).collect::<Vec<f64>>()
    };
    let (a, b) = (normalized(a), normalized(b));
    let n = (a.len() + b.len()).next_power_of_two();
    let (mut ar, mut ai) = (vec![0.0; n], vec![0.0; n]);
    let (mut br, mut bi) = (vec![0.0; n], vec![0.0; n]);
    ar[..a.len()].copy_from_slice(&a);
    br[..b.len()].copy_from_slice(&b);
    fft(&mut ar, &mut ai, false);
    fft(&mut br, &mut bi, false);
    // A · conj(B): its inverse at k is sum_i a[i + k] b[i].
    let (mut cr, mut ci) = (vec![0.0; n], vec![0.0; n]);
    for k in 0..n {
        cr[k] = ar[k] * br[k] + ai[k] * bi[k];
        ci[k] = ai[k] * br[k] - ar[k] * bi[k];
    }
    fft(&mut cr, &mut ci, true);
    let (index, peak) = cr.iter().enumerate().max_by(|x, y| x.1.total_cmp(y.1))?;
    let lag = if index > n / 2 {
        index as i64 - n as i64
    } else {
        index as i64
    };
    Some((lag, peak.clamp(0.0, 1.0) as f32))
}

#[cfg(test)]
#[path = "tests/sync.rs"]
mod tests;
