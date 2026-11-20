//! Cartridge mappers: the bank-switching hardware inside each cartridge.

mod mmc1;
mod mmc3;
mod simple;

use crate::cartridge::{Cartridge, Mirroring, RomError};

/// Cartridge hardware as seen from the CPU and PPU buses.
pub trait Mapper {
    /// Reads from the CPU address space (`$4020-$FFFF`). `None` means nothing drives
    /// the bus and the CPU sees open bus.
    fn cpu_read(&mut self, addr: u16) -> Option<u8>;
    /// Writes to the CPU address space (`$4020-$FFFF`).
    fn cpu_write(&mut self, addr: u16, value: u8);
    /// Reads pattern data (`$0000-$1FFF` on the PPU bus).
    fn chr_read(&mut self, addr: u16) -> u8;
    /// Writes pattern data; ignored unless the cartridge has CHR RAM.
    fn chr_write(&mut self, addr: u16, value: u8);
    /// Current nametable arrangement.
    fn mirroring(&self) -> Mirroring;
    /// Called once per rendered scanline, for mappers with a scanline counter.
    fn scanline(&mut self) {}
    /// Level of the cartridge IRQ line.
    fn irq(&self) -> bool {
        false
    }
    /// Battery-backed RAM contents, if the cartridge has any.
    fn save_ram(&self) -> Option<&[u8]> {
        None
    }
    /// Restores battery-backed RAM from a previous [`Mapper::save_ram`] snapshot.
    fn load_save_ram(&mut self, _data: &[u8]) {}
}

/// Mapper numbers implemented by this crate.
pub const SUPPORTED_MAPPERS: [u16; 6] = [0, 1, 2, 3, 4, 7];

/// Builds the mapper a cartridge asks for.
pub fn create(cart: Cartridge) -> Result<Box<dyn Mapper>, RomError> {
    Ok(match cart.mapper {
        0 => Box::new(simple::Nrom::new(cart)),
        1 => Box::new(mmc1::Mmc1::new(cart)),
        2 => Box::new(simple::Uxrom::new(cart)),
        3 => Box::new(simple::Cnrom::new(cart)),
        4 => Box::new(mmc3::Mmc3::new(cart)),
        7 => Box::new(simple::Axrom::new(cart)),
        other => return Err(RomError::UnsupportedMapper(other)),
    })
}

/// 8 KiB of PRG RAM at `$6000-$7FFF`, optionally battery backed.
struct PrgRam {
    data: Vec<u8>,
    battery: bool,
}

impl PrgRam {
    fn new(battery: bool) -> Self {
        PrgRam { data: vec![0; 0x2000], battery }
    }

    fn read(&self, addr: u16) -> u8 {
        self.data[addr as usize & 0x1FFF]
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.data[addr as usize & 0x1FFF] = value;
    }

    fn save(&self) -> Option<&[u8]> {
        self.battery.then_some(&self.data[..])
    }

    fn load(&mut self, data: &[u8]) {
        let len = data.len().min(self.data.len());
        self.data[..len].copy_from_slice(&data[..len]);
    }
}

/// Reads from a ROM/RAM array as a sequence of `bank_size` banks, wrapping bank
/// numbers that exceed the array (as the unconnected address lines would).
fn banked(data: &[u8], bank: usize, bank_size: usize, addr: u16) -> usize {
    (bank * bank_size + (addr as usize & (bank_size - 1))) % data.len()
}
