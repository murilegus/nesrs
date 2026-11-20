//! Game controller input. The first two controllers connected drive players
//! 1 and 2, alongside the keyboard.

// Without the `gamepad` feature only the unit tests use the mapping below.
#![cfg_attr(not(feature = "gamepad"), allow(dead_code))]

use nesrs_core::button;

/// How far a stick must be pushed to count as a direction.
const STICK_THRESHOLD: f32 = 0.5;

/// The parts of a controller we care about.
#[derive(Clone, Copy, Default, Debug)]
pub struct PadState {
    pub south: bool,
    pub east: bool,
    pub west: bool,
    pub north: bool,
    pub select: bool,
    pub start: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    /// Left stick, -1.0 (left) to 1.0 (right).
    pub stick_x: f32,
    /// Left stick, -1.0 (down) to 1.0 (up).
    pub stick_y: f32,
}

impl PadState {
    /// Translates the controller state to NES buttons. The bottom and right face
    /// buttons are B and A, like the NES pad's layout; the other two mirror them
    /// so either pair is comfortable.
    pub fn nes_buttons(&self) -> u8 {
        let mut buttons = 0;
        let mut map = |pressed: bool, mask: u8| {
            if pressed {
                buttons |= mask;
            }
        };
        map(self.east || self.north, button::A);
        map(self.south || self.west, button::B);
        map(self.select, button::SELECT);
        map(self.start, button::START);
        map(self.up || self.stick_y > STICK_THRESHOLD, button::UP);
        map(self.down || self.stick_y < -STICK_THRESHOLD, button::DOWN);
        map(self.left || self.stick_x < -STICK_THRESHOLD, button::LEFT);
        map(self.right || self.stick_x > STICK_THRESHOLD, button::RIGHT);
        buttons
    }
}

/// Connected controllers, in the order they were plugged in.
pub struct Gamepads {
    #[cfg(feature = "gamepad")]
    backend: Option<backend::Backend>,
}

impl Gamepads {
    /// Reads the buttons held on the first two controllers. Also returns a
    /// message when a controller was plugged in or removed since the last call.
    pub fn poll(&mut self) -> ([u8; 2], Option<String>) {
        #[cfg(feature = "gamepad")]
        if let Some(backend) = &mut self.backend {
            return backend.poll();
        }
        ([0; 2], None)
    }
}

#[cfg(feature = "gamepad")]
mod backend {
    use gilrs::{Axis, Button, EventType, GamepadId, Gilrs};

    use super::{Gamepads, PadState};

    pub struct Backend {
        gilrs: Gilrs,
        order: Vec<GamepadId>,
    }

    impl Gamepads {
        pub fn new() -> Self {
            let backend = match Gilrs::new() {
                Ok(gilrs) => {
                    let order = gilrs.gamepads().map(|(id, _)| id).collect();
                    Some(Backend { gilrs, order })
                }
                Err(error) => {
                    eprintln!("nesrs: gamepads disabled: {error}");
                    None
                }
            };
            Gamepads { backend }
        }
    }

    impl Backend {
        pub fn poll(&mut self) -> ([u8; 2], Option<String>) {
            let mut message = None;
            // Draining the event queue is also what updates the cached state.
            while let Some(event) = self.gilrs.next_event() {
                match event.event {
                    EventType::Connected if !self.order.contains(&event.id) => {
                        self.order.push(event.id);
                        message = Some(format!("GAMEPAD {} CONNECTED", self.order.len()));
                    }
                    EventType::Disconnected => {
                        if let Some(index) = self.order.iter().position(|&id| id == event.id) {
                            self.order.remove(index);
                            message = Some(format!("GAMEPAD {} DISCONNECTED", index + 1));
                        }
                    }
                    _ => {}
                }
            }

            let mut buttons = [0; 2];
            for (slot, &id) in buttons.iter_mut().zip(&self.order) {
                let pad = self.gilrs.gamepad(id);
                *slot = PadState {
                    south: pad.is_pressed(Button::South),
                    east: pad.is_pressed(Button::East),
                    west: pad.is_pressed(Button::West),
                    north: pad.is_pressed(Button::North),
                    select: pad.is_pressed(Button::Select),
                    start: pad.is_pressed(Button::Start),
                    up: pad.is_pressed(Button::DPadUp),
                    down: pad.is_pressed(Button::DPadDown),
                    left: pad.is_pressed(Button::DPadLeft),
                    right: pad.is_pressed(Button::DPadRight),
                    stick_x: pad.value(Axis::LeftStickX),
                    stick_y: pad.value(Axis::LeftStickY),
                }
                .nes_buttons();
            }
            (buttons, message)
        }
    }
}

#[cfg(not(feature = "gamepad"))]
impl Gamepads {
    /// Controller support was not compiled in.
    pub fn new() -> Self {
        Gamepads {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_buttons_follow_the_nes_layout() {
        let pad = |f: fn(&mut PadState)| {
            let mut state = PadState::default();
            f(&mut state);
            state.nes_buttons()
        };
        assert_eq!(pad(|_| {}), 0);
        assert_eq!(pad(|s| s.east = true), button::A);
        assert_eq!(pad(|s| s.north = true), button::A);
        assert_eq!(pad(|s| s.south = true), button::B);
        assert_eq!(pad(|s| s.west = true), button::B);
        assert_eq!(pad(|s| s.start = true), button::START);
        assert_eq!(pad(|s| s.select = true), button::SELECT);
    }

    #[test]
    fn dpad_and_stick_both_steer() {
        let state = PadState { up: true, right: true, ..PadState::default() };
        assert_eq!(state.nes_buttons(), button::UP | button::RIGHT);
        let state = PadState { stick_x: -0.9, stick_y: -0.7, ..PadState::default() };
        assert_eq!(state.nes_buttons(), button::LEFT | button::DOWN);
        let state = PadState { stick_x: 0.8, stick_y: 0.6, ..PadState::default() };
        assert_eq!(state.nes_buttons(), button::RIGHT | button::UP);
        // Small deflections (stick drift) are ignored.
        let state = PadState { stick_x: 0.3, stick_y: -0.4, ..PadState::default() };
        assert_eq!(state.nes_buttons(), 0);
    }
}
