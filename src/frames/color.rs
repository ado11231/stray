//! Colors the cat to match the terminal: white becomes the text color, black the background.

/// The largest value of one color channel.
const CHANNEL_MAX: u32 = 255;

/// The terminal's text and background colors, as red, green, and blue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub text: [u8; 3],
    pub background: [u8; 3],
}

impl Palette {
    /// Recolors RGBA pixels in place, leaving how see-through they are alone.
    ///
    /// Each pixel's brightness picks a color between the background, for black, and the
    /// text, for white. So the cat takes the text color and its eyes look like holes in it.
    pub fn apply(&self, rgba: &mut [u8]) {
        let (pixels, _) = rgba.as_chunks_mut::<{ super::BYTES_PER_PIXEL }>();
        for pixel in pixels {
            let brightness = (u32::from(pixel[0]) + u32::from(pixel[1]) + u32::from(pixel[2])) / 3;
            let colors = self.background.into_iter().zip(self.text);
            for (channel, (background, text)) in pixel.iter_mut().zip(colors) {
                *channel = mix(background, text, brightness);
            }
        }
    }
}

/// A color part of the way from `from` to `to`, where `share` runs from 0 to `CHANNEL_MAX`.
fn mix(from: u8, to: u8, share: u32) -> u8 {
    let mixed = (u32::from(from) * (CHANNEL_MAX - share) + u32::from(to) * share + CHANNEL_MAX / 2)
        / CHANNEL_MAX;
    // The mix never goes past either color, so it always fits in a byte.
    u8::try_from(mixed).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PALETTE: Palette = Palette {
        text: [200, 180, 160],
        background: [10, 20, 30],
    };

    #[test]
    fn white_takes_the_text_color_and_black_the_background() {
        let mut pixels = [255, 255, 255, 255, 0, 0, 0, 255];
        PALETTE.apply(&mut pixels);
        assert_eq!(pixels, [200, 180, 160, 255, 10, 20, 30, 255]);
    }

    #[test]
    fn grey_lands_between_and_see_through_stays_the_same() {
        let mut pixels = [128, 128, 128, 90];
        PALETTE.apply(&mut pixels);
        assert_eq!(pixels, [105, 100, 95, 90]);
    }
}
