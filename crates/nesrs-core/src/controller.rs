//! Standard NES controller: an 8-bit parallel-in, serial-out shift register.

/// Button bit masks, in the order the controller shifts them out.
pub mod button {
    pub const A: u8 = 0x01;
    pub const B: u8 = 0x02;
    pub const SELECT: u8 = 0x04;
    pub const START: u8 = 0x08;
    pub const UP: u8 = 0x10;
    pub const DOWN: u8 = 0x20;
    pub const LEFT: u8 = 0x40;
    pub const RIGHT: u8 = 0x80;
}

use crate::state::State;

#[derive(Default)]
pub struct Controller {
    buttons: u8,
    shift: u8,
    strobe: bool,
}

impl Controller {
    /// Sets which buttons are held, as a combination of [`button`] masks.
    pub fn set_buttons(&mut self, mut buttons: u8) {
        // A real D-pad cannot press opposite directions, and some games misbehave
        // when they see both.
        for pair in [button::UP | button::DOWN, button::LEFT | button::RIGHT] {
            if buttons & pair == pair {
                buttons &= !pair;
            }
        }
        self.buttons = buttons;
        if self.strobe {
            self.shift = buttons;
        }
    }

    /// Handles a write to `$4016`: while the strobe bit is high the shift register
    /// continuously reloads from the buttons.
    pub fn write(&mut self, value: u8) {
        self.strobe = value & 1 != 0;
        if self.strobe {
            self.shift = self.buttons;
        }
    }

    /// Shifts out the next button state (bit 0). After all eight, reads return 1.
    pub fn read(&mut self) -> u8 {
        if self.strobe {
            return self.buttons & 1;
        }
        let bit = self.shift & 1;
        self.shift = self.shift >> 1 | 0x80;
        bit
    }
}

impl Controller {
    pub(crate) fn sync(&mut self, s: &mut State) {
        s.u8(&mut self.shift);
        s.bool(&mut self.strobe);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifts_out_buttons_in_order_then_ones() {
        let mut pad = Controller::default();
        pad.set_buttons(button::A | button::START | button::RIGHT);
        pad.write(1);
        pad.write(0);
        let bits: Vec<u8> = (0..10).map(|_| pad.read()).collect();
        assert_eq!(bits, [1, 0, 0, 1, 0, 0, 0, 1, 1, 1]);
    }

    #[test]
    fn strobe_high_keeps_returning_a() {
        let mut pad = Controller::default();
        pad.set_buttons(button::A);
        pad.write(1);
        assert_eq!([pad.read(), pad.read(), pad.read()], [1, 1, 1]);
    }

    #[test]
    fn opposite_directions_cancel_out() {
        let mut pad = Controller::default();
        pad.set_buttons(button::LEFT | button::RIGHT | button::UP);
        pad.write(1);
        pad.write(0);
        let bits: Vec<u8> = (0..8).map(|_| pad.read()).collect();
        assert_eq!(bits, [0, 0, 0, 0, 1, 0, 0, 0]);
    }
}
