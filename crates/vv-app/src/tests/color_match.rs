use super::*;

#[test]
fn a_decoded_red_frame_samples_as_red() {
    let dir = vv_media::test_support::TempDir::new("vv-app-color-match");
    let path = dir.join("red.mkv");
    vv_media::test_support::ffmpeg(
        &["-f", "lavfi", "-i", "color=c=red:size=64x36:rate=25:d=1"],
        &path,
    );
    let mut decoder = vv_media::Decoder::open(&path).unwrap();
    let (_, frame) = decoder.next_frame().unwrap().unwrap();
    let samples = sample_rgb(&frame);
    assert_eq!(samples.len(), GRID * GRID);
    let [r, g, b] = samples[samples.len() / 2];
    assert!(r > 0.85 && g < 0.15 && b < 0.15, "{r} {g} {b}");
}
