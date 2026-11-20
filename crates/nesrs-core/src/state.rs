//! Save states: a snapshot of the whole console that can be restored later.
//!
//! Every component has a single `sync` method that is used for both saving and
//! loading, so the two directions cannot drift apart.

use std::fmt;

/// File signature; the last byte is the format version.
const MAGIC: &[u8; 7] = b"NESRSST";
/// Bump whenever the layout written by any `sync` method changes.
const VERSION: u8 = 1;
/// Signature, version and ROM hash.
const HEADER_LEN: usize = 16;
const CHECKSUM_LEN: usize = 8;

/// Reasons a save state can be rejected.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum StateError {
    /// The data is not a nesrs save state.
    NotAState,
    /// The state was written by an incompatible version of the emulator.
    UnsupportedVersion(u8),
    /// The state belongs to a different ROM.
    WrongRom,
    /// The data is damaged.
    Corrupt,
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::NotAState => write!(f, "not a nesrs save state"),
            StateError::UnsupportedVersion(v) => {
                write!(f, "save state format {v} is not supported (expected {VERSION})")
            }
            StateError::WrongRom => write!(f, "save state belongs to a different ROM"),
            StateError::Corrupt => write!(f, "save state is corrupt"),
        }
    }
}

impl std::error::Error for StateError {}

/// 64-bit FNV-1a hash, used to tie states to their ROM and to detect damage.
pub(crate) fn hash(data: &[u8]) -> u64 {
    data.iter().fold(0xCBF2_9CE4_8422_2325, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01B3))
}

/// A stream that either records values or restores them, depending on how it
/// was created.
pub struct State {
    data: Vec<u8>,
    pos: usize,
    loading: bool,
    overrun: bool,
}

impl State {
    pub(crate) fn saver() -> Self {
        State { data: Vec::new(), pos: 0, loading: false, overrun: false }
    }

    pub(crate) fn loader(data: &[u8]) -> Self {
        State { data: data.to_vec(), pos: 0, loading: true, overrun: false }
    }

    /// Whether loading consumed exactly the data that was provided.
    pub(crate) fn fully_consumed(&self) -> bool {
        !self.overrun && self.pos == self.data.len()
    }

    /// Wraps the recorded data in a header and checksum.
    pub(crate) fn finish(self, rom_hash: u64) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + self.data.len() + CHECKSUM_LEN);
        out.extend_from_slice(MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&rom_hash.to_le_bytes());
        out.extend_from_slice(&self.data);
        out.extend_from_slice(&hash(&out).to_le_bytes());
        out
    }

    /// Validates a finished state and returns its body.
    pub(crate) fn open(data: &[u8], rom_hash: u64) -> Result<&[u8], StateError> {
        if data.len() < HEADER_LEN + CHECKSUM_LEN || &data[..7] != MAGIC {
            return Err(StateError::NotAState);
        }
        if data[7] != VERSION {
            return Err(StateError::UnsupportedVersion(data[7]));
        }
        let (content, checksum) = data.split_at(data.len() - CHECKSUM_LEN);
        if checksum != hash(content).to_le_bytes() {
            return Err(StateError::Corrupt);
        }
        if content[8..HEADER_LEN] != rom_hash.to_le_bytes() {
            return Err(StateError::WrongRom);
        }
        Ok(&content[HEADER_LEN..])
    }

    pub fn bytes(&mut self, value: &mut [u8]) {
        if !self.loading {
            self.data.extend_from_slice(value);
            return;
        }
        match self.data.get(self.pos..self.pos + value.len()) {
            Some(source) => value.copy_from_slice(source),
            None => self.overrun = true,
        }
        self.pos += value.len();
    }

    pub fn u8(&mut self, value: &mut u8) {
        self.bytes(std::slice::from_mut(value));
    }

    pub fn bool(&mut self, value: &mut bool) {
        let mut byte = *value as u8;
        self.u8(&mut byte);
        *value = byte != 0;
    }

    pub fn u16(&mut self, value: &mut u16) {
        let mut bytes = value.to_le_bytes();
        self.bytes(&mut bytes);
        *value = u16::from_le_bytes(bytes);
    }

    pub fn u32(&mut self, value: &mut u32) {
        let mut bytes = value.to_le_bytes();
        self.bytes(&mut bytes);
        *value = u32::from_le_bytes(bytes);
    }

    pub fn u64(&mut self, value: &mut u64) {
        let mut bytes = value.to_le_bytes();
        self.bytes(&mut bytes);
        *value = u64::from_le_bytes(bytes);
    }

    pub fn usize(&mut self, value: &mut usize) {
        let mut wide = *value as u64;
        self.u64(&mut wide);
        *value = wide as usize;
    }

    pub fn option_u8(&mut self, value: &mut Option<u8>) {
        let mut present = value.is_some();
        let mut inner = value.unwrap_or(0);
        self.bool(&mut present);
        self.u8(&mut inner);
        *value = present.then_some(inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip() {
        let mut values = (0xABu8, true, 0x1234u16, 0xDEAD_BEEFu32, u64::MAX - 1, 77usize);
        let mut options = (Some(9u8), None::<u8>);
        let mut array = [1u8, 2, 3];
        let mut saver = State::saver();
        saver.u8(&mut values.0);
        saver.bool(&mut values.1);
        saver.u16(&mut values.2);
        saver.u32(&mut values.3);
        saver.u64(&mut values.4);
        saver.usize(&mut values.5);
        saver.option_u8(&mut options.0);
        saver.option_u8(&mut options.1);
        saver.bytes(&mut array);
        let file = saver.finish(42);

        let mut loader = State::loader(State::open(&file, 42).unwrap());
        let mut read = (0u8, false, 0u16, 0u32, 0u64, 0usize);
        let mut read_options = (None, Some(1u8));
        let mut read_array = [0u8; 3];
        loader.u8(&mut read.0);
        loader.bool(&mut read.1);
        loader.u16(&mut read.2);
        loader.u32(&mut read.3);
        loader.u64(&mut read.4);
        loader.usize(&mut read.5);
        loader.option_u8(&mut read_options.0);
        loader.option_u8(&mut read_options.1);
        assert!(!loader.fully_consumed());
        loader.bytes(&mut read_array);
        assert!(loader.fully_consumed());
        assert_eq!((read, read_options, read_array), (values, options, array));
        loader.u8(&mut read.0);
        assert!(!loader.fully_consumed());
    }

    #[test]
    fn open_rejects_bad_files() {
        let mut saver = State::saver();
        saver.u32(&mut 7);
        let file = saver.finish(1);
        assert_eq!(State::open(&file, 1).unwrap().len(), 4);
        assert_eq!(State::open(&file, 2), Err(StateError::WrongRom));
        assert_eq!(State::open(b"hello", 1), Err(StateError::NotAState));
        assert_eq!(State::open(&file[..file.len() - 1], 1), Err(StateError::Corrupt));
        let mut damaged = file.clone();
        damaged[17] ^= 1;
        assert_eq!(State::open(&damaged, 1), Err(StateError::Corrupt));
        let mut old = file;
        old[7] = 0;
        assert_eq!(State::open(&old, 1), Err(StateError::UnsupportedVersion(0)));
    }
}
