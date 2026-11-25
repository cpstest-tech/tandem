use super::*;

#[test]
fn a_shaking_shot_measures_its_shake() {
    let dir = vv_media::test_support::TempDir::new("vv-app-stabilize-test");
    let path = dir.join("shake.mkv");
    // A crop window jumping 8 pixels left and right on every frame.
    vv_media::test_support::ffmpeg(
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=s=320x240:r=25:d=1,crop=256:192:x='32+8*mod(n,2)':y=24",
            "-g",
            "1",
        ],
        &path,
    );
    let done = AtomicUsize::new(0);
    let motion = analyze(path, 0..20, &done, &AtomicBool::new(false)).unwrap();
    assert_eq!(motion.len(), 19);
    assert_eq!(done.load(Ordering::Relaxed), 20);
    for (i, [dx, _, _]) in motion.iter().enumerate() {
        let pixels = dx * 256.0;
        let expected = if i % 2 == 0 { -8.0 } else { 8.0 };
        assert!((pixels - expected).abs() < 2.0, "frame {i}: {pixels}");
    }
}
