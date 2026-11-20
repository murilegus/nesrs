//! A loaded game: the console plus the files that belong to its ROM.

use std::path::{Path, PathBuf};

use nesrs_core::Nes;

pub struct Session {
    pub nes: Nes,
    rom: PathBuf,
    /// Battery RAM as last written to disk, to avoid rewriting it needlessly.
    saved_ram: Option<Vec<u8>>,
}

impl Session {
    /// Loads a ROM, along with its battery save if there is one.
    pub fn open(rom: &Path) -> Result<Self, String> {
        let data = std::fs::read(rom).map_err(|e| format!("cannot read file: {e}"))?;
        let nes = Nes::new(&data).map_err(|e| e.to_string())?;
        let mut session = Session { nes, rom: rom.into(), saved_ram: None };
        if session.nes.save_ram().is_some()
            && let Ok(saved) = std::fs::read(session.battery_path())
        {
            session.nes.load_save_ram(&saved);
            session.saved_ram = session.nes.save_ram().map(<[u8]>::to_vec);
        }
        Ok(session)
    }

    /// The ROM's file name without its extension.
    pub fn name(&self) -> String {
        self.rom.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }

    fn battery_path(&self) -> PathBuf {
        self.rom.with_extension("sav")
    }

    fn state_path(&self, slot: u8) -> PathBuf {
        self.rom.with_extension(format!("state{slot}"))
    }

    /// Writes battery-backed RAM to disk if it changed since the last write.
    pub fn flush_battery(&mut self) -> Result<(), String> {
        let Some(ram) = self.nes.save_ram() else {
            return Ok(());
        };
        if self.saved_ram.as_deref() == Some(ram) {
            return Ok(());
        }
        let path = self.battery_path();
        std::fs::write(&path, ram).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        self.saved_ram = Some(ram.to_vec());
        Ok(())
    }

    /// Writes a save state to the given slot.
    pub fn save_state(&mut self, slot: u8) -> Result<(), String> {
        let path = self.state_path(slot);
        std::fs::write(&path, self.nes.save_state())
            .map_err(|e| format!("cannot write {}: {e}", path.display()))
    }

    /// Restores the save state in the given slot. `Ok(false)` means the slot is empty.
    pub fn load_state(&mut self, slot: u8) -> Result<bool, String> {
        let path = self.state_path(slot);
        let data = match std::fs::read(&path) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        };
        self.nes.load_state(&data).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a minimal MMC1 ROM with battery RAM into a fresh temporary directory.
    fn temp_rom(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("nesrs-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut image = vec![b'N', b'E', b'S', 0x1A, 1, 0, 0x12, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        // LDA #$42; STA $6000; INC $10; JMP $8005 -- with all vectors at $8000.
        let mut prg = vec![0xEA; 0x4000];
        prg[..10].copy_from_slice(&[0xA9, 0x42, 0x8D, 0x00, 0x60, 0xE6, 0x10, 0x4C, 0x05, 0x80]);
        prg[0x3FFA..].copy_from_slice(&[0x00, 0x80, 0x00, 0x80, 0x00, 0x80]);
        image.extend(prg);
        let rom = dir.join("game.nes");
        std::fs::write(&rom, image).unwrap();
        rom
    }

    #[test]
    fn save_states_live_in_numbered_slots_next_to_the_rom() {
        let rom = temp_rom("states");
        let mut session = Session::open(&rom).unwrap();
        assert_eq!(session.name(), "game");
        assert_eq!(session.load_state(3), Ok(false));

        session.nes.run_frame();
        let counter = session.nes.ram()[0x10];
        session.save_state(3).unwrap();
        assert!(rom.with_extension("state3").exists());
        session.nes.run_frame();
        assert_ne!(session.nes.ram()[0x10], counter);
        assert_eq!(session.load_state(3), Ok(true));
        assert_eq!(session.nes.ram()[0x10], counter);
        assert_eq!(session.load_state(4), Ok(false));

        std::fs::write(rom.with_extension("state5"), b"garbage").unwrap();
        assert!(session.load_state(5).is_err());
        assert_eq!(session.nes.ram()[0x10], counter);
        std::fs::remove_dir_all(rom.parent().unwrap()).unwrap();
    }

    #[test]
    fn battery_ram_is_written_when_changed_and_reloaded() {
        let rom = temp_rom("battery");
        let save = rom.with_extension("sav");
        let mut session = Session::open(&rom).unwrap();
        session.nes.run_frame();
        session.flush_battery().unwrap();
        assert_eq!(std::fs::read(&save).unwrap()[0], 0x42);

        // Nothing changed, so a deleted file is not rewritten.
        std::fs::remove_file(&save).unwrap();
        session.flush_battery().unwrap();
        assert!(!save.exists());

        let mut data = vec![0u8; 0x2000];
        data[1] = 0x99;
        std::fs::write(&save, &data).unwrap();
        let reopened = Session::open(&rom).unwrap();
        assert_eq!(reopened.nes.save_ram().unwrap()[1], 0x99);
        std::fs::remove_dir_all(rom.parent().unwrap()).unwrap();
    }

    #[test]
    fn opening_a_bad_file_reports_why() {
        let rom = temp_rom("bad");
        std::fs::write(&rom, b"not a rom").unwrap();
        assert!(Session::open(&rom).err().unwrap().contains("iNES"));
        assert!(Session::open(&rom.with_extension("missing")).is_err());
        std::fs::remove_dir_all(rom.parent().unwrap()).unwrap();
    }
}
