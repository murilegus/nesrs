//! Desktop frontend for nesrs: a window, keyboard input and sound output.

mod app;
mod audio;
mod gamepad;
mod osd;
mod session;
mod video;

use std::path::PathBuf;
use std::process::ExitCode;

use nesrs_core::button;
use session::Session;

const USAGE: &str = "\
Usage: nesrs [OPTIONS] [ROM]

Without a ROM, the window opens empty: press Ctrl+O or drop a file on it.

Options:
  --scale <N>          Initial window scale (default 3)
  --mute               Start with sound off
  --frames <N>         Quit after emulating N frames
  --screenshot <PATH>  Write the last frame as a PPM image on exit
  --headless           Run without a window or sound (use with --frames)
  --input <SCRIPT>     Scripted input for --headless: FRAME:BUTTONS,... where
                       BUTTONS are letters from UDLRAB, S (Start), E (Select);
                       e.g. 120:S,125:,300:RA holds Start for frames 120-124
  -h, --help           Show this help

Controls:
                Player 1          Player 2
  D-pad         Arrow keys        W A S D
  A / B         X / Z             G / F
  Start         Enter             E
  Select        Right Shift, Tab  Q

  The first two game controllers plugged in also drive players 1 and 2.

  R  Reset    P  Pause     M  Mute     F11  Fullscreen    Esc  Quit
  Ctrl+O  Open ROM         F5  Save state        F8  Load state
  0-9     Select save state slot (default 1)";

/// Parsed command-line options.
pub struct Options {
    pub rom: Option<PathBuf>,
    pub scale: u32,
    pub mute: bool,
    pub frames: Option<u64>,
    pub screenshot: Option<PathBuf>,
    pub headless: bool,
    /// Controller 1 state changes as `(frame, buttons)`, sorted by frame.
    pub input: Vec<(u64, u8)>,
}

/// Parses an `--input` script such as `120:S,125:,300:RA`.
fn parse_input(script: &str) -> Result<Vec<(u64, u8)>, String> {
    let mut events = Vec::new();
    for entry in script.split(',') {
        let (frame, letters) =
            entry.split_once(':').ok_or(format!("--input entry '{entry}' needs FRAME:BUTTONS"))?;
        let frame = frame.parse().map_err(|_| format!("--input: bad frame number '{frame}'"))?;
        let mut buttons = 0;
        for letter in letters.chars() {
            buttons |= match letter {
                'U' => button::UP,
                'D' => button::DOWN,
                'L' => button::LEFT,
                'R' => button::RIGHT,
                'A' => button::A,
                'B' => button::B,
                'S' => button::START,
                'E' => button::SELECT,
                other => return Err(format!("--input: unknown button '{other}'")),
            };
        }
        events.push((frame, buttons));
    }
    events.sort_by_key(|&(frame, _)| frame);
    Ok(events)
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut options = Options {
        rom: None,
        scale: 3,
        mute: false,
        frames: None,
        screenshot: None,
        headless: false,
        input: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--scale" => {
                options.scale = value("--scale")?
                    .parse()
                    .ok()
                    .filter(|n| (1..=16).contains(n))
                    .ok_or("--scale must be a number from 1 to 16")?;
            }
            "--mute" => options.mute = true,
            "--frames" => {
                let frames = value("--frames")?.parse().map_err(|_| "--frames must be a number")?;
                options.frames = Some(frames);
            }
            "--screenshot" => options.screenshot = Some(value("--screenshot")?.into()),
            "--headless" => options.headless = true,
            "--input" => options.input = parse_input(&value("--input")?)?,
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            _ if options.rom.is_some() => return Err("only one ROM can be loaded".into()),
            _ => options.rom = Some(PathBuf::from(arg)),
        }
    }
    if options.headless && (options.frames.is_none() || options.rom.is_none()) {
        return Err("--headless needs a ROM and --frames".into());
    }
    Ok(Some(options))
}

fn run(options: Options) -> Result<(), String> {
    let session = match &options.rom {
        Some(rom) => Some(Session::open(rom).map_err(|e| format!("{}: {e}", rom.display()))?),
        None => None,
    };

    let mut session = if options.headless {
        let mut session = session.expect("checked while parsing arguments");
        let mut input = options.input.iter().peekable();
        for frame in 0..options.frames.unwrap_or(0) {
            while let Some(&(_, buttons)) = input.next_if(|&&(at, _)| at <= frame) {
                session.nes.set_buttons(0, buttons);
            }
            session.nes.run_frame();
        }
        Some(session)
    } else {
        app::run(session, &options)?
    };

    let Some(session) = &mut session else {
        return Ok(());
    };
    if let Some(path) = &options.screenshot {
        video::write_ppm(path, session.nes.framebuffer())
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    session.flush_battery()
}

fn main() -> ExitCode {
    let options = match parse_args() {
        Ok(Some(options)) => options,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("nesrs: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("nesrs: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_scripts_parse_and_sort() {
        assert_eq!(
            parse_input("300:RA,120:S,125:").unwrap(),
            [(120, button::START), (125, 0), (300, button::RIGHT | button::A)]
        );
        assert!(parse_input("12").is_err());
        assert!(parse_input("x:A").is_err());
        assert!(parse_input("1:Q").is_err());
    }
}
