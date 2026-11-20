//! WebAssembly bindings: a thin wrapper that exposes the emulator core to
//! JavaScript. All browser work (canvas, audio, input, storage) lives in the
//! page's own scripts under `www/`.

use nesrs_core::{Nes, SCREEN_HEIGHT, SCREEN_WIDTH};
use wasm_bindgen::prelude::*;

/// A running console, as seen from JavaScript.
#[wasm_bindgen]
pub struct Emulator {
    nes: Nes,
    rgba: Vec<u8>,
    audio: Vec<f32>,
}

#[wasm_bindgen]
impl Emulator {
    /// Powers on a console with the given iNES ROM image.
    #[wasm_bindgen(constructor)]
    pub fn new(rom: &[u8]) -> Result<Emulator, JsError> {
        let nes = Nes::new(rom).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Emulator { nes, rgba: vec![0; SCREEN_WIDTH * SCREEN_HEIGHT * 4], audio: Vec::new() })
    }

    /// Picture width in pixels.
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> usize {
        SCREEN_WIDTH
    }

    /// Picture height in pixels.
    #[wasm_bindgen(getter)]
    pub fn height(&self) -> usize {
        SCREEN_HEIGHT
    }

    /// Emulates one video frame.
    pub fn run_frame(&mut self) {
        self.nes.run_frame();
    }

    /// The last completed picture as RGBA bytes, ready for `ImageData`.
    pub fn frame(&mut self) -> Vec<u8> {
        to_rgba(self.nes.framebuffer(), &mut self.rgba);
        self.rgba.clone()
    }

    /// Sets the held buttons of controller `port` (0 or 1): a bit mask of
    /// A=1, B=2, Select=4, Start=8, Up=16, Down=32, Left=64, Right=128.
    pub fn set_buttons(&mut self, port: usize, buttons: u8) {
        if port < 2 {
            self.nes.set_buttons(port, buttons);
        }
    }

    /// Presses the reset button.
    pub fn reset(&mut self) {
        self.nes.reset();
    }

    /// Sets the audio output rate in Hz.
    pub fn set_sample_rate(&mut self, hz: u32) {
        self.nes.set_sample_rate(hz);
    }

    /// Mono audio samples generated since the last call.
    pub fn take_audio(&mut self) -> Vec<f32> {
        self.audio.clear();
        self.nes.drain_audio(&mut self.audio);
        self.audio.clone()
    }

    /// Captures the complete state of the console.
    pub fn save_state(&mut self) -> Vec<u8> {
        self.nes.save_state()
    }

    /// Restores a state captured by `save_state` for the same ROM.
    pub fn load_state(&mut self, data: &[u8]) -> Result<(), JsError> {
        self.nes.load_state(data).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Battery-backed cartridge RAM, or `undefined` if the game has no battery.
    pub fn save_ram(&self) -> Option<Vec<u8>> {
        self.nes.save_ram().map(<[u8]>::to_vec)
    }

    /// Restores battery-backed RAM.
    pub fn load_save_ram(&mut self, data: &[u8]) {
        self.nes.load_save_ram(data);
    }
}

/// Converts `0x00RRGGBB` pixels to opaque RGBA bytes.
fn to_rgba(frame: &[u32], out: &mut [u8]) {
    for (pixel, rgba) in frame.iter().zip(out.chunks_exact_mut(4)) {
        let [_, r, g, b] = pixel.to_be_bytes();
        rgba.copy_from_slice(&[r, g, b, 0xFF]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_convert_to_opaque_rgba() {
        let mut out = [0u8; 8];
        to_rgba(&[0x00123456, 0x00FFFEFD], &mut out);
        assert_eq!(out, [0x12, 0x34, 0x56, 0xFF, 0xFF, 0xFE, 0xFD, 0xFF]);
    }
}
