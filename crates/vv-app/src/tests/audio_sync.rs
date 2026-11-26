use super::*;

#[test]
fn two_recordings_of_the_same_bursts_line_up() {
    let dir = vv_media::test_support::TempDir::new("vv-app-audio-sync");
    let bursts = "anoisesrc=d=10:seed=3:r=48000,volume='gt(sin(t*7.3)+sin(t*2.1),1.1)':eval=frame";
    let a = dir.join("a.wav");
    let b = dir.join("b.wav");
    vv_media::test_support::ffmpeg(&["-f", "lavfi", "-i", bursts], &a);
    vv_media::test_support::ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            &format!("{bursts},atrim=start=1.5,volume=0.4"),
        ],
        &b,
    );
    let side = |path: PathBuf, to_secs| Side {
        path,
        stream: 0,
        from_secs: 0.0,
        to_secs,
    };
    let (lag, confidence) = lag_of(&side(a, 10.0), &side(b, 8.5)).unwrap();
    let secs = lag as f64 / vv_audio::sync::ENVELOPE_RATE as f64;
    assert!((secs - 1.5).abs() < 0.03, "{secs} s");
    assert!(confidence > MIN_CONFIDENCE, "{confidence}");
}
