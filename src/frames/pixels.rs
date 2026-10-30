//! Turns a frame's PNG into plain pixels and back, and flips pixels to face the other way.

use super::BYTES_PER_PIXEL;
use anyhow::{Context, Result, bail};
use std::io::Cursor;

/// A picture as rows of RGBA pixels, four bytes each, top row first.
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Reads a PNG built into the program.
pub fn decode(png: &[u8]) -> Result<Pixels> {
    let mut decoder = png::Decoder::new(Cursor::new(png));
    // Every sprite is saved as 8 bit RGBA; this keeps any other PNG in the same shape.
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .context("could not read a frame's header")?;
    let size = reader
        .output_buffer_size()
        .context("a frame is too big to read")?;
    let mut rgba = vec![0; size];
    let info = reader
        .next_frame(&mut rgba)
        .context("could not read a frame's pixels")?;
    if info.color_type != png::ColorType::Rgba {
        bail!("a frame is not RGBA");
    }
    rgba.truncate(info.buffer_size());
    Ok(Pixels {
        width: info.width,
        height: info.height,
        rgba,
    })
}

/// Writes pixels as a PNG, ready to send to the terminal.
pub fn encode(pixels: &Pixels) -> Result<Vec<u8>> {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, pixels.width, pixels.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().context("could not start a frame")?;
    writer
        .write_image_data(&pixels.rgba)
        .context("could not write a frame")?;
    writer.finish().context("could not finish a frame")?;
    Ok(png)
}

/// Mirrors the pixels left to right, so a cat facing right faces left.
pub fn flip(pixels: &mut Pixels) {
    let width = pixels.width as usize;
    if width == 0 {
        return;
    }
    for row in pixels.rgba.chunks_exact_mut(width * BYTES_PER_PIXEL) {
        for left in 0..width / 2 {
            let right = width - 1 - left;
            for byte in 0..BYTES_PER_PIXEL {
                row.swap(
                    left * BYTES_PER_PIXEL + byte,
                    right * BYTES_PER_PIXEL + byte,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three pixels in one row: red, green, blue.
    fn row() -> Pixels {
        Pixels {
            width: 3,
            height: 1,
            rgba: vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255],
        }
    }

    #[test]
    fn flipping_swaps_the_ends_of_each_row() {
        let mut pixels = row();
        flip(&mut pixels);
        assert_eq!(
            pixels.rgba,
            [0, 0, 255, 255, 0, 255, 0, 255, 255, 0, 0, 255]
        );
    }

    #[test]
    fn encoding_then_decoding_keeps_the_pixels() -> Result<()> {
        let original = row();
        let decoded = decode(&encode(&original)?)?;
        assert_eq!((decoded.width, decoded.height), (3, 1));
        assert_eq!(decoded.rgba, original.rgba);
        Ok(())
    }
}
