//! Desktop frontend for nesrs: a window, keyboard input and sound output.

mod app;
mod audio;
mod video;

use std::path::PathBuf;
use std::process::ExitCode;

use nesrs_core::Nes;

const USAGE: &str = "\
Usage: nesrs [OPTIONS] <ROM>

Options:
  --scale <N>          Initial window scale (default 3)
  --mute               Start with sound off
  --frames <N>         Quit after emulating N frames
  --screenshot <PATH>  Write the last frame as a PPM image on exit
  --headless           Run without a window or sound (use with --frames)
  -h, --help           Show this help

Controls:
  Arrow keys  D-pad        X  A        Z  B
  Enter       Start        Right Shift / Tab  Select
  R  Reset    P  Pause     M  Mute     F11  Fullscreen    Esc  Quit";

/// Parsed command-line options.
pub struct Options {
    pub rom: PathBuf,
    pub scale: u32,
    pub mute: bool,
    pub frames: Option<u64>,
    pub screenshot: Option<PathBuf>,
    pub headless: bool,
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut rom = None;
    let mut options = Options {
        rom: PathBuf::new(),
        scale: 3,
        mute: false,
        frames: None,
        screenshot: None,
        headless: false,
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
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            _ if rom.is_some() => return Err("only one ROM can be loaded".into()),
            _ => rom = Some(PathBuf::from(arg)),
        }
    }
    options.rom = rom.ok_or("no ROM file given")?;
    if options.headless && options.frames.is_none() {
        return Err("--headless needs --frames".into());
    }
    Ok(Some(options))
}

fn run(options: Options) -> Result<(), String> {
    let rom = std::fs::read(&options.rom)
        .map_err(|e| format!("cannot read {}: {e}", options.rom.display()))?;
    let mut nes = Nes::new(&rom).map_err(|e| format!("{}: {e}", options.rom.display()))?;

    let save_path = options.rom.with_extension("sav");
    if nes.save_ram().is_some()
        && let Ok(data) = std::fs::read(&save_path)
    {
        nes.load_save_ram(&data);
    }

    let nes = if options.headless {
        for _ in 0..options.frames.unwrap_or(0) {
            nes.run_frame();
        }
        nes
    } else {
        app::run(nes, &options)?
    };

    if let Some(path) = &options.screenshot {
        video::write_ppm(path, nes.framebuffer())
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    if let Some(data) = nes.save_ram() {
        std::fs::write(&save_path, data)
            .map_err(|e| format!("cannot write {}: {e}", save_path.display()))?;
    }
    Ok(())
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
