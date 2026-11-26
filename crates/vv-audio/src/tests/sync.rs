use super::*;

/// Clicks at irregular times, as speech onsets would be, at 8 kHz.
fn clicks(times: &[f64], len_secs: f64, gain: f32, noise: f32) -> Vec<f32> {
    let rate = 8000.0;
    let mut out = vec![0.0f32; (len_secs * rate) as usize];
    let mut seed = 12345u32;
    for s in out.iter_mut() {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        *s = noise * ((seed >> 16) as f32 / 32768.0 - 1.0);
    }
    for &t in times {
        let start = (t * rate) as usize;
        for i in 0..400 {
            if let Some(s) = out.get_mut(start + i) {
                *s += gain * (i as f32 * 0.3).sin() * (1.0 - i as f32 / 400.0);
            }
        }
    }
    out
}

#[test]
fn the_lag_of_a_shifted_recording_is_found() {
    let events = [0.7, 1.9, 2.3, 4.1, 5.6, 6.0, 7.8, 9.4];
    let a = clicks(&events, 12.0, 0.8, 0.01);
    // The same events 1.5 s earlier, quieter and noisier.
    let shifted: Vec<f64> = events
        .iter()
        .filter(|t| **t > 1.5)
        .map(|t| t - 1.5)
        .collect();
    let b = clicks(&shifted, 9.0, 0.3, 0.03);
    let (lag, confidence) = best_lag(&envelope(&a, 1, 8000), &envelope(&b, 1, 8000)).unwrap();
    assert!((lag - 300).abs() <= 2, "lag {lag}");
    assert!(confidence > 0.5, "{confidence}");
}

#[test]
fn unrelated_recordings_have_a_low_confidence() {
    let a = clicks(&[0.5, 1.5, 3.2, 4.4], 6.0, 0.8, 0.01);
    let b = clicks(&[0.3, 2.9, 3.0, 5.1, 5.3], 6.0, 0.8, 0.01);
    let (_, confidence) = best_lag(&envelope(&a, 1, 8000), &envelope(&b, 1, 8000)).unwrap();
    assert!(confidence < 0.6, "{confidence}");
}
