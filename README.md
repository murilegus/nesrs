# nesrs

A NES emulator written in Rust, built as a portable emulation core with thin
per-platform frontends.

> **Status: early.** The CPU matches the nestest reference log cycle for cycle, and
> Contra (UxROM) boots and plays, but the PPU and APU have not yet been validated
> against accuracy test ROMs or a broad set of games. Expect bugs.

## Features

- **CPU** — cycle-stepped 6502 (Ricoh 2A03) with all official and unofficial opcodes
- **PPU** — dot-based background rendering with accurate scrolling, sprites,
  sprite-0 hit, colour emphasis
- **APU** — both pulse channels, triangle, noise and DMC, with the frame counter and IRQs
- **Mappers** — NROM (0), MMC1 (1), UxROM (2), CNROM (3), MMC3 (4), AxROM (7)
- **Save states** — ten slots per game, checked against the ROM they belong to
- **Desktop frontend** — keyboard input, sound, integer scaling, fullscreen,
  open ROMs from a file dialog or by drag and drop, battery saves
- **Web frontend** — the same core compiled to WebAssembly, with touch
  controls for phones and saves kept in the browser

NTSC only for now.

## Building

You need a [Rust toolchain](https://rustup.rs) (1.92 or newer).

On Linux, sound needs the ALSA development files:

```bash
sudo apt install pkgconf libasound2-dev
```

Then:

```bash
cargo run --release -- path/to/game.nes
```

Optional features, both on by default: `audio` (sound, needs ALSA on Linux)
and `file-dialog` (the `Ctrl+O` dialog). To build without either:

```bash
cargo run --release --no-default-features -- path/to/game.nes
```

## Usage (desktop)

```
nesrs [OPTIONS] [ROM]

  --scale <N>          Initial window scale (default 3)
  --mute               Start with sound off
  --frames <N>         Quit after emulating N frames
  --screenshot <PATH>  Write the last frame as a PPM image on exit
  --headless           Run without a window or sound (use with --frames)
  --input <SCRIPT>     Scripted input for --headless, e.g. 120:S,125:,300:RA
```

Started without a ROM, the window opens empty and waits for one.

| Key | Action |
|---|---|
| Arrow keys | D-pad |
| `X` / `Z` | A / B |
| `Enter` | Start |
| `Right Shift` or `Tab` | Select |
| `R` | Reset |
| `P` | Pause |
| `M` | Mute |
| `Ctrl+O` | Open a ROM |
| `F5` / `F8` | Save / load state |
| `0`-`9` | Select the save state slot (default 1) |
| `F11` | Fullscreen |
| `Esc` | Quit |

Files are kept next to the ROM: `game.sav` for battery-backed cartridge RAM
(written every ten seconds and on exit) and `game.state0` to `game.state9`
for save states.

Dropping a file onto the window works on X11, Windows and macOS; the windowing
library does not support it on Wayland yet, so use `Ctrl+O` there.

## Web frontend

The web build needs the WebAssembly target and the wasm-bindgen tool, once:

```bash
rustup target add wasm32-unknown-unknown
```

```bash
cargo install wasm-bindgen-cli --version 0.2.130 --locked
```

Build it, then serve the resulting static site with anything:

```bash
./scripts/build-web.sh
```

```bash
python3 -m http.server -d crates/nesrs-web/www 8080
```

Open <http://localhost:8080>, then pick a ROM or drop one on the page. Keys
are the same as on desktop, and touch devices get an on-screen controller.
ROMs are read locally and never uploaded; save states and battery saves live
in the browser's `localStorage`. Adding `?rom=some/file.nes` to the address
loads a ROM hosted next to the page.

`crates/nesrs-web/www` is a plain static site, so it can be published on any
static host as it is.

## Project layout

| Crate | Purpose |
|---|---|
| [`crates/nesrs-core`](crates/nesrs-core) | The emulator itself. No dependencies, no I/O, no `unsafe`. |
| [`crates/nesrs`](crates/nesrs) | Desktop frontend (winit, softbuffer, cpal). |
| [`crates/nesrs-web`](crates/nesrs-web) | WebAssembly bindings and the web page (`www/`). |

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the pieces fit together.

## Testing

```bash
cargo test --workspace
```

Extra accuracy tests run when their ROMs are present; see
[test-roms/README.md](test-roms/README.md).

## Roadmap

- Validate against blargg's and other test ROM suites (nestest already passes)
- Second controller, gamepad support, configurable keys
- More mappers, PAL timing
- A SNES core alongside the NES one

## ROMs

No games are included. Commercial ROMs are copyrighted: use homebrew, test
ROMs, or dumps of cartridges you own.

## Licence

[MIT](LICENSE)
