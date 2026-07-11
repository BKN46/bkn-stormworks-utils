use std::{fs, path::Path};

pub(crate) fn sanitize_filename_part(value: &str) -> String {
    let mut sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        sanitized.push_str("none");
    }
    sanitized
}

pub(crate) fn write_frame_preview_bmp(
    path: &Path,
    width: u32,
    height: u32,
    rgb: &[[u8; 3]],
    scale: u32,
) -> Result<(), String> {
    let scale = scale.max(1);
    let out_width = width
        .checked_mul(scale)
        .ok_or_else(|| "frame preview width overflow".to_string())?;
    let out_height = height
        .checked_mul(scale)
        .ok_or_else(|| "frame preview height overflow".to_string())?;
    let row_stride = ((out_width * 3 + 3) / 4) * 4;
    let image_size = row_stride
        .checked_mul(out_height)
        .ok_or_else(|| "frame preview image size overflow".to_string())?;
    let file_size = 14u32
        .checked_add(40)
        .and_then(|value| value.checked_add(image_size))
        .ok_or_else(|| "frame preview file size overflow".to_string())?;
    let expected_pixels = width
        .checked_mul(height)
        .ok_or_else(|| "frame preview source pixel count overflow".to_string())?
        as usize;
    if rgb.len() < expected_pixels {
        return Err("frame preview source pixel buffer is incomplete".to_string());
    }
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut bytes = Vec::with_capacity(file_size as usize);
    bytes.extend_from_slice(b"BM");
    bytes.extend_from_slice(&file_size.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&54u32.to_le_bytes());
    bytes.extend_from_slice(&40u32.to_le_bytes());
    bytes.extend_from_slice(&(out_width as i32).to_le_bytes());
    bytes.extend_from_slice(&(out_height as i32).to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&24u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&image_size.to_le_bytes());
    bytes.extend_from_slice(&2835i32.to_le_bytes());
    bytes.extend_from_slice(&2835i32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());

    let padding = (row_stride - out_width * 3) as usize;
    for out_y in (0..out_height).rev() {
        let src_y = (out_y / scale).min(height.saturating_sub(1));
        for out_x in 0..out_width {
            let src_x = (out_x / scale).min(width.saturating_sub(1));
            let index = (src_y * width + src_x) as usize;
            let [r, g, b] = rgb[index];
            bytes.push(b);
            bytes.push(g);
            bytes.push(r);
        }
        bytes.extend(std::iter::repeat(0).take(padding));
    }
    fs::write(path, bytes).map_err(|error| format!("write frame preview failed: {error}"))
}
