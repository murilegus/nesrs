//! iNES / NES 2.0 ROM image parsing.

use std::fmt;

/// Nametable arrangement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    SingleLower,
    SingleUpper,
    FourScreen,
}

/// Reasons a ROM image can be rejected.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RomError {
    /// The file does not start with the `NES\x1A` signature.
    BadMagic,
    /// The file is shorter than its header says it should be.
    Truncated,
    /// The header declares no PRG ROM at all.
    NoPrgRom,
    /// The cartridge uses a mapper this emulator does not implement yet.
    UnsupportedMapper(u16),
}

impl fmt::Display for RomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RomError::BadMagic => write!(f, "not an iNES ROM image (bad signature)"),
            RomError::Truncated => write!(f, "ROM image is truncated"),
            RomError::NoPrgRom => write!(f, "ROM image contains no PRG ROM"),
            RomError::UnsupportedMapper(id) => write!(f, "mapper {id} is not supported"),
        }
    }
}

impl std::error::Error for RomError {}

const PRG_UNIT: usize = 16 * 1024;
const CHR_UNIT: usize = 8 * 1024;

/// The contents of a cartridge, as described by an iNES file.
#[derive(Clone, Debug)]
pub struct Cartridge {
    pub prg_rom: Vec<u8>,
    /// CHR ROM, or 8 KiB of zeroed CHR RAM when the cartridge has no CHR ROM.
    pub chr: Vec<u8>,
    pub chr_is_ram: bool,
    pub mapper: u16,
    pub mirroring: Mirroring,
    /// Whether PRG RAM is battery backed and worth saving to disk.
    pub battery: bool,
}

impl Cartridge {
    /// Parses an iNES or NES 2.0 image.
    pub fn from_ines(data: &[u8]) -> Result<Self, RomError> {
        if data.len() < 16 || &data[0..4] != b"NES\x1A" {
            return Err(RomError::BadMagic);
        }
        let flags6 = data[6];
        let flags7 = data[7];
        let nes2 = flags7 & 0x0C == 0x08;

        let mut mapper = (flags6 >> 4) as u16;
        if nes2 {
            mapper |= (flags7 & 0xF0) as u16 | ((data[8] & 0x0F) as u16) << 8;
        } else if data[12..16].iter().all(|&b| b == 0) {
            // Old dumping tools left junk such as "DiskDude!" in bytes 7-15; the high
            // mapper nibble can only be trusted when the tail of the header is clean.
            mapper |= (flags7 & 0xF0) as u16;
        }

        let mut prg_units = data[4] as usize;
        let mut chr_units = data[5] as usize;
        if nes2 {
            prg_units |= ((data[9] & 0x0F) as usize) << 8;
            chr_units |= ((data[9] >> 4) as usize) << 8;
        }
        if prg_units == 0 {
            return Err(RomError::NoPrgRom);
        }

        let trainer = if flags6 & 0x04 != 0 { 512 } else { 0 };
        let prg_start = 16 + trainer;
        let chr_start = prg_start + prg_units * PRG_UNIT;
        let chr_end = chr_start + chr_units * CHR_UNIT;
        if data.len() < chr_end {
            return Err(RomError::Truncated);
        }

        let mirroring = if flags6 & 0x08 != 0 {
            Mirroring::FourScreen
        } else if flags6 & 0x01 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        let chr_is_ram = chr_units == 0;
        let chr = if chr_is_ram { vec![0; CHR_UNIT] } else { data[chr_start..chr_end].to_vec() };

        Ok(Cartridge {
            prg_rom: data[prg_start..chr_start].to_vec(),
            chr,
            chr_is_ram,
            mapper,
            mirroring,
            battery: flags6 & 0x02 != 0,
        })
    }
}

#[cfg(test)]
pub(crate) mod test_util {
    /// Builds an iNES image. PRG and CHR sizes are in 16 KiB / 8 KiB units, and each
    /// byte of PRG is filled with the index of the 8 KiB bank it belongs to.
    pub fn ines(mapper: u8, prg_units: u8, chr_units: u8, flags6_low: u8) -> Vec<u8> {
        let mut rom = vec![b'N', b'E', b'S', 0x1A, prg_units, chr_units];
        rom.push((mapper << 4) | (flags6_low & 0x0F));
        rom.push(mapper & 0xF0);
        rom.extend_from_slice(&[0; 8]);
        for bank in 0..prg_units as usize * 2 {
            rom.extend(std::iter::repeat_n(bank as u8, 0x2000));
        }
        // CHR bytes hold the index of their 1 KiB bank.
        for bank in 0..chr_units as usize * 8 {
            rom.extend(std::iter::repeat_n(bank as u8, 0x400));
        }
        rom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_header_fields() {
        let rom = test_util::ines(4, 2, 1, 0x03);
        let cart = Cartridge::from_ines(&rom).unwrap();
        assert_eq!(cart.mapper, 4);
        assert_eq!(cart.prg_rom.len(), 32 * 1024);
        assert_eq!(cart.chr.len(), 8 * 1024);
        assert!(!cart.chr_is_ram);
        assert!(cart.battery);
        assert_eq!(cart.mirroring, Mirroring::Vertical);
    }

    #[test]
    fn allocates_chr_ram_when_there_is_no_chr_rom() {
        let cart = Cartridge::from_ines(&test_util::ines(0, 1, 0, 0)).unwrap();
        assert!(cart.chr_is_ram);
        assert_eq!(cart.chr.len(), 8 * 1024);
        assert_eq!(cart.mirroring, Mirroring::Horizontal);
    }

    #[test]
    fn skips_the_trainer() {
        let mut rom = test_util::ines(0, 1, 0, 0x04);
        rom.splice(16..16, std::iter::repeat_n(0xEE, 512));
        let cart = Cartridge::from_ines(&rom).unwrap();
        assert_eq!(cart.prg_rom[0], 0);
        assert_eq!(cart.prg_rom.len(), 16 * 1024);
    }

    #[test]
    fn ignores_high_mapper_nibble_in_dirty_headers() {
        let mut rom = test_util::ines(1, 1, 0, 0);
        rom[7] = 0x40;
        rom[12..16].copy_from_slice(b"ude!");
        assert_eq!(Cartridge::from_ines(&rom).unwrap().mapper, 1);
    }

    #[test]
    fn rejects_bad_images() {
        assert_eq!(Cartridge::from_ines(b"nope").unwrap_err(), RomError::BadMagic);
        assert_eq!(Cartridge::from_ines(&[0u8; 32]).unwrap_err(), RomError::BadMagic);
        let mut rom = test_util::ines(0, 1, 1, 0);
        rom.truncate(1000);
        assert_eq!(Cartridge::from_ines(&rom).unwrap_err(), RomError::Truncated);
        let rom = test_util::ines(0, 0, 0, 0);
        assert_eq!(Cartridge::from_ines(&rom).unwrap_err(), RomError::NoPrgRom);
    }
}
