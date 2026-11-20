//! Scaling the NES picture into a window, and saving it to disk.

use std::io::Write;
use std::path::Path;

use nesrs_core::{SCREEN_HEIGHT, SCREEN_WIDTH};

/// Draws the 256x240 `frame` into a `width` x `height` buffer, as large as fits
/// while keeping the aspect ratio, centred on a black background. Whole-number
/// scales are preferred so pixels stay square and even.
pub fn blit(frame: &[u32], target: &mut [u32], width: usize, height: usize) {
    target.fill(0);
    if width == 0 || height == 0 {
        return;
    }
    let (out_w, out_h) = fit(width, height);
    let left = (width - out_w) / 2;
    let top = (height - out_h) / 2;
    let columns: Vec<usize> = (0..out_w).map(|x| x * SCREEN_WIDTH / out_w).collect();
    for y in 0..out_h {
        let source = &frame[(y * SCREEN_HEIGHT / out_h) * SCREEN_WIDTH..][..SCREEN_WIDTH];
        let row = &mut target[(top + y) * width + left..][..out_w];
        for (pixel, &column) in row.iter_mut().zip(&columns) {
            *pixel = source[column];
        }
    }
}

/// Size of the picture inside a window of the given size.
fn fit(width: usize, height: usize) -> (usize, usize) {
    let scale = (width / SCREEN_WIDTH).min(height / SCREEN_HEIGHT);
    if scale >= 1 {
        return (SCREEN_WIDTH * scale, SCREEN_HEIGHT * scale);
    }
    // Smaller than the native resolution: shrink to fit.
    if width * SCREEN_HEIGHT <= height * SCREEN_WIDTH {
        (width, (width * SCREEN_HEIGHT / SCREEN_WIDTH).max(1))
    } else {
        ((height * SCREEN_WIDTH / SCREEN_HEIGHT).max(1), height)
    }
}

/// Writes a frame as a binary PPM image.
pub fn write_ppm(path: &Path, frame: &[u32]) -> std::io::Result<()> {
    let mut data = format!("P6\n{SCREEN_WIDTH} {SCREEN_HEIGHT}\n255\n").into_bytes();
    for pixel in frame {
        data.extend_from_slice(&pixel.to_be_bytes()[1..]);
    }
    std::fs::File::create(path)?.write_all(&data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_prefers_integer_scales() {
        assert_eq!(fit(256, 240), (256, 240));
        assert_eq!(fit(800, 600), (512, 480));
        assert_eq!(fit(1920, 1080), (1024, 960));
        assert_eq!(fit(128, 240), (128, 120));
        assert_eq!(fit(256, 60), (64, 60));
    }

    #[test]
    fn blit_centres_and_scales() {
        let mut frame = vec![0x111111; SCREEN_WIDTH * SCREEN_HEIGHT];
        frame[0] = 0xFF0000;
        frame[SCREEN_WIDTH * SCREEN_HEIGHT - 1] = 0x0000FF;
        let (w, h) = (600, 500);
        let mut target = vec![0xABCDEF; w * h];
        blit(&frame, &mut target, w, h);
        // Scale 2 gives a 512x480 picture with a 44x10 border.
        assert_eq!(target[0], 0);
        assert_eq!(target[10 * w + 44], 0xFF0000);
        assert_eq!(target[11 * w + 45], 0xFF0000);
        assert_eq!(target[10 * w + 46], 0x111111);
        assert_eq!(target[489 * w + 555], 0x0000FF);
        assert_eq!(target[490 * w + 555], 0);
    }
}
