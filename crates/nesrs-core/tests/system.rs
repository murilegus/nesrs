//! Whole-system tests driven by small hand-assembled ROMs.

use nesrs_core::{Nes, RomError, button};

/// Wraps 6502 code in an NROM image with CHR RAM. `nmi_offset` is the offset of
/// the NMI handler within `code`, which is loaded at `$8000`.
fn rom(code: &[u8], nmi_offset: usize) -> Vec<u8> {
    let mut image = vec![b'N', b'E', b'S', 0x1A, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let mut prg = vec![0xEA; 0x4000];
    prg[..code.len()].copy_from_slice(code);
    let nmi = 0x8000 + nmi_offset as u16;
    prg[0x3FFA..0x3FFC].copy_from_slice(&nmi.to_le_bytes());
    prg[0x3FFC..0x3FFE].copy_from_slice(&0x8000u16.to_le_bytes());
    prg[0x3FFE..0x4000].copy_from_slice(&0x8000u16.to_le_bytes());
    image.extend(prg);
    image
}

/// A program that draws one white tile, enables NMIs, and has its NMI handler
/// count frames in `$10` and read controller 1 into `$11`.
fn demo_rom() -> Vec<u8> {
    #[rustfmt::skip]
    let mut code: Vec<u8> = vec![
        0x78, 0xD8,                   // SEI; CLD
        0xA2, 0xFF, 0x9A,             // LDX #$FF; TXS
        0x2C, 0x02, 0x20, 0x10, 0xFB, // wait for vblank
        0x2C, 0x02, 0x20, 0x10, 0xFB, // ... twice
        // Palette: backdrop black, colour 1 white.
        0xA9, 0x3F, 0x8D, 0x06, 0x20, 0xA9, 0x00, 0x8D, 0x06, 0x20,
        0xA9, 0x0F, 0x8D, 0x07, 0x20, 0xA9, 0x30, 0x8D, 0x07, 0x20,
        // Tile 1: low bitplane all ones.
        0xA9, 0x00, 0x8D, 0x06, 0x20, 0xA9, 0x10, 0x8D, 0x06, 0x20,
        0xA2, 0x08, 0xA9, 0xFF,
        0x8D, 0x07, 0x20, 0xCA, 0xD0, 0xFA,
        // Nametable entry at row 1, column 1.
        0xA9, 0x20, 0x8D, 0x06, 0x20, 0xA9, 0x21, 0x8D, 0x06, 0x20,
        0xA9, 0x01, 0x8D, 0x07, 0x20,
        // Reset the scroll, then enable NMIs and the background.
        0xA9, 0x00, 0x8D, 0x06, 0x20, 0x8D, 0x06, 0x20, 0x8D, 0x05, 0x20, 0x8D, 0x05, 0x20,
        0xA9, 0x80, 0x8D, 0x00, 0x20,
        0xA9, 0x0A, 0x8D, 0x01, 0x20,
    ];
    let forever = 0x8000 + code.len() as u16;
    code.push(0x4C);
    code.extend(forever.to_le_bytes());
    let nmi_offset = code.len();
    #[rustfmt::skip]
    code.extend([
        0xE6, 0x10,                               // INC $10
        0xA9, 0x01, 0x8D, 0x16, 0x40,             // strobe the controller
        0xA9, 0x00, 0x8D, 0x16, 0x40,
        0xA2, 0x08,                               // LDX #8
        0xAD, 0x16, 0x40, 0x4A, 0x26, 0x11,       // LDA $4016; LSR A; ROL $11
        0xCA, 0xD0, 0xF7,                         // DEX; BNE
        0x40,                                     // RTI
    ]);
    rom(&code, nmi_offset)
}

#[test]
fn boots_renders_and_handles_nmi_and_input() {
    let mut nes = Nes::new(&demo_rom()).unwrap();
    nes.set_buttons(0, button::A | button::START);
    for _ in 0..10 {
        nes.run_frame();
    }
    // The NMI handler has been running once per frame since rendering came on.
    let frames = nes.ram()[0x10];
    assert!((6..=9).contains(&frames), "NMI ran {frames} times");
    // Buttons arrive A first, so ROL leaves A in bit 7 and Start in bit 4.
    assert_eq!(nes.ram()[0x11], 0x90);

    let pixel = |x: usize, y: usize| nes.framebuffer()[y * 256 + x];
    assert_eq!(pixel(8, 8), 0xFFFEFF);
    assert_eq!(pixel(15, 15), 0xFFFEFF);
    assert_eq!(pixel(7, 8), 0x000000);
    assert_eq!(pixel(16, 8), 0x000000);
    assert_eq!(pixel(8, 16), 0x000000);
}

#[test]
fn frames_take_the_ntsc_number_of_cpu_cycles() {
    let mut nes = Nes::new(&demo_rom()).unwrap();
    for _ in 0..10 {
        nes.run_frame();
    }
    let start = nes.cycles();
    for _ in 0..60 {
        nes.run_frame();
    }
    // 341 * 262 dots per frame, minus half a dot on average, at 3 dots per cycle.
    let per_frame = (nes.cycles() - start) as f64 / 60.0;
    assert!((per_frame - 29780.67).abs() < 1.0, "{per_frame} cycles per frame");
}

#[test]
fn audio_is_generated_at_the_requested_rate() {
    let mut nes = Nes::new(&demo_rom()).unwrap();
    nes.set_sample_rate(44_100);
    let mut samples = Vec::new();
    // The first frame after power-on is short: discard it.
    nes.run_frame();
    nes.drain_audio(&mut samples);
    samples.clear();
    for _ in 0..60 {
        nes.run_frame();
        nes.drain_audio(&mut samples);
    }
    let expected = 44_100.0 * 60.0 / nesrs_core::FRAME_RATE;
    assert!((samples.len() as f64 - expected).abs() < 50.0, "{} samples", samples.len());
}

#[test]
fn oam_dma_halts_the_cpu_for_513_or_514_cycles() {
    // LDA #$02; STA $4014; then NOPs.
    let mut nes = Nes::new(&rom(&[0xA9, 0x02, 0x8D, 0x14, 0x40], 0)).unwrap();
    nes.step();
    nes.step();
    let before = nes.cycles();
    nes.step(); // DMA followed by a 2-cycle NOP
    let dma = nes.cycles() - before - 2;
    assert!(dma == 513 || dma == 514, "DMA took {dma} cycles");
}

#[test]
fn reset_restarts_the_program_but_keeps_ram() {
    let mut nes = Nes::new(&demo_rom()).unwrap();
    for _ in 0..10 {
        nes.run_frame();
    }
    let frames = nes.ram()[0x10];
    nes.reset();
    assert_eq!(nes.cpu().pc, 0x8000);
    assert_eq!(nes.ram()[0x10], frames);
    for _ in 0..10 {
        nes.run_frame();
    }
    assert!(nes.ram()[0x10] > frames);
}

#[test]
fn unsupported_mappers_are_reported() {
    let mut image = rom(&[], 0);
    image[6] = 0x50; // mapper 5
    assert_eq!(Nes::new(&image).err(), Some(RomError::UnsupportedMapper(5)));
}

/// Everything observable about a run: picture, RAM, CPU and timing.
fn fingerprint(nes: &Nes) -> (Vec<u32>, Vec<u8>, u16, u64, (u16, u16)) {
    (nes.framebuffer().to_vec(), nes.ram().to_vec(), nes.cpu().pc, nes.cycles(), nes.ppu_position())
}

#[test]
fn save_state_resumes_exactly_where_it_was_taken() {
    let mut nes = Nes::new(&demo_rom()).unwrap();
    for _ in 0..7 {
        nes.run_frame();
    }
    // Stop mid-frame, where the PPU pipeline and CPU are both in full swing.
    for _ in 0..1234 {
        nes.step();
    }
    let state = nes.save_state();
    assert_eq!(nes.save_state(), state, "saving must not change anything");

    nes.set_buttons(0, button::B);
    for _ in 0..5 {
        nes.run_frame();
    }
    let expected = fingerprint(&nes);

    // Wander off, then come back.
    nes.set_buttons(0, button::LEFT | button::A);
    for _ in 0..13 {
        nes.run_frame();
    }
    assert_ne!(fingerprint(&nes), expected);
    nes.load_state(&state).unwrap();
    nes.set_buttons(0, button::B);
    for _ in 0..5 {
        nes.run_frame();
    }
    assert_eq!(fingerprint(&nes), expected);

    // A freshly powered-on console can load it too.
    let mut other = Nes::new(&demo_rom()).unwrap();
    other.load_state(&state).unwrap();
    other.set_buttons(0, button::B);
    for _ in 0..5 {
        other.run_frame();
    }
    assert_eq!(fingerprint(&other), expected);
}

#[test]
fn bad_save_states_are_rejected_without_side_effects() {
    use nesrs_core::StateError;

    let mut nes = Nes::new(&demo_rom()).unwrap();
    for _ in 0..5 {
        nes.run_frame();
    }
    let state = nes.save_state();
    for _ in 0..3 {
        nes.run_frame();
    }
    let before = fingerprint(&nes);

    assert_eq!(nes.load_state(b"not a state"), Err(StateError::NotAState));
    let mut damaged = state.clone();
    damaged[100] ^= 0x40;
    assert_eq!(nes.load_state(&damaged), Err(StateError::Corrupt));
    assert_eq!(nes.load_state(&state[..state.len() / 2]), Err(StateError::Corrupt));
    assert_eq!(fingerprint(&nes), before);

    // A state from another game is refused.
    let mut other = Nes::new(&rom(&[0xA9, 0x02, 0x8D, 0x14, 0x40], 0)).unwrap();
    assert_eq!(other.load_state(&state), Err(StateError::WrongRom));
}

#[test]
fn both_controller_ports_are_read_independently() {
    // Strobe, then shift eight bits from $4016 into $20 and from $4017 into $21.
    #[rustfmt::skip]
    let code = [
        0xA9, 0x01, 0x8D, 0x16, 0x40,       // LDA #1; STA $4016
        0xA9, 0x00, 0x8D, 0x16, 0x40,       // LDA #0; STA $4016
        0xA2, 0x08,                         // LDX #8
        0xAD, 0x16, 0x40, 0x4A, 0x26, 0x20, // LDA $4016; LSR A; ROL $20
        0xAD, 0x17, 0x40, 0x4A, 0x26, 0x21, // LDA $4017; LSR A; ROL $21
        0xCA, 0xD0, 0xF1,                   // DEX; BNE
        0x4C, 0x1B, 0x80,                   // JMP self
    ];
    let mut nes = Nes::new(&rom(&code, 0)).unwrap();
    nes.set_buttons(0, button::A | button::RIGHT);
    nes.set_buttons(1, button::B | button::START | button::UP);
    for _ in 0..200 {
        nes.step();
    }
    // The first button read ends up in bit 7: A B Select Start Up Down Left Right.
    assert_eq!(nes.ram()[0x20], 0b1000_0001);
    assert_eq!(nes.ram()[0x21], 0b0101_1000);
}
