//! The complete console.

use crate::bus::SystemBus;
use crate::cartridge::{Cartridge, RomError};
use crate::cpu::Cpu;
use crate::mapper;
use crate::state::{self, State, StateError};

/// NTSC frame rate in frames per second.
pub const FRAME_RATE: f64 = 60.0988;

/// A NES with a cartridge inserted.
pub struct Nes {
    cpu: Cpu,
    bus: SystemBus,
    /// Identifies the ROM, so save states are only loaded into the game they came from.
    rom_hash: u64,
}

impl Nes {
    /// Powers on a console with the given iNES ROM image inserted.
    pub fn new(rom: &[u8]) -> Result<Self, RomError> {
        let cartridge = Cartridge::from_ines(rom)?;
        let mut nes = Nes {
            cpu: Cpu::new(),
            bus: SystemBus::new(mapper::create(cartridge)?),
            rom_hash: state::hash(rom),
        };
        nes.cpu.reset(&mut nes.bus);
        Ok(nes)
    }

    /// Presses the reset button. RAM and cartridge state are preserved.
    pub fn reset(&mut self) {
        self.bus.ppu.reset();
        self.bus.apu.reset();
        self.bus.oam_dma = None;
        self.bus.stall = 0;
        self.cpu.reset(&mut self.bus);
    }

    /// Executes one CPU instruction (or interrupt sequence, or pending DMA).
    pub fn step(&mut self) {
        if let Some(page) = self.bus.oam_dma.take() {
            self.bus.run_oam_dma(page);
        }
        while self.bus.stall > 0 {
            self.bus.stall -= 1;
            self.bus.tick();
        }
        self.cpu.step(&mut self.bus);
    }

    /// Runs until the PPU finishes the current frame.
    pub fn run_frame(&mut self) {
        let frame = self.bus.ppu.frame();
        while self.bus.ppu.frame() == frame {
            self.step();
        }
    }

    /// The last completed picture: 256x240 pixels, row-major, `0x00RRGGBB`.
    pub fn framebuffer(&self) -> &[u32] {
        self.bus.ppu.framebuffer()
    }

    /// Sets the held buttons of controller `port` (0 or 1) as a combination of
    /// [`crate::button`] masks.
    pub fn set_buttons(&mut self, port: usize, buttons: u8) {
        self.bus.controllers[port].set_buttons(buttons);
    }

    /// Sets the audio output rate in Hz (48 000 by default).
    pub fn set_sample_rate(&mut self, hz: u32) {
        self.bus.apu.set_sample_rate(hz);
    }

    /// Appends the mono audio samples generated since the last call to `out`.
    /// Samples are roughly in the range -1.0 to 1.0.
    pub fn drain_audio(&mut self, out: &mut Vec<f32>) {
        self.bus.apu.drain_samples(out);
    }

    /// Battery-backed cartridge RAM, if the cartridge has a battery.
    pub fn save_ram(&self) -> Option<&[u8]> {
        self.bus.mapper.save_ram()
    }

    /// Restores battery-backed RAM saved by [`Nes::save_ram`].
    pub fn load_save_ram(&mut self, data: &[u8]) {
        self.bus.mapper.load_save_ram(data);
    }

    /// Captures the complete state of the console. Takes `&mut self` only because
    /// saving and loading share one code path; nothing is modified.
    pub fn save_state(&mut self) -> Vec<u8> {
        let mut state = State::saver();
        self.sync(&mut state);
        state.finish(self.rom_hash)
    }

    /// Restores a state captured by [`Nes::save_state`] for the same ROM. On error
    /// the console is left untouched.
    pub fn load_state(&mut self, data: &[u8]) -> Result<(), StateError> {
        let body = State::open(data, self.rom_hash)?;
        // The layout is fixed for a given ROM, so a different length means the
        // state cannot be ours even though its checksum is valid.
        let current = self.save_state();
        if data.len() != current.len() {
            return Err(StateError::Corrupt);
        }
        let mut state = State::loader(body);
        self.sync(&mut state);
        debug_assert!(state.fully_consumed());
        Ok(())
    }

    fn sync(&mut self, state: &mut State) {
        self.cpu.sync(state);
        self.bus.sync(state);
    }

    // --- Debugging aids ----------------------------------------------------

    /// CPU registers.
    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    /// Mutable CPU registers, for test harnesses that need to redirect execution.
    pub fn cpu_mut(&mut self) -> &mut Cpu {
        &mut self.cpu
    }

    /// The console's 2 KiB of internal RAM.
    pub fn ram(&self) -> &[u8] {
        &self.bus.ram
    }

    /// CPU cycles elapsed since power-on.
    pub fn cycles(&self) -> u64 {
        self.bus.cycles
    }

    /// PPU beam position as `(scanline, dot)`.
    pub fn ppu_position(&self) -> (u16, u16) {
        self.bus.ppu.position()
    }
}
