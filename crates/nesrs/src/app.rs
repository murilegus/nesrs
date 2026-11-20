//! The windowed application: event handling, frame pacing and presentation.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use nesrs_core::{FRAME_RATE, SCREEN_HEIGHT, SCREEN_WIDTH, button};
use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::Options;
use crate::audio::AudioOutput;
use crate::gamepad::Gamepads;
use crate::osd::{Anchor, Canvas};
use crate::session::Session;
use crate::video;

/// How far emulation may fall behind real time before we stop trying to catch up.
const MAX_CATCH_UP_FRAMES: u32 = 3;
/// How long status messages stay on screen.
const TOAST_TIME: Duration = Duration::from_millis(1800);
/// Battery saves are written this often while playing, as well as on exit.
const BATTERY_FLUSH_FRAMES: u64 = 600;

const NO_ROM_HELP: &str = "CTRL+O: OPEN A ROM\nOR DROP A .NES FILE HERE";
const BACKGROUND: u32 = 0x0010_1018;

/// Messages sent to the event loop from other threads.
enum UserEvent {
    /// The file dialog closed, with the chosen ROM if any.
    RomPicked(Option<PathBuf>),
}

struct App {
    session: Option<Session>,
    audio: Option<AudioOutput>,
    samples: Vec<f32>,
    window: Option<Rc<Window>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
    proxy: EventLoopProxy<UserEvent>,
    scale: u32,
    /// Buttons held on the keyboard, per player.
    keyboard: [u8; 2],
    gamepads: Gamepads,
    ctrl_held: bool,
    paused: bool,
    muted: bool,
    dialog_open: bool,
    /// Save state slot selected with the number keys.
    slot: u8,
    toast: Option<(String, Instant)>,
    frames_left: Option<u64>,
    frames_run: u64,
    frame_time: Duration,
    next_frame: Instant,
    error: Option<String>,
}

/// Opens a window and runs until the user quits. Returns the game that was
/// loaded at that point, if any.
pub fn run(session: Option<Session>, options: &Options) -> Result<Option<Session>, String> {
    let audio = match AudioOutput::start() {
        Ok(audio) => Some(audio),
        Err(message) => {
            eprintln!("nesrs: sound disabled: {message}");
            None
        }
    };
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .map_err(|e| format!("cannot start event loop: {e}"))?;
    let mut app = App {
        session: None,
        audio,
        samples: Vec::new(),
        window: None,
        surface: None,
        proxy: event_loop.create_proxy(),
        scale: options.scale,
        keyboard: [0; 2],
        gamepads: Gamepads::new(),
        ctrl_held: false,
        paused: false,
        muted: options.mute,
        dialog_open: false,
        slot: 1,
        toast: None,
        frames_left: options.frames,
        frames_run: 0,
        frame_time: Duration::from_secs_f64(1.0 / FRAME_RATE),
        next_frame: Instant::now(),
        error: None,
    };
    if let Some(session) = session {
        app.insert(session);
    }

    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    if let Some(session) = &mut app.session {
        session.flush_battery()?;
    }
    match app.error {
        Some(message) => Err(message),
        None => Ok(app.session),
    }
}

/// Maps a keyboard key to a player (0 or 1) and one of their buttons.
fn button_for(key: KeyCode) -> Option<(usize, u8)> {
    Some(match key {
        // Player 1: arrow keys and the bottom-left letter keys.
        KeyCode::KeyX => (0, button::A),
        KeyCode::KeyZ => (0, button::B),
        KeyCode::ShiftRight | KeyCode::Tab => (0, button::SELECT),
        KeyCode::Enter => (0, button::START),
        KeyCode::ArrowUp => (0, button::UP),
        KeyCode::ArrowDown => (0, button::DOWN),
        KeyCode::ArrowLeft => (0, button::LEFT),
        KeyCode::ArrowRight => (0, button::RIGHT),
        // Player 2: WASD and the keys around it.
        KeyCode::KeyG => (1, button::A),
        KeyCode::KeyF => (1, button::B),
        KeyCode::KeyQ => (1, button::SELECT),
        KeyCode::KeyE => (1, button::START),
        KeyCode::KeyW => (1, button::UP),
        KeyCode::KeyS => (1, button::DOWN),
        KeyCode::KeyA => (1, button::LEFT),
        KeyCode::KeyD => (1, button::RIGHT),
        _ => return None,
    })
}

/// Maps the number row to a save state slot.
fn slot_for(key: KeyCode) -> Option<u8> {
    const DIGITS: [KeyCode; 10] = [
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    DIGITS.iter().position(|&digit| digit == key).map(|slot| slot as u8)
}

#[cfg(feature = "file-dialog")]
fn pick_rom() -> Option<PathBuf> {
    rfd::FileDialog::new().set_title("Open NES ROM").add_filter("NES ROM", &["nes"]).pick_file()
}

#[cfg(not(feature = "file-dialog"))]
fn pick_rom() -> Option<PathBuf> {
    None
}

impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, message: String) {
        self.error = Some(message);
        event_loop.exit();
    }

    fn title(&self) -> String {
        match &self.session {
            Some(session) => format!("{} - nesrs", session.name()),
            None => "nesrs".into(),
        }
    }

    /// Shows a short status message. The built-in font only has capitals.
    fn toast(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now() + TOAST_TIME));
        self.redraw();
    }

    fn redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    /// Makes `session` the running game.
    fn insert(&mut self, mut session: Session) {
        if let Some(audio) = &self.audio {
            session.nes.set_sample_rate(audio.sample_rate());
        }
        self.session = Some(session);
        self.paused = false;
        self.next_frame = Instant::now();
        if let Some(window) = &self.window {
            window.set_title(&self.title());
        }
    }

    /// Switches to another ROM, keeping the current game if it cannot be loaded.
    fn open_rom(&mut self, path: &Path) {
        match Session::open(path) {
            Ok(session) => {
                if let Some(old) = &mut self.session
                    && let Err(message) = old.flush_battery()
                {
                    eprintln!("nesrs: {message}");
                }
                self.insert(session);
                self.redraw();
            }
            Err(message) => {
                eprintln!("nesrs: {}: {message}", path.display());
                self.toast(format!("CANNOT OPEN ROM\n{message}"));
            }
        }
    }

    /// Opens the file dialog on another thread so the window stays responsive.
    fn show_open_dialog(&mut self) {
        if self.dialog_open {
            return;
        }
        if !cfg!(feature = "file-dialog") {
            self.toast("NO FILE DIALOG IN THIS BUILD");
            return;
        }
        self.dialog_open = true;
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let _ = proxy.send_event(UserEvent::RomPicked(pick_rom()));
        });
    }

    fn save_state(&mut self) {
        let slot = self.slot;
        let Some(session) = &mut self.session else { return };
        match session.save_state(slot) {
            Ok(()) => self.toast(format!("STATE {slot} SAVED")),
            Err(message) => {
                eprintln!("nesrs: {message}");
                self.toast("CANNOT SAVE STATE");
            }
        }
    }

    fn load_state(&mut self) {
        let slot = self.slot;
        let Some(session) = &mut self.session else { return };
        match session.load_state(slot) {
            Ok(true) => {
                self.next_frame = Instant::now();
                self.toast(format!("STATE {slot} LOADED"));
            }
            Ok(false) => self.toast(format!("STATE {slot} IS EMPTY")),
            Err(message) => {
                eprintln!("nesrs: {message}");
                self.toast("CANNOT LOAD STATE");
            }
        }
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let size =
            LogicalSize::new(SCREEN_WIDTH as u32 * self.scale, SCREEN_HEIGHT as u32 * self.scale);
        let attributes = Window::default_attributes()
            .with_title(self.title())
            .with_inner_size(size)
            .with_min_inner_size(LogicalSize::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32));
        let window = Rc::new(
            event_loop.create_window(attributes).map_err(|e| format!("cannot open window: {e}"))?,
        );
        let context = Context::new(window.clone()).map_err(|e| e.to_string())?;
        self.surface = Some(Surface::new(&context, window.clone()).map_err(|e| e.to_string())?);
        self.window = Some(window);
        self.next_frame = Instant::now();
        Ok(())
    }

    fn key(&mut self, event_loop: &ActiveEventLoop, key: KeyCode, pressed: bool, repeat: bool) {
        if self.ctrl_held {
            if pressed && !repeat && key == KeyCode::KeyO {
                self.show_open_dialog();
            }
            return;
        }
        if let Some((player, mask)) = button_for(key) {
            if pressed {
                self.keyboard[player] |= mask;
            } else {
                self.keyboard[player] &= !mask;
            }
            return;
        }
        if !pressed || repeat {
            return;
        }
        if let Some(slot) = slot_for(key) {
            self.slot = slot;
            self.toast(format!("STATE SLOT {slot}"));
            return;
        }
        match key {
            KeyCode::Escape => event_loop.exit(),
            KeyCode::KeyR => {
                if let Some(session) = &mut self.session {
                    session.nes.reset();
                    self.toast("RESET");
                }
            }
            KeyCode::KeyP if self.session.is_some() => {
                self.paused = !self.paused;
                self.next_frame = Instant::now();
                self.redraw();
            }
            KeyCode::KeyM => {
                self.muted = !self.muted;
                self.toast(if self.muted { "SOUND OFF" } else { "SOUND ON" });
            }
            KeyCode::F5 => self.save_state(),
            KeyCode::F8 => self.load_state(),
            KeyCode::F11 => {
                if let Some(window) = &self.window {
                    let fullscreen = window.fullscreen().is_none();
                    window.set_fullscreen(fullscreen.then_some(Fullscreen::Borderless(None)));
                }
            }
            _ => {}
        }
    }

    /// Emulates one frame and hands its sound to the audio device.
    fn emulate_frame(&mut self) {
        let (pads, message) = self.gamepads.poll();
        if let Some(message) = message {
            self.toast(message);
        }
        let Some(session) = &mut self.session else { return };
        let nes = &mut session.nes;
        // Applied every frame, so input survives resets and loaded states.
        for player in 0..2 {
            nes.set_buttons(player, self.keyboard[player] | pads[player]);
        }
        if let Some(audio) = &self.audio {
            // The audio device and the emulation loop run off different clocks.
            // Nudging the generated sample rate by up to 0.5% keeps the queue
            // near its target instead of slowly draining or overflowing.
            let target = audio.target_queue() as f64;
            let error = (target - audio.queued() as f64) / target;
            let ratio = 1.0 + (error * 0.005).clamp(-0.005, 0.005);
            nes.set_sample_rate((audio.sample_rate() as f64 * ratio).round() as u32);
        }
        nes.run_frame();
        self.samples.clear();
        nes.drain_audio(&mut self.samples);
        if let Some(audio) = &self.audio
            && !self.muted
        {
            audio.push(&self.samples);
        }
        self.frames_run += 1;
        if self.frames_run.is_multiple_of(BATTERY_FLUSH_FRAMES)
            && let Err(message) = session.flush_battery()
        {
            eprintln!("nesrs: {message}");
        }
    }

    fn draw(&mut self) -> Result<(), String> {
        let (Some(window), Some(surface)) = (&self.window, &mut self.surface) else {
            return Ok(());
        };
        let size = window.inner_size();
        let (Some(width), Some(height)) =
            (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return Ok(()); // minimised
        };
        surface.resize(width, height).map_err(|e| e.to_string())?;
        let mut buffer = surface.buffer_mut().map_err(|e| e.to_string())?;
        let (width, height) = (size.width as usize, size.height as usize);
        match &self.session {
            Some(session) => video::blit(session.nes.framebuffer(), &mut buffer, width, height),
            None => buffer.fill(BACKGROUND),
        }

        let text_scale = (width / SCREEN_WIDTH).min(height / SCREEN_HEIGHT).max(1);
        let mut canvas = Canvas { pixels: &mut buffer, width, height };
        if self.session.is_none() {
            canvas.text(NO_ROM_HELP, Anchor::Centre, text_scale);
        } else if self.paused {
            canvas.text("PAUSED", Anchor::Centre, text_scale);
        }
        if let Some((message, _)) = &self.toast {
            canvas.text(message, Anchor::BottomLeft, text_scale);
        }
        buffer.present().map_err(|e| e.to_string())
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none()
            && let Err(message) = self.create_window(event_loop)
        {
            self.fail(event_loop, message);
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::RomPicked(path) => {
                self.dialog_open = false;
                if let Some(path) = path {
                    self.open_rom(&path);
                }
                // Emulation kept its deadline while the dialog was up; do not
                // try to make up for any time lost.
                self.next_frame = Instant::now();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::ModifiersChanged(modifiers) => {
                self.ctrl_held = modifiers.state().control_key();
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent { physical_key: PhysicalKey::Code(key), state, repeat, .. },
                ..
            } => self.key(event_loop, key, state == ElementState::Pressed, repeat),
            WindowEvent::DroppedFile(path) => self.open_rom(&path),
            WindowEvent::Focused(false) => {
                // Key releases are not delivered while unfocused.
                self.keyboard = [0; 2];
                self.ctrl_held = false;
            }
            WindowEvent::RedrawRequested => {
                if let Err(message) = self.draw() {
                    self.fail(event_loop, message);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if self.toast.as_ref().is_some_and(|(_, expiry)| now >= *expiry) {
            self.toast = None;
            self.redraw();
        }
        if self.paused || self.session.is_none() {
            // Nothing to animate: sleep until input arrives or the toast expires.
            event_loop.set_control_flow(match &self.toast {
                Some((_, expiry)) => ControlFlow::WaitUntil(*expiry),
                None => ControlFlow::Wait,
            });
            return;
        }
        if now >= self.next_frame {
            // After a long stall (window dragged, laptop suspended) skip ahead
            // rather than fast-forwarding through the backlog.
            if now - self.next_frame > self.frame_time * MAX_CATCH_UP_FRAMES {
                self.next_frame = now;
            }
            while self.next_frame <= now {
                self.emulate_frame();
                self.next_frame += self.frame_time;
                if let Some(frames) = &mut self.frames_left {
                    *frames = frames.saturating_sub(1);
                    if *frames == 0 {
                        event_loop.exit();
                        return;
                    }
                }
            }
            self.redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}
