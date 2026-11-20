//! On-screen text, drawn with a built-in 5x7 bitmap font.

const GLYPH_WIDTH: usize = 5;
const GLYPH_HEIGHT: usize = 7;
/// Glyph size plus spacing.
const CELL_WIDTH: usize = GLYPH_WIDTH + 1;
const CELL_HEIGHT: usize = GLYPH_HEIGHT + 2;

const TEXT: u32 = 0x00FF_FFFF;
const BACKDROP: u32 = 0x0010_1018;

/// Rows of a glyph, top to bottom; bit 4 is the leftmost pixel. Only capitals,
/// digits and a little punctuation exist; anything else is drawn as a space.
#[rustfmt::skip]
fn glyph(c: char) -> [u8; GLYPH_HEIGHT] {
    match c.to_ascii_uppercase() {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        ':' => [0b00000, 0b01100, 0b01100, 0b00000, 0b01100, 0b01100, 0b00000],
        '+' => [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100],
        ',' => [0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b00100, 0b01000],
        '/' => [0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000],
        '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100],
        '?' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100],
        _ => [0; GLYPH_HEIGHT],
    }
}

/// Where a block of text sits in the window.
#[derive(Clone, Copy)]
pub enum Anchor {
    Centre,
    BottomLeft,
}

/// A pixel buffer being drawn into.
pub struct Canvas<'a> {
    pub pixels: &'a mut [u32],
    pub width: usize,
    pub height: usize,
}

impl Canvas<'_> {
    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, colour: u32) {
        for row in y..(y + h).min(self.height) {
            let start = row * self.width + x.min(self.width);
            let end = row * self.width + (x + w).min(self.width);
            self.pixels[start..end].fill(colour);
        }
    }

    /// Draws `text` (which may span several lines) on a dark panel. `scale` is
    /// the size of one font pixel in window pixels.
    pub fn text(&mut self, text: &str, anchor: Anchor, scale: usize) {
        let scale = scale.max(1);
        let lines: Vec<&str> = text.lines().collect();
        let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        let padding = 3 * scale;
        let panel_w = longest * CELL_WIDTH * scale + 2 * padding;
        let panel_h = lines.len() * CELL_HEIGHT * scale + 2 * padding;
        let (panel_x, panel_y) = match anchor {
            Anchor::Centre => {
                (self.width.saturating_sub(panel_w) / 2, self.height.saturating_sub(panel_h) / 2)
            }
            Anchor::BottomLeft => (padding, self.height.saturating_sub(panel_h + padding)),
        };
        self.fill_rect(panel_x, panel_y, panel_w, panel_h, BACKDROP);

        for (row, line) in lines.iter().enumerate() {
            // Centre each line within the panel.
            let indent = (longest - line.chars().count()) * CELL_WIDTH * scale / 2;
            let line_y = panel_y + padding + row * CELL_HEIGHT * scale + scale;
            for (column, c) in line.chars().enumerate() {
                let char_x = panel_x + padding + indent + column * CELL_WIDTH * scale;
                for (glyph_y, bits) in glyph(c).into_iter().enumerate() {
                    for glyph_x in 0..GLYPH_WIDTH {
                        if bits & (0b10000 >> glyph_x) != 0 {
                            self.fill_rect(
                                char_x + glyph_x * scale,
                                line_y + glyph_y * scale,
                                scale,
                                scale,
                                TEXT,
                            );
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(pixels: &[u32]) -> usize {
        pixels.iter().filter(|&&p| p == TEXT).count()
    }

    #[test]
    fn draws_scaled_glyphs_on_a_panel() {
        let mut pixels = vec![0u32; 200 * 100];
        Canvas { pixels: &mut pixels, width: 200, height: 100 }.text("I", Anchor::Centre, 1);
        // "I" has 3 + 5 + 3 = 11 lit pixels.
        assert_eq!(lit(&pixels), 11);
        assert!(pixels.contains(&BACKDROP));
        assert_eq!(pixels[0], 0);

        let mut big = vec![0u32; 200 * 100];
        Canvas { pixels: &mut big, width: 200, height: 100 }.text("i", Anchor::BottomLeft, 3);
        assert_eq!(lit(&big), 11 * 9);
    }

    #[test]
    fn text_that_does_not_fit_is_clipped_not_fatal() {
        let mut pixels = vec![0u32; 20 * 10];
        let mut canvas = Canvas { pixels: &mut pixels, width: 20, height: 10 };
        canvas.text("A VERY LONG LINE\nAND ANOTHER ONE", Anchor::Centre, 4);
        canvas.text("UNKNOWN: \u{1F600}", Anchor::BottomLeft, 2);
    }
}
