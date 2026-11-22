use std::path::Path;

pub fn frame_png(path: &Path, at_seconds: f64) -> Result<Vec<u8>, String> {
    frame_png_sized(path, at_seconds, 320)
}

pub fn frame_png_sized(path: &Path, at_seconds: f64, max_width: u32) -> Result<Vec<u8>, String> {
    let mut decoder = vv_media::Decoder::open(path).map_err(|e| e.to_string())?;
    if at_seconds > 0.0 {
        decoder
            .seek_to_time(at_seconds)
            .map_err(|e| e.to_string())?;
    }
    let (_, frame) = decoder
        .next_frame()
        .map_err(|e| e.to_string())?
        .ok_or("No video frame at the selected time")?;
    let thumb = vv_media::thumbnail::downscale_to_rgba(&frame, max_width);
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, thumb.width, thumb.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .map_err(|e| e.to_string())?
            .write_image_data(&thumb.rgba)
            .map_err(|e| e.to_string())?;
    }
    Ok(bytes)
}
