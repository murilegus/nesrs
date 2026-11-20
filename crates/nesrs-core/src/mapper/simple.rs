//! Discrete-logic mappers: NROM (0), UxROM (2), CNROM (3) and AxROM (7).

use super::{Mapper, PrgRam, banked};
use crate::cartridge::{Cartridge, Mirroring};

/// Mapper 0: no bank switching at all.
pub struct Nrom {
    cart: Cartridge,
    ram: PrgRam,
}

impl Nrom {
    pub fn new(cart: Cartridge) -> Self {
        let ram = PrgRam::new(cart.battery);
        Nrom { cart, ram }
    }
}

impl Mapper for Nrom {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.ram.read(addr)),
            0x8000..=0xFFFF => {
                Some(self.cart.prg_rom[(addr as usize - 0x8000) % self.cart.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if let 0x6000..=0x7FFF = addr {
            self.ram.write(addr, value);
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        self.cart.chr[banked(&self.cart.chr, 0, 0x2000, addr)]
    }

    fn chr_write(&mut self, addr: u16, value: u8) {
        if self.cart.chr_is_ram {
            self.cart.chr[addr as usize & 0x1FFF] = value;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.cart.mirroring
    }

    fn save_ram(&self) -> Option<&[u8]> {
        self.ram.save()
    }

    fn load_save_ram(&mut self, data: &[u8]) {
        self.ram.load(data);
    }
}

/// Mapper 2: switchable 16 KiB PRG bank at `$8000`, last bank fixed at `$C000`.
pub struct Uxrom {
    cart: Cartridge,
    bank: usize,
}

impl Uxrom {
    pub fn new(cart: Cartridge) -> Self {
        Uxrom { cart, bank: 0 }
    }
}

impl Mapper for Uxrom {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        let prg = &self.cart.prg_rom;
        match addr {
            0x8000..=0xBFFF => Some(prg[banked(prg, self.bank, 0x4000, addr)]),
            0xC000..=0xFFFF => Some(prg[banked(prg, prg.len() / 0x4000 - 1, 0x4000, addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            self.bank = value as usize;
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        self.cart.chr[banked(&self.cart.chr, 0, 0x2000, addr)]
    }

    fn chr_write(&mut self, addr: u16, value: u8) {
        if self.cart.chr_is_ram {
            self.cart.chr[addr as usize & 0x1FFF] = value;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.cart.mirroring
    }
}

/// Mapper 3: fixed PRG, switchable 8 KiB CHR bank.
pub struct Cnrom {
    cart: Cartridge,
    chr_bank: usize,
}

impl Cnrom {
    pub fn new(cart: Cartridge) -> Self {
        Cnrom { cart, chr_bank: 0 }
    }
}

impl Mapper for Cnrom {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        (addr >= 0x8000)
            .then(|| self.cart.prg_rom[(addr as usize - 0x8000) % self.cart.prg_rom.len()])
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            self.chr_bank = value as usize;
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        self.cart.chr[banked(&self.cart.chr, self.chr_bank, 0x2000, addr)]
    }

    fn chr_write(&mut self, addr: u16, value: u8) {
        if self.cart.chr_is_ram {
            let index = banked(&self.cart.chr, self.chr_bank, 0x2000, addr);
            self.cart.chr[index] = value;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.cart.mirroring
    }
}

/// Mapper 7: switchable 32 KiB PRG bank and single-screen mirroring select.
pub struct Axrom {
    cart: Cartridge,
    bank: usize,
    upper_screen: bool,
}

impl Axrom {
    pub fn new(cart: Cartridge) -> Self {
        Axrom { cart, bank: 0, upper_screen: false }
    }
}

impl Mapper for Axrom {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        let prg = &self.cart.prg_rom;
        (addr >= 0x8000).then(|| prg[banked(prg, self.bank, 0x8000, addr)])
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            self.bank = (value & 0x07) as usize;
            self.upper_screen = value & 0x10 != 0;
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        self.cart.chr[banked(&self.cart.chr, 0, 0x2000, addr)]
    }

    fn chr_write(&mut self, addr: u16, value: u8) {
        if self.cart.chr_is_ram {
            self.cart.chr[addr as usize & 0x1FFF] = value;
        }
    }

    fn mirroring(&self) -> Mirroring {
        if self.upper_screen { Mirroring::SingleUpper } else { Mirroring::SingleLower }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::test_util::ines;

    fn cart(mapper: u8, prg: u8, chr: u8) -> Cartridge {
        Cartridge::from_ines(&ines(mapper, prg, chr, 0)).unwrap()
    }

    #[test]
    fn nrom_mirrors_16k_prg() {
        let mut m = Nrom::new(cart(0, 1, 1));
        assert_eq!(m.cpu_read(0x8000), Some(0));
        assert_eq!(m.cpu_read(0xA000), Some(1));
        assert_eq!(m.cpu_read(0xC000), Some(0));
        assert_eq!(m.cpu_read(0x5000), None);
        m.cpu_write(0x6123, 0x77);
        assert_eq!(m.cpu_read(0x6123), Some(0x77));
        assert_eq!(m.save_ram(), None);
    }

    #[test]
    fn uxrom_switches_low_bank_and_fixes_last() {
        let mut m = Uxrom::new(cart(2, 8, 0));
        assert_eq!(m.cpu_read(0xC000), Some(14));
        m.cpu_write(0x8000, 3);
        assert_eq!(m.cpu_read(0x8000), Some(6));
        assert_eq!(m.cpu_read(0xA000), Some(7));
        m.chr_write(0x0123, 0x55);
        assert_eq!(m.chr_read(0x0123), 0x55);
    }

    #[test]
    fn cnrom_switches_chr() {
        let mut m = Cnrom::new(cart(3, 2, 4));
        m.cpu_write(0x8000, 2);
        assert_eq!(m.chr_read(0x0000), 16);
        assert_eq!(m.chr_read(0x1C00), 23);
        m.chr_write(0x0000, 0xFF); // CHR ROM is read-only
        assert_eq!(m.chr_read(0x0000), 16);
    }

    #[test]
    fn axrom_switches_32k_and_screen() {
        let mut m = Axrom::new(cart(7, 8, 0));
        m.cpu_write(0x8000, 0x12);
        assert_eq!(m.cpu_read(0x8000), Some(8));
        assert_eq!(m.cpu_read(0xE000), Some(11));
        assert_eq!(m.mirroring(), Mirroring::SingleUpper);
    }
}
