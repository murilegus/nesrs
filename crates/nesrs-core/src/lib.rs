//! `nesrs-core` — a platform-independent NES emulation core.
//!
//! The crate has no dependencies and performs no I/O: a frontend feeds it a ROM
//! image and controller state, and reads back video frames and audio samples.

pub mod cartridge;
pub mod cpu;
pub mod mapper;
pub mod ppu;
