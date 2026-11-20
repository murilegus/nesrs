# nesrs

A NES emulator written in Rust, built as a portable emulation core with thin
per-platform frontends.

> **Status: early.** The core passes its own unit and system tests, but it has
> not yet been validated against the standard accuracy test ROMs or a broad
> set of games. Expect bugs.

## Features

- **CPU** — cycle-stepped 6502 (Ricoh 2A03) with all official and unofficial opcodes
- **PPU** — dot-based background rendering with accurate scrolling, sprites,
  sprite-0 hit, colour emphasis
- **APU** — both pulse channels, triangle, noise and DMC, with the frame counter and IRQs
- **Mappers** — NROM (0), MMC1 (1), UxROM (2), CNROM (3), MMC3 (4), AxROM (7)
- **Desktop frontend** — keyboard input, sound, integer scaling, fullscreen,
  battery saves (`.sav` next to the ROM)

NTSC only for now.

## Building

You need a [Rust toolchain](https://rustup.rs) (1.85 or newer).

On Linux, sound needs the ALSA development files:

```bash
sudo apt install pkgconf libasound2-dev
```

Then:

```bash
cargo run --release -- path/to/game.nes
```

To build without sound (and without the ALSA dependency):

```bash
cargo run --release --no-default-features -- path/to/game.nes
```

## Usage

```
nesrs [OPTIONS] <ROM>

  --scale <N>          Initial window scale (default 3)
  --mute               Start with sound off
  --frames <N>         Quit after emulating N frames
  --screenshot <PATH>  Write the last frame as a PPM image on exit
  --headless           Run without a window or sound (use with --frames)
```

| Key | Action |
|---|---|
| Arrow keys | D-pad |
| `X` / `Z` | A / B |
| `Enter` | Start |
| `Right Shift` or `Tab` | Select |
| `R` | Reset |
| `P` | Pause |
| `M` | Mute |
| `F11` | Fullscreen |
| `Esc` | Quit |

## Project layout

| Crate | Purpose |
|---|---|
| [`crates/nesrs-core`](crates/nesrs-core) | The emulator itself. No dependencies, no I/O, no `unsafe`. |
| [`crates/nesrs`](crates/nesrs) | Desktop frontend (winit, softbuffer, cpal). |

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the pieces fit together.

## Testing

```bash
cargo test --workspace
```

Extra accuracy tests run when their ROMs are present; see
[test-roms/README.md](test-roms/README.md).

## Roadmap

- Validate against nestest, blargg's and other test ROM suites
- Save states, second controller, gamepad support
- Web frontend (WebAssembly)
- More mappers, PAL timing
- A SNES core alongside the NES one

## ROMs

No games are included. Commercial ROMs are copyrighted: use homebrew, test
ROMs, or dumps of cartridges you own.

## Licence

[MIT](LICENSE)
