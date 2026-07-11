use serde::Serialize;
use std::{fs, path::PathBuf};
use stormworks_modkit_shared::PluginRuntimeContext;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ByteCheckSummary {
    pub(crate) checked: usize,
    pub(crate) verified: usize,
    pub(crate) failed: usize,
    pub(crate) failures: Vec<String>,
}

pub(crate) fn read_pe_image_base(path: &PathBuf) -> Result<u64, String> {
    let image = fs::read(path).map_err(|error| format!("reading {}: {error}", path.display()))?;
    Ok(PeImage::parse(&image)?.image_base)
}

pub(crate) fn verify_signature_bytes(
    context: &PluginRuntimeContext,
    symbols: &serde_json::Value,
) -> Result<ByteCheckSummary, String> {
    let image = fs::read(&context.game_exe)
        .map_err(|error| format!("reading game exe {}: {error}", context.game_exe.display()))?;
    let pe = PeImage::parse(&image)?;
    let mut summary = ByteCheckSummary {
        checked: 0,
        verified: 0,
        failed: 0,
        failures: Vec::new(),
    };

    if let Some(object) = symbols.as_object() {
        for (group_name, group) in object {
            let Some(values) = group.get("value").and_then(|value| value.as_array()) else {
                continue;
            };
            for candidate in values {
                let Some(byte_check) = candidate.get("byte_check") else {
                    continue;
                };
                summary.checked += 1;
                let label = candidate
                    .get("entry")
                    .and_then(|value| value.as_str())
                    .unwrap_or("<missing-entry>");
                match verify_one_byte_check(&image, &pe, byte_check) {
                    Ok(()) => summary.verified += 1,
                    Err(error) => {
                        summary.failed += 1;
                        summary
                            .failures
                            .push(format!("{group_name}:{label}: {error}"));
                    }
                }
            }
        }
    }

    if summary.failed != 0 {
        return Err(format!(
            "{} of {} signature byte checks failed: {}",
            summary.failed,
            summary.checked,
            summary.failures.join("; ")
        ));
    }
    Ok(summary)
}

fn verify_one_byte_check(
    image: &[u8],
    pe: &PeImage,
    byte_check: &serde_json::Value,
) -> Result<(), String> {
    let va_text = byte_check
        .get("va")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "missing byte_check.va".to_string())?;
    let expected = byte_check
        .get("bytes")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "missing byte_check.bytes".to_string())
        .and_then(parse_hex_bytes)?;
    let va = u64::from_str_radix(va_text.trim_start_matches("0x"), 16)
        .map_err(|error| format!("invalid byte_check.va {va_text}: {error}"))?;
    let raw = pe
        .va_to_raw(va)
        .ok_or_else(|| format!("VA {va_text} is outside mapped sections"))?;
    let end = raw + expected.len();
    if end > image.len() {
        return Err(format!("VA {va_text} byte check extends beyond file"));
    }
    let actual = &image[raw..end];
    if actual != expected.as_slice() {
        return Err(format!(
            "VA {va_text} mismatch: expected {}, got {}",
            hex_bytes(&expected),
            hex_bytes(actual)
        ));
    }
    Ok(())
}

fn parse_hex_bytes(text: &str) -> Result<Vec<u8>, String> {
    text.split_whitespace()
        .map(|part| {
            u8::from_str_radix(part, 16)
                .map_err(|error| format!("invalid hex byte {part}: {error}"))
        })
        .collect()
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug)]
struct PeImage {
    image_base: u64,
    sections: Vec<PeSection>,
}

#[derive(Debug)]
struct PeSection {
    virtual_address: u32,
    virtual_size: u32,
    raw_ptr: u32,
    raw_size: u32,
}

impl PeImage {
    fn parse(image: &[u8]) -> Result<Self, String> {
        if image.len() < 0x40 || &image[0..2] != b"MZ" {
            return Err("invalid PE: missing MZ header".to_string());
        }
        let pe_offset = read_u32(image, 0x3c)? as usize;
        if pe_offset + 0x108 > image.len() || &image[pe_offset..pe_offset + 4] != b"PE\0\0" {
            return Err("invalid PE: missing PE signature".to_string());
        }
        let section_count = read_u16(image, pe_offset + 6)? as usize;
        let optional_size = read_u16(image, pe_offset + 20)? as usize;
        let optional_offset = pe_offset + 24;
        let magic = read_u16(image, optional_offset)?;
        if magic != 0x20b {
            return Err(format!("unsupported PE optional header magic {magic:#x}"));
        }
        let image_base = read_u64(image, optional_offset + 24)?;
        let section_offset = optional_offset + optional_size;
        let mut sections = Vec::new();
        for index in 0..section_count {
            let offset = section_offset + index * 40;
            if offset + 40 > image.len() {
                return Err("invalid PE: section table truncated".to_string());
            }
            sections.push(PeSection {
                virtual_size: read_u32(image, offset + 8)?,
                virtual_address: read_u32(image, offset + 12)?,
                raw_size: read_u32(image, offset + 16)?,
                raw_ptr: read_u32(image, offset + 20)?,
            });
        }
        Ok(Self {
            image_base,
            sections,
        })
    }

    fn va_to_raw(&self, va: u64) -> Option<usize> {
        let rva = va.checked_sub(self.image_base)? as u32;
        for section in &self.sections {
            let section_size = section.virtual_size.max(section.raw_size);
            if rva >= section.virtual_address && rva < section.virtual_address + section_size {
                let offset = rva - section.virtual_address;
                if offset < section.raw_size {
                    return Some((section.raw_ptr + offset) as usize);
                }
            }
        }
        None
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let end = offset + 2;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| format!("PE read out of bounds at {offset:#x}"))?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset + 4;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| format!("PE read out of bounds at {offset:#x}"))?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, String> {
    let end = offset + 8;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| format!("PE read out of bounds at {offset:#x}"))?;
    Ok(u64::from_le_bytes([
        value[0], value[1], value[2], value[3], value[4], value[5], value[6], value[7],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_bytes() {
        assert_eq!(parse_hex_bytes("48 8b c4").unwrap(), vec![0x48, 0x8b, 0xc4]);
    }
}
