#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct PixelStats {
    pub(crate) pixels: usize,
    pub(crate) bytes: usize,
    pub(crate) nonzero_bytes: usize,
    pub(crate) nonzero_pixels: usize,
    pub(crate) min: u8,
    pub(crate) max: u8,
    pub(crate) sample: Vec<u8>,
}

pub(crate) fn resize_rgb_nearest(
    rgb: &[[u8; 3]],
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> Result<Vec<[u8; 3]>, String> {
    if src_width == 0 || src_height == 0 || dst_width == 0 || dst_height == 0 {
        return Err("texture resize dimensions must be >= 1".to_string());
    }
    let expected_len = src_width
        .checked_mul(src_height)
        .ok_or_else(|| "texture resize source size overflow".to_string())?
        as usize;
    if rgb.len() != expected_len {
        return Err(format!(
            "texture resize source length {} does not match {}x{}",
            rgb.len(),
            src_width,
            src_height
        ));
    }
    if src_width == dst_width && src_height == dst_height {
        return Ok(rgb.to_vec());
    }
    let dst_len = dst_width
        .checked_mul(dst_height)
        .ok_or_else(|| "texture resize destination size overflow".to_string())?
        as usize;
    let mut out = Vec::with_capacity(dst_len);
    for y in 0..dst_height {
        let src_y = (u64::from(y) * u64::from(src_height) / u64::from(dst_height)) as u32;
        for x in 0..dst_width {
            let src_x = (u64::from(x) * u64::from(src_width) / u64::from(dst_width)) as u32;
            let index = (src_y * src_width + src_x) as usize;
            out.push(rgb[index]);
        }
    }
    Ok(out)
}

pub(crate) fn pixel_stats_from_rgb(rgb: &[[u8; 3]]) -> PixelStats {
    let mut bytes = Vec::with_capacity(rgb.len().min(8) * 3);
    let mut nonzero_bytes = 0usize;
    let mut nonzero_pixels = 0usize;
    let mut min = u8::MAX;
    let mut max = 0u8;
    for pixel in rgb {
        if pixel.iter().any(|channel| *channel != 0) {
            nonzero_pixels += 1;
        }
        for channel in pixel {
            if *channel != 0 {
                nonzero_bytes += 1;
            }
            min = min.min(*channel);
            max = max.max(*channel);
            if bytes.len() < 24 {
                bytes.push(*channel);
            }
        }
    }
    if rgb.is_empty() {
        min = 0;
    }
    PixelStats {
        pixels: rgb.len(),
        bytes: rgb.len().saturating_mul(3),
        nonzero_bytes,
        nonzero_pixels,
        min,
        max,
        sample: bytes,
    }
}

pub(crate) fn pixel_stats_from_bytes(bytes: &[u8], stride: usize) -> PixelStats {
    let stride = stride.max(1);
    let pixels = bytes.len() / stride;
    let mut nonzero_pixels = 0usize;
    let mut min = u8::MAX;
    let mut max = 0u8;
    for pixel in bytes.chunks(stride) {
        if pixel.iter().any(|channel| *channel != 0) {
            nonzero_pixels += 1;
        }
        for channel in pixel {
            min = min.min(*channel);
            max = max.max(*channel);
        }
    }
    if bytes.is_empty() {
        min = 0;
    }
    PixelStats {
        pixels,
        bytes: bytes.len(),
        nonzero_bytes: bytes.iter().filter(|byte| **byte != 0).count(),
        nonzero_pixels,
        min,
        max,
        sample: bytes.iter().take(24).copied().collect(),
    }
}

pub(crate) fn rgb_content_hash(rgb: &[[u8; 3]]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for pixel in rgb {
        for byte in pixel {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

pub(crate) fn format_pixel_stats(stats: &PixelStats) -> String {
    let sample = stats
        .sample
        .iter()
        .map(|byte| byte.to_string())
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "pixels={} bytes={} nonzero_pixels={} nonzero_bytes={} min={} max={} sample=[{}]",
        stats.pixels,
        stats.bytes,
        stats.nonzero_pixels,
        stats.nonzero_bytes,
        stats.min,
        stats.max,
        sample
    )
}

pub(crate) fn rgb_is_blank(rgb: &[[u8; 3]]) -> bool {
    rgb.iter()
        .all(|pixel| pixel.iter().all(|channel| *channel == 0))
}
