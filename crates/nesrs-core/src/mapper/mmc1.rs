//! Mapper 1 (MMC1): serial-loaded registers, 16/32 KiB PRG and 4/8 KiB CHR banking.

use super::{Mapper, PrgRam, banked};
use crate::cartridge::{Cartridge, Mirroring};

pub struct Mmc1 {
    cart: Cartridge,
    ram: PrgRam,
    /// 5-bit shift register; the marker bit reaching bit 0 signals the fifth write.
    shift: u8,
    control: u8,
    chr0: u8,
    chr1: u8,
    prg: u8,
}

impl Mmc1 {
    pub fn new(cart: Cartridge) -> Self {
        let ram = PrgRam::new(cart.battery);
        Mmc1 { cart, ram, shift: 0x10, control: 0x0C, chr0: 0, chr1: 0, prg: 0 }
    }

    fn prg_index(&self, addr: u16) -> usize {
        let prg = &self.cart.prg_rom;
        // 512 KiB boards (SUROM) route a CHR register bit to the top PRG address line.
        let outer = if prg.len() > 0x40000 { (self.chr0 & 0x10) as usize } else { 0 };
        let bank = (self.prg & 0x0F) as usize;
        let high = addr >= 0xC000;
        let bank16 = match (self.control >> 2) & 3 {
            0 | 1 => (bank & !1) | high as usize,
            2 => {
                if high {
                    bank
                } else {
                    0
                }
            }
            _ => {
                if high {
                    0x0F
                } else {
                    bank
                }
            }
        };
        // Wrap within the 256 KiB the inner bank bits can reach, then add the outer bank.
        let inner_banks = (prg.len() / 0x4000).min(16);
        banked(prg, outer | (bank16 % inner_banks), 0x4000, addr)
    }

    fn chr_index(&self, addr: u16) -> usize {
        let bank4 = if self.control & 0x10 == 0 {
            (self.chr0 & 0x1E) as usize | (addr >> 12) as usize
        } else if addr < 0x1000 {
            self.chr0 as usize
        } else {
            self.chr1 as usize
        };
        banked(&self.cart.chr, bank4, 0x1000, addr)
    }
}

impl Mapper for Mmc1 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.ram.read(addr)),
            0x8000..=0xFFFF => Some(self.cart.prg_rom[self.prg_index(addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF => self.ram.write(addr, value),
            0x8000..=0xFFFF => {
                if value & 0x80 != 0 {
                    self.shift = 0x10;
                    self.control |= 0x0C;
                    return;
                }
                let complete = self.shift & 1 != 0;
                self.shift = self.shift >> 1 | (value & 1) << 4;
                if complete {
                    match (addr >> 13) & 3 {
                        0 => self.control = self.shift,
                        1 => self.chr0 = self.shift,
                        2 => self.chr1 = self.shift,
                        _ => self.prg = self.shift,
                    }
                    self.shift = 0x10;
                }
            }
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
        match self.control & 3 {
            0 => Mirroring::SingleLower,
            1 => Mirroring::SingleUpper,
            2 => Mirroring::Vertical,
            _ => Mirroring::Horizontal,
        }
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

    fn serial_write(m: &mut Mmc1, addr: u16, value: u8) {
        for bit in 0..5 {
            m.cpu_write(addr, (value >> bit) & 1);
        }
    }

    #[test]
    fn powers_on_with_last_bank_fixed_high() {
        let mut m = Mmc1::new(Cartridge::from_ines(&ines(1, 8, 2, 0)).unwrap());
        assert_eq!(m.cpu_read(0x8000), Some(0));
        assert_eq!(m.cpu_read(0xE000), Some(15));
    }

    #[test]
    fn switches_prg_in_each_mode() {
        let mut m = Mmc1::new(Cartridge::from_ines(&ines(1, 8, 2, 0)).unwrap());
        serial_write(&mut m, 0xE000, 3);
        assert_eq!(m.cpu_read(0x8000), Some(6));
        assert_eq!(m.cpu_read(0xC000), Some(14));
        // Mode 2: first bank fixed low, switchable high.
        serial_write(&mut m, 0x8000, 0x08);
        assert_eq!(m.cpu_read(0x8000), Some(0));
        assert_eq!(m.cpu_read(0xC000), Some(6));
        // Mode 0: 32 KiB bank, low bit ignored.
        serial_write(&mut m, 0x8000, 0x00);
        assert_eq!(m.cpu_read(0x8000), Some(4));
        assert_eq!(m.cpu_read(0xE000), Some(7));
    }

    #[test]
    fn reset_bit_aborts_a_partial_write() {
        let mut m = Mmc1::new(Cartridge::from_ines(&ines(1, 8, 2, 0)).unwrap());
        m.cpu_write(0xE000, 1);
        m.cpu_write(0xE000, 1);
        m.cpu_write(0xE000, 0x80);
        serial_write(&mut m, 0xE000, 2);
        assert_eq!(m.cpu_read(0x8000), Some(4));
    }

    #[test]
    fn switches_chr_and_mirroring() {
        let mut m = Mmc1::new(Cartridge::from_ines(&ines(1, 2, 2, 0)).unwrap());
        // 4 KiB CHR mode, horizontal mirroring.
        serial_write(&mut m, 0x8000, 0x1F);
        assert_eq!(m.mirroring(), Mirroring::Horizontal);
        serial_write(&mut m, 0xA000, 3);
        serial_write(&mut m, 0xC000, 1);
        assert_eq!(m.chr_read(0x0000), 12);
        assert_eq!(m.chr_read(0x1000), 4);
        // 8 KiB mode ignores the low bit of the first register.
        serial_write(&mut m, 0x8000, 0x0E);
        assert_eq!(m.mirroring(), Mirroring::Vertical);
        assert_eq!(m.chr_read(0x0000), 8);
        assert_eq!(m.chr_read(0x1000), 12);
    }

    #[test]
    fn battery_ram_round_trips() {
        let mut m = Mmc1::new(Cartridge::from_ines(&ines(1, 2, 2, 0x02)).unwrap());
        m.cpu_write(0x6010, 0xAB);
        let snapshot = m.save_ram().unwrap().to_vec();
        let mut fresh = Mmc1::new(Cartridge::from_ines(&ines(1, 2, 2, 0x02)).unwrap());
        fresh.load_save_ram(&snapshot);
        assert_eq!(fresh.cpu_read(0x6010), Some(0xAB));
    }
}
