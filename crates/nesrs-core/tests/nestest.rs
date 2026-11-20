//! Compares CPU execution against the reference log of kevtris's `nestest` ROM.
//!
//! The ROM is not distributed with this repository. To run the test, place
//! `nestest.nes` and `nestest.log` in `test-roms/` at the workspace root.

use std::path::Path;

use nesrs_core::Nes;

fn field(line: &str, key: &str) -> u64 {
    let start = line.find(key).unwrap_or_else(|| panic!("no {key} in {line}")) + key.len();
    let digits: String = line[start..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
    let radix = if key == "CYC:" { 10 } else { 16 };
    u64::from_str_radix(&digits, radix).unwrap()
}

/// Parses the `PPU:scanline,dot` column.
fn ppu_field(line: &str) -> (u16, u16) {
    let start = line.find("PPU:").unwrap() + 4;
    let end = line.find("CYC:").unwrap();
    let (scanline, dot) = line[start..end].split_once(',').unwrap();
    (scanline.trim().parse().unwrap(), dot.trim().parse().unwrap())
}

#[test]
fn matches_the_reference_log() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test-roms");
    let (Ok(rom), Ok(log)) =
        (std::fs::read(dir.join("nestest.nes")), std::fs::read_to_string(dir.join("nestest.log")))
    else {
        eprintln!("skipping: test-roms/nestest.nes and nestest.log not found");
        return;
    };

    let mut nes = Nes::new(&rom).unwrap();
    // The automated mode of the ROM starts at $C000 instead of the reset vector.
    nes.cpu_mut().pc = 0xC000;

    for (number, line) in log.lines().enumerate() {
        let cpu = nes.cpu();
        let actual = (cpu.pc as u64, cpu.a, cpu.x, cpu.y, cpu.p, cpu.sp, nes.cycles());
        let ppu = nes.ppu_position();
        let expected = (
            u64::from_str_radix(&line[0..4], 16).unwrap(),
            field(line, "A:") as u8,
            field(line, "X:") as u8,
            field(line, "Y:") as u8,
            field(line, "P:") as u8,
            field(line, "SP:") as u8,
            field(line, "CYC:"),
        );
        assert_eq!(actual, expected, "diverged at log line {}:\n{line}", number + 1);
        assert_eq!(ppu, ppu_field(line), "PPU position at log line {}:\n{line}", number + 1);
        nes.step();
    }
    // The ROM reports failures in $02 (official opcodes) and $03 (unofficial).
    assert_eq!(nes.ram()[0x02], 0, "official opcode test failed");
    assert_eq!(nes.ram()[0x03], 0, "unofficial opcode test failed");
}
