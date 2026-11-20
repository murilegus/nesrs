//! Mapper 4 (MMC3): 8 KiB PRG / 1 KiB CHR banking and a scanline IRQ counter.

use super::{Mapper, PrgRam, banked};
use crate::cartridge::{Cartridge, Mirroring};

pub struct Mmc3 {
    cart: Cartridge,
    ram: PrgRam,
    bank_select: u8,
    banks: [u8; 8],
    horizontal: bool,
    irq_latch: u8,
    irq_counter: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_line: bool,
}

impl Mmc3 {
    pub fn new(cart: Cartridge) -> Self {
        let ram = PrgRam::new(cart.battery);
        let horizontal = cart.mirroring == Mirroring::Horizontal;
        Mmc3 {
            cart,
            ram,
            bank_select: 0,
            banks: [0, 2, 4, 5, 6, 7, 0, 1],
            horizontal,
            irq_latch: 0,
            irq_counter: 0,
            irq_reload: false,
            irq_enabled: false,
            irq_line: false,
        }
    }

    fn prg_index(&self, addr: u16) -> usize {
        let prg = &self.cart.prg_rom;
        let last = prg.len() / 0x2000 - 1;
        let swapped = self.bank_select & 0x40 != 0;
        let bank = match ((addr - 0x8000) >> 13, swapped) {
            (0, false) | (2, true) => self.banks[6] as usize,
            (0, true) | (2, false) => last - 1,
            (1, _) => self.banks[7] as usize,
            _ => last,
        };
        banked(prg, bank, 0x2000, addr)
    }

    fn chr_index(&self, addr: u16) -> usize {
        // With the inversion bit set the two halves of the pattern table swap.
        let slot = (addr >> 10) as usize ^ if self.bank_select & 0x80 != 0 { 4 } else { 0 };
        let bank = match slot {
            0 => self.banks[0] & 0xFE,
            1 => self.banks[0] | 0x01,
            2 => self.banks[1] & 0xFE,
            3 => self.banks[1] | 0x01,
            n => self.banks[n - 2],
        };
        banked(&self.cart.chr, bank as usize, 0x400, addr)
    }
}

impl Mapper for Mmc3 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.ram.read(addr)),
            0x8000..=0xFFFF => Some(self.cart.prg_rom[self.prg_index(addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        let odd = addr & 1 != 0;
        match (addr, odd) {
            (0x6000..=0x7FFF, _) => self.ram.write(addr, value),
            (0x8000..=0x9FFF, false) => self.bank_select = value,
            (0x8000..=0x9FFF, true) => self.banks[(self.bank_select & 7) as usize] = value,
            (0xA000..=0xBFFF, false) => self.horizontal = value & 1 != 0,
            (0xA000..=0xBFFF, true) => {} // PRG RAM protect: not emulated
            (0xC000..=0xDFFF, false) => self.irq_latch = value,
            (0xC000..=0xDFFF, true) => self.irq_reload = true,
            (0xE000..=0xFFFF, false) => {
                self.irq_enabled = false;
                self.irq_line = false;
            }
            (0xE000..=0xFFFF, true) => self.irq_enabled = true,
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        self.cart.chr[self.chr_index(addr)]
    }

    fn chr_write(&mut self, addr: u16, value: u8) {
        if self.cart.chr_is_ram {
            let index = self.chr_index(addr);
            self.cart.chr[index] = value;
        }
    }

    fn mirroring(&self) -> Mirroring {
        if self.cart.mirroring == Mirroring::FourScreen {
            Mirroring::FourScreen
        } else if self.horizontal {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn scanline(&mut self) {
        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter -= 1;
        }
        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_line = true;
        }
    }

    fn irq(&self) -> bool {
        self.irq_line
    }

    fn save_ram(&self) -> Option<&[u8]> {
        self.ram.save()
    }

    fn load_save_ram(&mut self, data: &[u8]) {
        self.ram.load(data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::test_util::ines;

    fn mmc3() -> Mmc3 {
        Mmc3::new(Cartridge::from_ines(&ines(4, 8, 4, 0)).unwrap())
    }

    #[test]
    fn prg_banking_in_both_modes() {
        let mut m = mmc3();
        m.cpu_write(0x8000, 6);
        m.cpu_write(0x8001, 5);
        m.cpu_write(0x8000, 7);
        m.cpu_write(0x8001, 9);
        assert_eq!(m.cpu_read(0x8000), Some(5));
        assert_eq!(m.cpu_read(0xA000), Some(9));
        assert_eq!(m.cpu_read(0xC000), Some(14));
        assert_eq!(m.cpu_read(0xE000), Some(15));
        m.cpu_write(0x8000, 0x46);
        assert_eq!(m.cpu_read(0x8000), Some(14));
        assert_eq!(m.cpu_read(0xC000), Some(5));
    }

    #[test]
    fn chr_banking_and_inversion() {
        let mut m = mmc3();
        m.cpu_write(0x8000, 0);
        m.cpu_write(0x8001, 9); // 2 KiB bank: low bit dropped
        m.cpu_write(0x8000, 2);
        m.cpu_write(0x8001, 20);
        assert_eq!(m.chr_read(0x0000), 8);
        assert_eq!(m.chr_read(0x0400), 9);
        assert_eq!(m.chr_read(0x1000), 20);
        m.cpu_write(0x8000, 0x80);
        assert_eq!(m.chr_read(0x1000), 8);
        assert_eq!(m.chr_read(0x0000), 20);
    }

    #[test]
    fn irq_fires_after_latch_plus_one_scanlines() {
        let mut m = mmc3();
        m.cpu_write(0xC000, 3);
        m.cpu_write(0xC001, 0);
        m.cpu_write(0xE001, 0);
        for _ in 0..3 {
            m.scanline();
            assert!(!m.irq());
        }
        m.scanline();
        assert!(m.irq());
        m.cpu_write(0xE000, 0);
        assert!(!m.irq());
    }

    #[test]
    fn mirroring_register() {
        let mut m = mmc3();
        m.cpu_write(0xA000, 0);
        assert_eq!(m.mirroring(), Mirroring::Vertical);
        m.cpu_write(0xA000, 1);
        assert_eq!(m.mirroring(), Mirroring::Horizontal);
    }
}
