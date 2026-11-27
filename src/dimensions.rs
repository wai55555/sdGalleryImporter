//! 画像の実寸取得（REQ-6.5）。
//!
//! fast_meta は寸法を返さないため、形式判定（`ScanOutcome.format`）に応じて
//! ファイルヘッダを直接読む。取得できない場合は None。

use fast_meta::Format;
use std::fs;
use std::path::Path;

pub fn read(path: &Path, format: Format) -> Option<(u32, u32)> {
    let bytes = fs::read(path).ok()?;
    match format {
        Format::Png => png_size(&bytes),
        Format::Jpeg => jpeg_size(&bytes),
        Format::Webp => webp_size(&bytes),
    }
}

fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.get(..8)? != SIGNATURE || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    nonzero(width, height)
}

fn jpeg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..2)? != [0xff, 0xd8] {
        return None;
    }
    let mut cursor = 2usize;
    while cursor + 4 <= bytes.len() {
        if bytes[cursor] != 0xff {
            return None;
        }
        let mut marker = bytes[cursor + 1];
        while marker == 0xff {
            cursor += 1;
            marker = *bytes.get(cursor + 1)?;
        }
        cursor += 2;
        match marker {
            0xd8 | 0x01 | 0xd0..=0xd7 => continue,
            0xd9 | 0xda => return None,
            _ => {}
        }
        let length = u16::from_be_bytes(bytes.get(cursor..cursor + 2)?.try_into().ok()?) as usize;
        if length < 2 {
            return None;
        }
        let data = bytes.get(cursor + 2..cursor + length)?;
        if is_sof(marker) {
            let height = u16::from_be_bytes(data.get(1..3)?.try_into().ok()?) as u32;
            let width = u16::from_be_bytes(data.get(3..5)?.try_into().ok()?) as u32;
            return nonzero(width, height);
        }
        cursor += length;
    }
    None
}

fn is_sof(marker: u8) -> bool {
    matches!(
        marker,
        0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf
    )
}

fn webp_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WEBP" {
        return None;
    }
    let mut cursor = 12usize;
    let mut canvas = None;
    let mut lossless = None;
    let mut lossy = None;
    while cursor + 8 <= bytes.len() {
        let kind = bytes.get(cursor..cursor + 4)?;
        let length =
            u32::from_le_bytes(bytes.get(cursor + 4..cursor + 8)?.try_into().ok()?) as usize;
        let data = bytes.get(cursor + 8..cursor + 8 + length)?;
        match kind {
            b"VP8X" => {
                let width = u24(&data[4..7])? + 1;
                let height = u24(&data[7..10])? + 1;
                canvas = nonzero(width, height);
            }
            b"VP8L" => {
                if data.first() != Some(&0x2f) {
                    continue;
                }
                let bits = u32::from_le_bytes(data.get(1..5)?.try_into().ok()?);
                let width = (bits & 0x3fff) + 1;
                let height = ((bits >> 14) & 0x3fff) + 1;
                lossless = nonzero(width, height);
            }
            b"VP8 " => {
                if data.get(3..6)? != [0x9d, 0x01, 0x2a] {
                    continue;
                }
                let width = (u16::from_le_bytes(data.get(6..8)?.try_into().ok()?) & 0x3fff) as u32;
                let height =
                    (u16::from_le_bytes(data.get(8..10)?.try_into().ok()?) & 0x3fff) as u32;
                lossy = nonzero(width, height);
            }
            _ => {}
        }
        cursor += 8 + length + (length % 2);
    }
    canvas.or(lossless).or(lossy)
}

fn u24(bytes: &[u8]) -> Option<u32> {
    Some(
        u32::from(bytes.get(0..3)?[0])
            | (u32::from(bytes.get(0..3)?[1]) << 8)
            | (u32::from(bytes.get(0..3)?[2]) << 16),
    )
}

fn nonzero(width: u32, height: u32) -> Option<(u32, u32)> {
    (width > 0 && height > 0).then_some((width, height))
}

#[cfg(test)]
mod tests {
    use super::{jpeg_size, png_size, webp_size};

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    fn jpeg_with_sof(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8];
        bytes.extend_from_slice(&[0xff, 0xe0, 0x00, 0x04, 0x00, 0x00]); // APP0 dummy
        let mut sof = vec![0xff, 0xc0, 0x00, 0x11, 8];
        sof.extend_from_slice(&height.to_be_bytes());
        sof.extend_from_slice(&width.to_be_bytes());
        sof.extend_from_slice(&[3, 1, 0x11, 0, 2, 0x11, 1, 3, 0x11, 1]);
        bytes.extend_from_slice(&sof);
        bytes.extend_from_slice(&[0xff, 0xda, 0x00, 0x02, 0xff, 0xd9]);
        bytes
    }

    #[test]
    fn reads_png_ihdr() {
        assert_eq!(png_size(&png_header(832, 1216)), Some((832, 1216)));
        assert_eq!(png_size(&png_header(0, 10)), None);
    }

    #[test]
    fn reads_jpeg_sof() {
        assert_eq!(jpeg_size(&jpeg_with_sof(512, 768)), Some((512, 768)));
    }

    #[test]
    fn reads_webp_vp8x() {
        let mut vp8x = vec![0, 0, 0, 0];
        vp8x.extend_from_slice(&(1023u32).to_le_bytes()[..3]);
        vp8x.extend_from_slice(&(767u32).to_le_bytes()[..3]);
        let mut body = b"WEBP".to_vec();
        body.extend_from_slice(b"VP8X");
        body.extend_from_slice(&(vp8x.len() as u32).to_le_bytes());
        body.extend_from_slice(&vp8x);
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&body);
        assert_eq!(webp_size(&bytes), Some((1024, 768)));
    }
}
