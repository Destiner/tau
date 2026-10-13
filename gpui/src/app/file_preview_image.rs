//! Admission precedes GPUI's eager decoder. Encoded size is not a pixel budget.
use gpui_kit::ImageFormat;
use std::sync::atomic::{AtomicBool, Ordering};

// 32 MiB retained BGRA pixels, at most 256 animation frames, and bounded axes.
// Decoder scratch/compositing buffers and the later display decode are additional,
// but each is bounded by these same dimensions; validation retains no frames.
const MAX_BYTES: u64 = 32 * 1024 * 1024;
const MAX_FRAMES: u64 = 256;
const MAX_AXIS: u32 = 8192;

pub(super) const DECODE_LIMITS: gpui_kit::ImageDecodeLimits = gpui_kit::ImageDecodeLimits {
    max_bytes: MAX_BYTES,
    max_frames: MAX_FRAMES,
    max_axis: MAX_AXIS,
};

#[derive(Default)]
struct Budget {
    bytes: u64,
    frames: u64,
}
impl Budget {
    fn frame(&mut self, width: u32, height: u32) -> Option<()> {
        if width == 0 || height == 0 || width > MAX_AXIS || height > MAX_AXIS {
            return None;
        }
        self.bytes = self
            .bytes
            .checked_add(u64::from(width) * u64::from(height) * 4)?;
        self.frames += 1;
        (self.bytes <= MAX_BYTES && self.frames <= MAX_FRAMES).then_some(())
    }
}

struct Input<'a> {
    bytes: &'a [u8],
    at: usize,
    cancelled: &'a AtomicBool,
}
impl<'a> Input<'a> {
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        if self.cancelled.load(Ordering::Relaxed) {
            return None;
        }
        let end = self.at.checked_add(len)?;
        let result = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(result)
    }
    fn byte(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn blocks(&mut self) -> Option<()> {
        loop {
            let len = self.byte()? as usize;
            if len == 0 {
                return Some(());
            }
            self.take(len)?;
        }
    }
}
fn le16(b: &[u8]) -> u32 {
    u32::from(u16::from_le_bytes([b[0], b[1]]))
}
fn le32(b: &[u8]) -> u32 {
    u32::from_le_bytes(b[..4].try_into().unwrap())
}
fn be16(b: &[u8]) -> u32 {
    u32::from(u16::from_be_bytes([b[0], b[1]]))
}
fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes(b[..4].try_into().unwrap())
}
fn png(b: &[u8], budget: &mut Budget) -> Option<()> {
    let header = b.get(..24)?;
    if &header[..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR" {
        return None;
    }
    budget.frame(be32(&header[16..20]), be32(&header[20..24]))
}
fn gif(input: &mut Input<'_>, budget: &mut Budget) -> Option<()> {
    let header = input.take(13)?;
    if &header[..6] != b"GIF87a" && &header[..6] != b"GIF89a" {
        return None;
    }
    let (width, height) = (le16(&header[6..8]), le16(&header[8..10]));
    // Check the canvas before any image descriptor, including empty animations.
    Budget::default().frame(width, height)?;
    if header[10] & 0x80 != 0 {
        input.take(3 * (1 << ((header[10] & 7) + 1)))?;
    }
    loop {
        match input.byte()? {
            0x3b => return (budget.frames > 0).then_some(()),
            0x21 => {
                input.byte()?;
                input.blocks()?;
            }
            0x2c => {
                let descriptor = input.take(9)?;
                // The decoder can allocate a subframe before canvas composition.
                Budget::default().frame(le16(&descriptor[4..6]), le16(&descriptor[6..8]))?;
                budget.frame(width, height)?;
                if descriptor[8] & 0x80 != 0 {
                    input.take(3 * (1 << ((descriptor[8] & 7) + 1)))?;
                }
                input.byte()?; // LZW code size; GPUI validates compressed data later.
                input.blocks()?;
            }
            _ => return None,
        }
    }
}
fn jpeg(input: &mut Input<'_>, budget: &mut Budget) -> Option<()> {
    if input.take(2)? != [0xff, 0xd8] {
        return None;
    }
    let mut found = false;
    let mut entropy = false;
    loop {
        if input.byte()? != 0xff {
            if entropy {
                continue;
            }
            return None;
        }
        let mut marker = input.byte()?;
        while marker == 0xff {
            marker = input.byte()?;
        }
        if marker == 0 && entropy {
            continue;
        }
        if marker == 0xd9 {
            return found.then_some(());
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if marker == 0 || marker == 0xd8 {
            return None;
        }
        entropy = false;
        let len = be16(input.take(2)?) as usize;
        let segment = input.take(len.checked_sub(2)?)?;
        // Inspect all headers, even those after entropy-coded scans. Never let
        // a later SOF replace a small admitted size with a huge scratch buffer.
        if (0xc0..=0xcf).contains(&marker) && ![0xc4, 0xc8, 0xcc].contains(&marker) {
            let dims = segment.get(..5)?;
            budget.frame(be16(&dims[3..5]), be16(&dims[1..3]))?;
            found = true;
        }
        if marker == 0xda {
            entropy = true;
        }
    }
}
fn bmp(b: &[u8], budget: &mut Budget, icon: bool) -> Option<()> {
    let dib = if icon {
        b
    } else {
        if b.get(..2)? != b"BM" {
            return None;
        }
        b.get(14..)?
    };
    let size = le32(dib.get(..4)?);
    let (width, height) = if size == 12 {
        let dims = dib.get(4..8)?;
        (le16(dims), le16(&dims[2..]))
    } else if size >= 40 {
        let dims = dib.get(4..12)?;
        let width = i32::from_le_bytes(dims[..4].try_into().ok()?);
        let height = i32::from_le_bytes(dims[4..].try_into().ok()?);
        (u32::try_from(width).ok()?, height.unsigned_abs())
    } else {
        return None;
    };
    budget.frame(width, if icon { height / 2 } else { height })
}
fn webp(input: &mut Input<'_>, budget: &mut Budget) -> Option<()> {
    let header = input.take(12)?;
    if &header[..4] != b"RIFF" || &header[8..12] != b"WEBP" {
        return None;
    }
    let mut found = false;
    while input.at < input.bytes.len() {
        let chunk = input.take(8)?;
        let len = le32(&chunk[4..]) as usize;
        let data = input.take(len)?;
        let dims = match &chunk[..4] {
            b"VP8X" => {
                let d = data.get(..10)?;
                Some((1 + le24(&d[4..7]), 1 + le24(&d[7..10])))
            }
            b"VP8 " => {
                let d = data.get(..10)?;
                if &d[3..6] != b"\x9d\x01\x2a" {
                    return None;
                }
                Some((le16(&d[6..8]) & 0x3fff, le16(&d[8..10]) & 0x3fff))
            }
            b"VP8L" => {
                let d = data.get(..5)?;
                if d[0] != 0x2f {
                    return None;
                }
                let bits = le32(&d[1..]);
                Some((1 + (bits & 0x3fff), 1 + ((bits >> 14) & 0x3fff)))
            }
            // GPUI treats WebP as static, but its decoder can allocate animation
            // scratch. Fail closed rather than trust uninspected nested frames.
            b"ANIM" | b"ANMF" => return None,
            _ => None,
        };
        if let Some((width, height)) = dims {
            let mut frame = Budget::default();
            frame.frame(width, height)?;
            // Extended and bitstream headers describe the same static frame.
            budget.bytes = budget.bytes.max(frame.bytes);
            budget.frames = 1;
            found = true;
        }
        if len & 1 != 0 {
            input.take(1)?;
        }
    }
    found.then_some(())
}
fn le24(b: &[u8]) -> u32 {
    u32::from(b[0]) | (u32::from(b[1]) << 8) | (u32::from(b[2]) << 16)
}

pub(super) fn admit(format: ImageFormat, bytes: &[u8], cancelled: &AtomicBool) -> Option<()> {
    let mut input = Input {
        bytes,
        at: 0,
        cancelled,
    };
    input.take(0)?;
    let mut budget = Budget::default();
    match format {
        ImageFormat::Gif => gif(&mut input, &mut budget),
        ImageFormat::Png => png(bytes, &mut budget),
        ImageFormat::Jpeg => jpeg(&mut input, &mut budget),
        ImageFormat::Bmp => bmp(bytes, &mut budget, false),
        ImageFormat::Webp => webp(&mut input, &mut budget),
        ImageFormat::Ico => {
            let h = input.take(6)?;
            if h[..4] != [0, 0, 1, 0] {
                return None;
            }
            let count = le16(&h[4..]) as usize;
            if count == 0 || count > MAX_FRAMES as usize {
                return None;
            }
            for _ in 0..count {
                let entry = input.take(16)?;
                let start = le32(&entry[12..]) as usize;
                let end = start.checked_add(le32(&entry[8..12]) as usize)?;
                let data = bytes.get(start..end)?;
                // Inspect actual embedded dimensions, not the one-byte directory hints.
                if data.starts_with(b"\x89PNG") {
                    png(data, &mut budget)?;
                } else {
                    bmp(data, &mut budget, true)?;
                }
            }
            Some(())
        }
        ImageFormat::Svg => {
            let text = std::str::from_utf8(bytes).ok()?;
            let document = roxmltree::Document::parse_with_options(
                text,
                roxmltree::ParsingOptions {
                    allow_dtd: true,
                    nodes_limit: 100_000,
                    ..Default::default()
                },
            )
            .ok()?;
            // usvg decodes embedded raster images while building the tree.
            if document.descendants().any(|node| {
                cancelled.load(Ordering::Relaxed)
                    || node.has_tag_name("image")
                    || node.has_tag_name("feImage")
            }) {
                return None;
            }
            input.take(0)?;
            // Physical output is guarded by GPUI after its actual font resolution.
            Some(())
        }
        _ => None,
    }?;
    input.take(0)?;
    Some(())
}

#[cfg(test)]
#[path = "file_preview_image_tests.rs"]
mod tests;
