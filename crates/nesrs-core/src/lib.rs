//! `nesrs-core` — a platform-independent NES emulation core.
//!
//! The crate has no dependencies and performs no I/O: a frontend feeds it a ROM
//! image and controller state, and reads back video frames and audio samples.
//!
//! ```no_run
//! use nesrs_core::{Nes, button};
//!
//! let rom = std::fs::read("game.nes").unwrap();
//! let mut nes = Nes::new(&rom).unwrap();
//! let mut audio = Vec::new();
//! loop {
//!     nes.set_buttons(0, button::START);
//!     nes.run_frame();
//!     let _pixels: &[u32] = nes.framebuffer(); // 256x240, 0x00RRGGBB
//!     nes.drain_audio(&mut audio);
//! }
//! ```

pub mod apu;
mod bus;
pub mod cartridge;
pub mod controller;
pub mod cpu;
pub mod mapper;
mod nes;
pub mod ppu;
pub mod state;

pub use cartridge::RomError;
pub use controller::button;
pub use nes::{FRAME_RATE, Nes};
pub use ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};
pub use state::StateError;
