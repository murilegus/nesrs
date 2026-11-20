//! Ricoh 2C02 picture processing unit (NTSC).
//!
//! The background is rendered dot by dot with the same shift registers and
//! "loopy" scroll registers as the real chip, so mid-frame scroll changes work.
//! Sprites are simplified: each scanline's sprites are evaluated and fetched in
//! one go at dot 257 into a per-line buffer.

use crate::cartridge::Mirroring;
use crate::mapper::Mapper;

pub const SCREEN_WIDTH: usize = 256;
pub const SCREEN_HEIGHT: usize = 240;

const DOTS_PER_LINE: u16 = 341;
const PRE_RENDER_LINE: u16 = 261;
const VBLANK_LINE: u16 = 241;

// PPUCTRL bits.
const CTRL_INCREMENT_32: u8 = 0x04;
const CTRL_SPRITE_TABLE: u8 = 0x08;
const CTRL_BG_TABLE: u8 = 0x10;
const CTRL_TALL_SPRITES: u8 = 0x20;
const CTRL_NMI: u8 = 0x80;
// PPUMASK bits.
const MASK_GRAYSCALE: u8 = 0x01;
const MASK_BG_LEFT: u8 = 0x02;
const MASK_SPRITES_LEFT: u8 = 0x04;
const MASK_BG: u8 = 0x08;
const MASK_SPRITES: u8 = 0x10;
// PPUSTATUS bits.
const STATUS_OVERFLOW: u8 = 0x20;
const STATUS_SPRITE0_HIT: u8 = 0x40;
const STATUS_VBLANK: u8 = 0x80;
// Sprite line buffer encoding: bits 0-1 pixel, 2-3 palette, then these flags.
const SPRITE_BEHIND_BG: u8 = 0x20;
const SPRITE_ZERO: u8 = 0x40;

/// The 2C02's 64 colours as `0x00RRGGBB`.
#[rustfmt::skip]
const BASE_PALETTE: [u32; 64] = [
    0x666666, 0x002A88, 0x1412A7, 0x3B00A4, 0x5C007E, 0x6E0040, 0x6C0600, 0x561D00,
    0x333500, 0x0B4800, 0x005200, 0x004F08, 0x00404D, 0x000000, 0x000000, 0x000000,
    0xADADAD, 0x155FD9, 0x4240FF, 0x7527FE, 0xA01ACC, 0xB71E7B, 0xB53120, 0x994E00,
    0x6B6D00, 0x388700, 0x0C9300, 0x008F32, 0x007C8D, 0x000000, 0x000000, 0x000000,
    0xFFFEFF, 0x64B0FF, 0x9290FF, 0xC676FF, 0xF36AFF, 0xFE6ECC, 0xFE8170, 0xEA9E22,
    0xBCBE00, 0x88D800, 0x5CE430, 0x45E082, 0x48CDDE, 0x4F4F4F, 0x000000, 0x000000,
    0xFFFEFF, 0xC0DFFF, 0xD3D2FF, 0xE8C8FF, 0xFBC2FF, 0xFEC4EA, 0xFECCC5, 0xF7D8A5,
    0xE4E594, 0xCFEF96, 0xBDF4AB, 0xB3F3CC, 0xB5EBF2, 0xB8B8B8, 0x000000, 0x000000,
];

/// Builds the palette for each of the 8 colour-emphasis settings. Emphasising a
/// channel works by dimming the other two.
fn build_palettes() -> Vec<[u32; 64]> {
    (0..8u32)
        .map(|emphasis| {
            let mut table = BASE_PALETTE;
            for colour in table.iter_mut() {
                let mut rgb = [(*colour >> 16) & 0xFF, (*colour >> 8) & 0xFF, *colour & 0xFF];
                for channel in 0..3 {
                    if emphasis & (1 << channel) == 0 {
                        continue;
                    }
                    for (other, value) in rgb.iter_mut().enumerate() {
                        if other != channel {
                            *value = *value * 3 / 4;
                        }
                    }
                }
                *colour = rgb[0] << 16 | rgb[1] << 8 | rgb[2];
            }
            table
        })
        .collect()
}

pub struct Ppu {
    ctrl: u8,
    mask: u8,
    status: u8,
    oam_addr: u8,
    /// Current VRAM address ("loopy v"): fine Y, nametable, coarse Y, coarse X.
    v: u16,
    /// Temporary VRAM address ("loopy t"): the scroll position to be applied.
    t: u16,
    /// Fine X scroll.
    fine_x: u8,
    /// Write toggle shared by PPUSCROLL and PPUADDR.
    w: bool,
    read_buffer: u8,
    /// Last value driven onto the register bus, visible in unimplemented bits.
    io_latch: u8,

    vram: Vec<u8>,
    palette: [u8; 32],
    oam: [u8; 256],

    scanline: u16,
    dot: u16,
    frame: u64,
    odd_frame: bool,
    suppress_vblank: bool,

    // Background pipeline.
    nt_latch: u8,
    at_latch: u8,
    pattern_lo_latch: u8,
    pattern_hi_latch: u8,
    pattern_shift_lo: u16,
    pattern_shift_hi: u16,
    attr_shift_lo: u16,
    attr_shift_hi: u16,

    /// Sprite pixels for the scanline being drawn.
    sprite_line: [u8; SCREEN_WIDTH],

    palettes: Vec<[u32; 64]>,
    framebuffer: Vec<u32>,
}

impl Default for Ppu {
    fn default() -> Self {
        Self::new()
    }
}

impl Ppu {
    pub fn new() -> Self {
        Ppu {
            ctrl: 0,
            mask: 0,
            status: 0,
            oam_addr: 0,
            v: 0,
            t: 0,
            fine_x: 0,
            w: false,
            read_buffer: 0,
            io_latch: 0,
            // Enough for four-screen cartridges; the rest only ever use half.
            vram: vec![0; 0x1000],
            palette: [0; 32],
            oam: [0; 256],
            scanline: 0,
            dot: 0,
            frame: 0,
            odd_frame: false,
            suppress_vblank: false,
            nt_latch: 0,
            at_latch: 0,
            pattern_lo_latch: 0,
            pattern_hi_latch: 0,
            pattern_shift_lo: 0,
            pattern_shift_hi: 0,
            attr_shift_lo: 0,
            attr_shift_hi: 0,
            sprite_line: [0; SCREEN_WIDTH],
            palettes: build_palettes(),
            framebuffer: vec![0; SCREEN_WIDTH * SCREEN_HEIGHT],
        }
    }

    /// Reset line: clears the registers but leaves memory intact.
    pub fn reset(&mut self) {
        self.ctrl = 0;
        self.mask = 0;
        self.w = false;
        self.read_buffer = 0;
        self.scanline = 0;
        self.dot = 0;
        self.odd_frame = false;
    }

    /// The most recently completed picture, 256x240 pixels of `0x00RRGGBB`.
    pub fn framebuffer(&self) -> &[u32] {
        &self.framebuffer
    }

    /// Number of frames completed so far (incremented when vblank starts).
    pub fn frame(&self) -> u64 {
        self.frame
    }

    /// Current beam position as `(scanline, dot)`.
    pub fn position(&self) -> (u16, u16) {
        (self.scanline, self.dot)
    }

    /// Level of the NMI output: asserted while in vblank with NMIs enabled.
    pub fn nmi_line(&self) -> bool {
        self.ctrl & CTRL_NMI != 0 && self.status & STATUS_VBLANK != 0
    }

    // --- CPU-facing registers ($2000-$2007) --------------------------------

    pub fn read_register(&mut self, reg: u16, mapper: &mut dyn Mapper) -> u8 {
        let value = match reg & 7 {
            2 => {
                let value = (self.status & 0xE0) | (self.io_latch & 0x1F);
                self.status &= !STATUS_VBLANK;
                self.w = false;
                // Reading one dot before the flag would be set stops it being set.
                if self.scanline == VBLANK_LINE && self.dot == 1 {
                    self.suppress_vblank = true;
                }
                value
            }
            4 => {
                let value = self.oam[self.oam_addr as usize];
                // The unused attribute bits do not exist in OAM.
                if self.oam_addr & 3 == 2 { value & 0xE3 } else { value }
            }
            7 => {
                let addr = self.v & 0x3FFF;
                let value = if addr < 0x3F00 {
                    let buffered = self.read_buffer;
                    self.read_buffer = self.mem_read(addr, mapper);
                    buffered
                } else {
                    // Palette reads are immediate; the buffer gets the nametable
                    // byte "underneath" the palette.
                    self.read_buffer = self.mem_read(addr - 0x1000, mapper);
                    (self.palette[palette_index(addr)] & 0x3F) | (self.io_latch & 0xC0)
                };
                self.increment_v();
                value
            }
            _ => self.io_latch,
        };
        self.io_latch = value;
        value
    }

    pub fn write_register(&mut self, reg: u16, value: u8, mapper: &mut dyn Mapper) {
        self.io_latch = value;
        match reg & 7 {
            0 => {
                self.ctrl = value;
                self.t = (self.t & 0xF3FF) | ((value as u16 & 3) << 10);
            }
            1 => self.mask = value,
            3 => self.oam_addr = value,
            4 => {
                self.oam[self.oam_addr as usize] = value;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            5 => {
                if !self.w {
                    self.t = (self.t & 0xFFE0) | (value as u16 >> 3);
                    self.fine_x = value & 7;
                } else {
                    self.t = (self.t & 0x8C1F)
                        | ((value as u16 & 0x07) << 12)
                        | ((value as u16 & 0xF8) << 2);
                }
                self.w = !self.w;
            }
            6 => {
                if !self.w {
                    self.t = (self.t & 0x00FF) | ((value as u16 & 0x3F) << 8);
                } else {
                    self.t = (self.t & 0xFF00) | value as u16;
                    self.v = self.t;
                }
                self.w = !self.w;
            }
            7 => {
                self.mem_write(self.v & 0x3FFF, value, mapper);
                self.increment_v();
            }
            _ => {}
        }
    }

    fn increment_v(&mut self) {
        let step = if self.ctrl & CTRL_INCREMENT_32 != 0 { 32 } else { 1 };
        self.v = self.v.wrapping_add(step) & 0x7FFF;
    }

    // --- PPU address space -------------------------------------------------

    fn nametable_index(&self, addr: u16, mirroring: Mirroring) -> usize {
        let table = (addr as usize >> 10) & 3;
        let physical = match mirroring {
            Mirroring::Horizontal => table >> 1,
            Mirroring::Vertical => table & 1,
            Mirroring::SingleLower => 0,
            Mirroring::SingleUpper => 1,
            Mirroring::FourScreen => table,
        };
        physical << 10 | (addr as usize & 0x3FF)
    }

    fn mem_read(&mut self, addr: u16, mapper: &mut dyn Mapper) -> u8 {
        match addr {
            0x0000..=0x1FFF => mapper.chr_read(addr),
            0x2000..=0x3EFF => self.vram[self.nametable_index(addr, mapper.mirroring())],
            _ => self.palette[palette_index(addr)],
        }
    }

    fn mem_write(&mut self, addr: u16, value: u8, mapper: &mut dyn Mapper) {
        match addr {
            0x0000..=0x1FFF => mapper.chr_write(addr, value),
            0x2000..=0x3EFF => {
                let index = self.nametable_index(addr, mapper.mirroring());
                self.vram[index] = value;
            }
            _ => self.palette[palette_index(addr)] = value,
        }
    }

    // --- Rendering ---------------------------------------------------------

    /// Advances the PPU by one dot (a third of a CPU cycle).
    pub fn tick(&mut self, mapper: &mut dyn Mapper) {
        let rendering = self.mask & (MASK_BG | MASK_SPRITES) != 0;
        let visible = self.scanline < SCREEN_HEIGHT as u16;
        let pre_render = self.scanline == PRE_RENDER_LINE;
        let dot = self.dot;

        if pre_render && dot == 1 {
            self.status &= !(STATUS_VBLANK | STATUS_SPRITE0_HIT | STATUS_OVERFLOW);
        }
        if self.scanline == VBLANK_LINE && dot == 1 {
            if !self.suppress_vblank {
                self.status |= STATUS_VBLANK;
            }
            self.suppress_vblank = false;
            self.frame += 1;
        }

        if rendering && (visible || pre_render) {
            self.background_pipeline(mapper);
            match dot {
                256 => self.increment_y(),
                // Copy the horizontal scroll bits from t.
                257 => {
                    self.v = (self.v & !0x041F) | (self.t & 0x041F);
                    if visible {
                        self.evaluate_sprites(mapper);
                    } else {
                        self.sprite_line = [0; SCREEN_WIDTH];
                    }
                }
                // Approximates the rise of PPU A12 during sprite fetches, which is
                // what clocks MMC3-style scanline counters.
                260 => mapper.scanline(),
                // Copy the vertical scroll bits from t, ready for the new frame.
                280..=304 if pre_render => self.v = (self.v & !0x7BE0) | (self.t & 0x7BE0),
                _ => {}
            }
        }

        if visible && (1..=256).contains(&dot) {
            self.render_pixel(rendering);
        }

        self.dot += 1;
        // With rendering on, odd frames are one dot shorter.
        if pre_render && self.dot == DOTS_PER_LINE - 1 && rendering && self.odd_frame {
            self.dot += 1;
        }
        if self.dot >= DOTS_PER_LINE {
            self.dot = 0;
            self.scanline += 1;
            if self.scanline > PRE_RENDER_LINE {
                self.scanline = 0;
                self.odd_frame = !self.odd_frame;
            }
        }
    }

    /// Shifts the background registers and performs this dot's tile fetch.
    fn background_pipeline(&mut self, mapper: &mut dyn Mapper) {
        let dot = self.dot;
        if (2..=257).contains(&dot) || (322..=337).contains(&dot) {
            self.pattern_shift_lo <<= 1;
            self.pattern_shift_hi <<= 1;
            self.attr_shift_lo <<= 1;
            self.attr_shift_hi <<= 1;
            // Every 8 dots the tile fetched during the previous 8 enters the low
            // byte of the shifters.
            if dot % 8 == 1 {
                self.pattern_shift_lo |= self.pattern_lo_latch as u16;
                self.pattern_shift_hi |= self.pattern_hi_latch as u16;
                self.attr_shift_lo |= if self.at_latch & 1 != 0 { 0xFF } else { 0 };
                self.attr_shift_hi |= if self.at_latch & 2 != 0 { 0xFF } else { 0 };
            }
        }
        if !((1..=256).contains(&dot) || (321..=336).contains(&dot)) {
            return;
        }
        let v = self.v;
        let pattern_addr =
            ((self.ctrl & CTRL_BG_TABLE) as u16) << 8 | (self.nt_latch as u16) << 4 | (v >> 12) & 7;
        match (dot - 1) % 8 {
            0 => self.nt_latch = self.mem_read(0x2000 | (v & 0x0FFF), mapper),
            2 => {
                let addr = 0x23C0 | (v & 0x0C00) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07);
                let shift = ((v >> 4) & 4) | (v & 2);
                self.at_latch = (self.mem_read(addr, mapper) >> shift) & 3;
            }
            4 => self.pattern_lo_latch = mapper.chr_read(pattern_addr),
            6 => self.pattern_hi_latch = mapper.chr_read(pattern_addr + 8),
            7 => self.increment_x(),
            _ => {}
        }
    }

    /// Moves v one tile to the right, wrapping into the next nametable.
    fn increment_x(&mut self) {
        if self.v & 0x001F == 31 {
            self.v = (self.v & !0x001F) ^ 0x0400;
        } else {
            self.v += 1;
        }
    }

    /// Moves v one pixel row down, wrapping into the next nametable.
    fn increment_y(&mut self) {
        if self.v & 0x7000 != 0x7000 {
            self.v += 0x1000;
            return;
        }
        self.v &= !0x7000;
        let mut coarse_y = (self.v & 0x03E0) >> 5;
        if coarse_y == 29 {
            coarse_y = 0;
            self.v ^= 0x0800;
        } else if coarse_y == 31 {
            // Rows 30 and 31 hold attribute data; wrapping from there does not
            // switch nametables.
            coarse_y = 0;
        } else {
            coarse_y += 1;
        }
        self.v = (self.v & !0x03E0) | (coarse_y << 5);
    }

    /// Finds the sprites on the next scanline and draws them into the line buffer.
    fn evaluate_sprites(&mut self, mapper: &mut dyn Mapper) {
        self.sprite_line = [0; SCREEN_WIDTH];
        let height: u16 = if self.ctrl & CTRL_TALL_SPRITES != 0 { 16 } else { 8 };
        let line = self.scanline;

        // Sprites are drawn one line below their Y coordinate, so a sprite is on
        // the next scanline when the current one falls within its rows.
        let mut found = [0usize; 8];
        let mut count = 0;
        for index in 0..64 {
            let y = self.oam[index * 4] as u16;
            if line < y || line >= y + height {
                continue;
            }
            if count == found.len() {
                self.status |= STATUS_OVERFLOW;
                break;
            }
            found[count] = index;
            count += 1;
        }

        // Draw back to front so lower OAM indices end up on top.
        for &index in found[..count].iter().rev() {
            let [y, tile, attr, x] = [0, 1, 2, 3].map(|i| self.oam[index * 4 + i]);
            let mut row = line - y as u16;
            if attr & 0x80 != 0 {
                row = height - 1 - row;
            }
            let tile = tile as u16;
            let addr = if height == 8 {
                ((self.ctrl & CTRL_SPRITE_TABLE) as u16) << 9 | tile << 4 | row
            } else {
                // 8x16 sprites take their pattern table from bit 0 of the tile index.
                (tile & 1) << 12 | ((tile & 0xFE) + (row >> 3)) << 4 | (row & 7)
            };
            let lo = mapper.chr_read(addr);
            let hi = mapper.chr_read(addr + 8);
            let flags = (attr & 3) << 2
                | (attr & SPRITE_BEHIND_BG)
                | if index == 0 { SPRITE_ZERO } else { 0 };
            for column in 0..8usize {
                let bit = if attr & 0x40 != 0 { column } else { 7 - column };
                let pixel = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
                if pixel == 0 {
                    continue;
                }
                if let Some(slot) = self.sprite_line.get_mut(x as usize + column) {
                    *slot = pixel | flags;
                }
            }
        }
    }

    /// Composes the background and sprite pixel for the current dot.
    fn render_pixel(&mut self, rendering: bool) {
        let x = (self.dot - 1) as usize;
        let left_edge = x < 8;

        let mut background = 0u8;
        if self.mask & MASK_BG != 0 && (!left_edge || self.mask & MASK_BG_LEFT != 0) {
            let bit = 15 - self.fine_x;
            let pixel =
                ((self.pattern_shift_hi >> bit) & 1) << 1 | (self.pattern_shift_lo >> bit) & 1;
            if pixel != 0 {
                let attr = ((self.attr_shift_hi >> bit) & 1) << 1 | (self.attr_shift_lo >> bit) & 1;
                background = (attr << 2 | pixel) as u8;
            }
        }

        let mut sprite = 0u8;
        if self.mask & MASK_SPRITES != 0 && (!left_edge || self.mask & MASK_SPRITES_LEFT != 0) {
            sprite = self.sprite_line[x];
        }

        let index = if sprite & 3 == 0 {
            background
        } else if background == 0 {
            0x10 | (sprite & 0x0F)
        } else {
            if sprite & SPRITE_ZERO != 0 && x != 255 {
                self.status |= STATUS_SPRITE0_HIT;
            }
            if sprite & SPRITE_BEHIND_BG != 0 { background } else { 0x10 | (sprite & 0x0F) }
        };

        // With rendering off the backdrop is shown, unless v points into the
        // palette, in which case that entry is shown instead.
        let index = if !rendering && self.v & 0x3F00 == 0x3F00 {
            palette_index(self.v)
        } else {
            index as usize
        };

        let grey_mask = if self.mask & MASK_GRAYSCALE != 0 { 0x30 } else { 0x3F };
        let colour = (self.palette[index] & grey_mask) as usize;
        let emphasis = (self.mask >> 5) as usize;
        self.framebuffer[self.scanline as usize * SCREEN_WIDTH + x] =
            self.palettes[emphasis][colour];
    }
}

/// Maps a PPU address to a palette RAM index. The sprite palettes' transparent
/// entries mirror the background ones.
fn palette_index(addr: u16) -> usize {
    let index = addr as usize & 0x1F;
    if index & 0x13 == 0x10 { index & 0x0F } else { index }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::Cartridge;
    use crate::cartridge::test_util::ines;
    use crate::mapper;

    /// NROM with CHR RAM so tests can upload their own tiles.
    fn setup(flags6: u8) -> (Ppu, Box<dyn Mapper>) {
        let cart = Cartridge::from_ines(&ines(0, 1, 0, flags6)).unwrap();
        (Ppu::new(), mapper::create(cart).unwrap())
    }

    fn poke(ppu: &mut Ppu, m: &mut dyn Mapper, addr: u16, data: &[u8]) {
        ppu.write_register(6, (addr >> 8) as u8, m);
        ppu.write_register(6, addr as u8, m);
        for &byte in data {
            ppu.write_register(7, byte, m);
        }
    }

    fn peek(ppu: &mut Ppu, m: &mut dyn Mapper, addr: u16) -> u8 {
        ppu.write_register(6, (addr >> 8) as u8, m);
        ppu.write_register(6, addr as u8, m);
        ppu.read_register(7, m);
        ppu.read_register(7, m)
    }

    fn run_frames(ppu: &mut Ppu, m: &mut dyn Mapper, frames: u64) {
        let target = ppu.frame() + frames;
        while ppu.frame() < target {
            ppu.tick(m);
        }
    }

    fn pixel(ppu: &Ppu, x: usize, y: usize) -> u32 {
        ppu.framebuffer()[y * SCREEN_WIDTH + x]
    }

    #[test]
    fn data_reads_are_buffered_except_for_palette() {
        let (mut ppu, mut m) = setup(0);
        poke(&mut ppu, &mut *m, 0x2005, &[0xAB, 0xCD]);
        ppu.write_register(6, 0x20, &mut *m);
        ppu.write_register(6, 0x05, &mut *m);
        ppu.read_register(7, &mut *m); // stale buffer
        assert_eq!(ppu.read_register(7, &mut *m), 0xAB);
        assert_eq!(ppu.read_register(7, &mut *m), 0xCD);
        poke(&mut ppu, &mut *m, 0x3F01, &[0x2A]);
        ppu.write_register(6, 0x3F, &mut *m);
        ppu.write_register(6, 0x01, &mut *m);
        assert_eq!(ppu.read_register(7, &mut *m), 0x2A);
    }

    #[test]
    fn nametable_mirroring_follows_the_cartridge() {
        let (mut ppu, mut m) = setup(0); // horizontal
        poke(&mut ppu, &mut *m, 0x2000, &[0x11]);
        assert_eq!(peek(&mut ppu, &mut *m, 0x2400), 0x11);
        assert_eq!(peek(&mut ppu, &mut *m, 0x2800), 0x00);
        let (mut ppu, mut m) = setup(1); // vertical
        poke(&mut ppu, &mut *m, 0x2000, &[0x22]);
        assert_eq!(peek(&mut ppu, &mut *m, 0x2800), 0x22);
        assert_eq!(peek(&mut ppu, &mut *m, 0x2400), 0x00);
    }

    #[test]
    fn sprite_palette_backdrops_mirror_background_ones() {
        let (mut ppu, mut m) = setup(0);
        poke(&mut ppu, &mut *m, 0x3F10, &[0x21]);
        ppu.write_register(6, 0x3F, &mut *m);
        ppu.write_register(6, 0x00, &mut *m);
        assert_eq!(ppu.read_register(7, &mut *m), 0x21);
    }

    #[test]
    fn vblank_flag_and_nmi_line() {
        let (mut ppu, mut m) = setup(0);
        ppu.write_register(0, CTRL_NMI, &mut *m);
        run_frames(&mut ppu, &mut *m, 1);
        assert_eq!(ppu.position(), (241, 2));
        assert!(ppu.nmi_line());
        assert_eq!(ppu.read_register(2, &mut *m) & 0x80, 0x80);
        assert!(!ppu.nmi_line()); // reading the status clears the flag
        assert_eq!(ppu.read_register(2, &mut *m) & 0x80, 0);
    }

    #[test]
    fn frame_is_one_dot_shorter_on_odd_frames_when_rendering() {
        let (mut ppu, mut m) = setup(0);
        let mut lengths = [0u32; 2];
        run_frames(&mut ppu, &mut *m, 1);
        ppu.write_register(1, MASK_BG, &mut *m);
        for length in lengths.iter_mut() {
            let frame = ppu.frame();
            while ppu.frame() == frame {
                ppu.tick(&mut *m);
                *length += 1;
            }
        }
        lengths.sort_unstable();
        assert_eq!(lengths, [341 * 262 - 1, 341 * 262]);
    }

    /// Fills tile 1 with solid colour 1 and tile 2 with solid colour 3.
    fn upload_tiles(ppu: &mut Ppu, m: &mut dyn Mapper) {
        poke(ppu, m, 0x0010, &[0xFF; 8]);
        poke(ppu, m, 0x0020, &[0xFF; 16]);
    }

    #[test]
    fn renders_background_tiles_with_attributes_and_scroll() {
        let (mut ppu, mut m) = setup(0);
        upload_tiles(&mut ppu, &mut *m);
        // Tile 1 at column 2, row 1; its attribute quadrant selects palette 2.
        poke(&mut ppu, &mut *m, 0x2022, &[0x01]);
        poke(&mut ppu, &mut *m, 0x23C0, &[0x02 << 2]);
        poke(&mut ppu, &mut *m, 0x3F00, &[0x0F]);
        poke(&mut ppu, &mut *m, 0x3F09, &[0x16]);
        // Reset scroll to the origin and enable the background everywhere.
        ppu.write_register(0, 0, &mut *m);
        ppu.write_register(5, 0, &mut *m);
        ppu.write_register(5, 0, &mut *m);
        ppu.write_register(1, MASK_BG | MASK_BG_LEFT, &mut *m);
        run_frames(&mut ppu, &mut *m, 2);
        assert_eq!(pixel(&ppu, 16, 8), BASE_PALETTE[0x16]);
        assert_eq!(pixel(&ppu, 23, 15), BASE_PALETTE[0x16]);
        assert_eq!(pixel(&ppu, 15, 8), BASE_PALETTE[0x0F]);
        assert_eq!(pixel(&ppu, 24, 8), BASE_PALETTE[0x0F]);
        assert_eq!(pixel(&ppu, 16, 7), BASE_PALETTE[0x0F]);
        assert_eq!(pixel(&ppu, 16, 16), BASE_PALETTE[0x0F]);

        // Scrolling 3 pixels right and 2 down moves the tile up and to the left.
        ppu.write_register(5, 3, &mut *m);
        ppu.write_register(5, 2, &mut *m);
        run_frames(&mut ppu, &mut *m, 2);
        assert_eq!(pixel(&ppu, 13, 6), BASE_PALETTE[0x16]);
        assert_eq!(pixel(&ppu, 20, 13), BASE_PALETTE[0x16]);
        assert_eq!(pixel(&ppu, 12, 6), BASE_PALETTE[0x0F]);
        assert_eq!(pixel(&ppu, 21, 6), BASE_PALETTE[0x0F]);
        assert_eq!(pixel(&ppu, 13, 5), BASE_PALETTE[0x0F]);
        assert_eq!(pixel(&ppu, 13, 14), BASE_PALETTE[0x0F]);
    }

    fn write_sprite(ppu: &mut Ppu, m: &mut dyn Mapper, index: u8, bytes: [u8; 4]) {
        ppu.write_register(3, index * 4, m);
        for byte in bytes {
            ppu.write_register(4, byte, m);
        }
    }

    #[test]
    fn renders_sprites_and_reports_sprite_zero_hit() {
        let (mut ppu, mut m) = setup(0);
        upload_tiles(&mut ppu, &mut *m);
        // Hide every sprite, then place sprite 0 over a background tile.
        for index in 0..64 {
            write_sprite(&mut ppu, &mut *m, index, [0xF0, 0, 0, 0]);
        }
        write_sprite(&mut ppu, &mut *m, 0, [39, 0x02, 0x01, 100]);
        poke(&mut ppu, &mut *m, 0x20AC, &[0x01]); // row 5, column 12 covers (96..104, 40..48)
        poke(&mut ppu, &mut *m, 0x3F00, &[0x0F, 0x11]);
        poke(&mut ppu, &mut *m, 0x3F17, &[0x27]);
        ppu.write_register(0, 0, &mut *m);
        ppu.write_register(5, 0, &mut *m);
        ppu.write_register(5, 0, &mut *m);
        ppu.write_register(1, MASK_BG | MASK_SPRITES | MASK_BG_LEFT | MASK_SPRITES_LEFT, &mut *m);
        run_frames(&mut ppu, &mut *m, 2);
        // The sprite covers x 100..108, y 40..48 (one line below its Y of 39).
        assert_eq!(pixel(&ppu, 100, 40), BASE_PALETTE[0x27]);
        assert_eq!(pixel(&ppu, 107, 47), BASE_PALETTE[0x27]);
        assert_eq!(pixel(&ppu, 99, 40), BASE_PALETTE[0x11]);
        assert_eq!(pixel(&ppu, 108, 40), BASE_PALETTE[0x0F]);
        assert_eq!(pixel(&ppu, 100, 39), BASE_PALETTE[0x0F]);
        assert_eq!(pixel(&ppu, 104, 48), BASE_PALETTE[0x0F]);
        assert_ne!(ppu.read_register(2, &mut *m) & STATUS_SPRITE0_HIT, 0);

        // Behind-background priority: the background wins where it is opaque.
        write_sprite(&mut ppu, &mut *m, 0, [39, 0x02, 0x21, 100]);
        run_frames(&mut ppu, &mut *m, 2);
        assert_eq!(pixel(&ppu, 100, 40), BASE_PALETTE[0x11]);
        assert_eq!(pixel(&ppu, 104, 40), BASE_PALETTE[0x27]);
    }

    #[test]
    fn sprite_overflow_is_flagged_for_nine_sprites_on_a_line() {
        let (mut ppu, mut m) = setup(0);
        for index in 0..64 {
            write_sprite(&mut ppu, &mut *m, index, [0xF0, 0, 0, 0]);
        }
        for index in 0..8 {
            write_sprite(&mut ppu, &mut *m, index, [50, 0, 0, index * 10]);
        }
        ppu.write_register(1, MASK_SPRITES, &mut *m);
        run_frames(&mut ppu, &mut *m, 2);
        assert_eq!(ppu.read_register(2, &mut *m) & STATUS_OVERFLOW, 0);
        write_sprite(&mut ppu, &mut *m, 8, [50, 0, 0, 90]);
        run_frames(&mut ppu, &mut *m, 2);
        assert_ne!(ppu.read_register(2, &mut *m) & STATUS_OVERFLOW, 0);
    }
}
