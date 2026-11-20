//! The windowed application: event handling, frame pacing and presentation.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use nesrs_core::{FRAME_RATE, Nes, SCREEN_HEIGHT, SCREEN_WIDTH, button};
use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

use crate::Options;
use crate::audio::AudioOutput;
use crate::video;

/// How far emulation may fall behind real time before we stop trying to catch up.
const MAX_CATCH_UP_FRAMES: u32 = 3;

struct App {
    nes: Nes,
    audio: Option<AudioOutput>,
    samples: Vec<f32>,
    window: Option<Rc<Window>>,
    surface: Option<Surface<Rc<Window>, Rc<Window>>>,
    title: String,
    scale: u32,
    buttons: u8,
    paused: bool,
    muted: bool,
    frames_left: Option<u64>,
    frame_time: Duration,
    next_frame: Instant,
    error: Option<String>,
}

/// Opens a window and runs the emulator until the user quits. Returns the
/// console so the caller can save its state.
pub fn run(nes: Nes, options: &Options) -> Result<Nes, String> {
    let audio = match AudioOutput::start() {
        Ok(audio) => Some(audio),
        Err(message) => {
            eprintln!("nesrs: sound disabled: {message}");
            None
        }
    };
    let name = options.rom.file_stem().map(|s| s.to_string_lossy().into_owned());
    let mut app = App {
        nes,
        audio,
        samples: Vec::new(),
        window: None,
        surface: None,
        title: format!("{} - nesrs", name.unwrap_or_default()),
        scale: options.scale,
        buttons: 0,
        paused: false,
        muted: options.mute,
        frames_left: options.frames,
        frame_time: Duration::from_secs_f64(1.0 / FRAME_RATE),
        next_frame: Instant::now(),
        error: None,
    };
    if let Some(audio) = &app.audio {
        app.nes.set_sample_rate(audio.sample_rate());
    }

    let event_loop = EventLoop::new().map_err(|e| format!("cannot start event loop: {e}"))?;
    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    match app.error {
        Some(message) => Err(message),
        None => Ok(app.nes),
    }
}

/// Maps a keyboard key to a controller button.
fn button_for(key: KeyCode) -> Option<u8> {
    Some(match key {
        KeyCode::KeyX => button::A,
        KeyCode::KeyZ => button::B,
        KeyCode::ShiftRight | KeyCode::Tab => button::SELECT,
        KeyCode::Enter => button::START,
        KeyCode::ArrowUp => button::UP,
        KeyCode::ArrowDown => button::DOWN,
        KeyCode::ArrowLeft => button::LEFT,
        KeyCode::ArrowRight => button::RIGHT,
        _ => return None,
    })
}

impl App {
    fn fail(&mut self, event_loop: &ActiveEventLoop, message: String) {
        self.error = Some(message);
        event_loop.exit();
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let size =
            LogicalSize::new(SCREEN_WIDTH as u32 * self.scale, SCREEN_HEIGHT as u32 * self.scale);
        let attributes = Window::default_attributes()
            .with_title(self.title.clone())
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
        if let Some(mask) = button_for(key) {
            if pressed {
                self.buttons |= mask;
            } else {
                self.buttons &= !mask;
            }
            self.nes.set_buttons(0, self.buttons);
            return;
        }
        if !pressed || repeat {
            return;
        }
        match key {
            KeyCode::Escape => event_loop.exit(),
            KeyCode::KeyR => self.nes.reset(),
            KeyCode::KeyP => {
                self.paused = !self.paused;
                self.next_frame = Instant::now();
            }
            KeyCode::KeyM => self.muted = !self.muted,
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
        if let Some(audio) = &self.audio {
            // The audio device and the emulation loop run off different clocks.
            // Nudging the generated sample rate by up to 0.5% keeps the queue
            // near its target instead of slowly draining or overflowing.
            let target = audio.target_queue() as f64;
            let error = (target - audio.queued() as f64) / target;
            let ratio = 1.0 + (error * 0.005).clamp(-0.005, 0.005);
            self.nes.set_sample_rate((audio.sample_rate() as f64 * ratio).round() as u32);
        }
        self.nes.run_frame();
        self.samples.clear();
        self.nes.drain_audio(&mut self.samples);
        if let Some(audio) = &self.audio
            && !self.muted
        {
            audio.push(&self.samples);
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
        video::blit(self.nes.framebuffer(), &mut buffer, size.width as usize, size.height as usize);
        buffer.present().map_err(|e| e.to_string())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none()
            && let Err(message) = self.create_window(event_loop)
        {
            self.fail(event_loop, message);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::KeyboardInput {
                event: KeyEvent { physical_key: PhysicalKey::Code(key), state, repeat, .. },
                ..
            } => self.key(event_loop, key, state == ElementState::Pressed, repeat),
            WindowEvent::Focused(false) => {
                // Key releases are not delivered while unfocused.
                self.buttons = 0;
                self.nes.set_buttons(0, 0);
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
        if self.paused {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        let now = Instant::now();
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
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}
