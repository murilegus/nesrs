# Architecture

nesrs is split into an emulation **core** and **frontends**. The core knows
nothing about windows, files or sound devices; a frontend owns those and
drives the core once per video frame.

```
        ROM bytes, buttons                 pixels, audio samples
frontend ─────────────────▶  nesrs-core  ─────────────────────────▶ frontend
```

Adding a platform (web, mobile, libretro) means adding a frontend crate. Adding
a console (SNES) means adding a core crate; frontends can then host both.

## The core (`crates/nesrs-core`)

| Module | Responsibility |
|---|---|
| `cpu` | 6502 instruction execution, interrupts |
| `bus` | CPU address decoding; clocks everything else |
| `ppu` | Video: registers, background and sprite rendering |
| `apu` | Audio: five channels, frame counter, resampling |
| `cartridge` | iNES / NES 2.0 parsing |
| `mapper` | Cartridge bank-switching hardware |
| `controller` | Standard joypad shift register |
| `state` | Save state format and the reader/writer components sync through |
| `nes` | The public `Nes` type tying it together |

### Timing model

The CPU is the master clock. Every memory access the CPU makes is one CPU
cycle, including the dummy reads real hardware performs, and each access first
advances the rest of the system by one cycle:

```
cpu.step()
  └─ bus.read(addr) / bus.write(addr, value)
       ├─ ppu.tick() x3      (3 PPU dots per CPU cycle)
       ├─ apu.tick() x1
       └─ the access itself
```

This keeps the PPU and APU within one CPU cycle of the CPU at all times
without a scheduler, which is enough for mid-frame scroll changes, sprite-0
timing and raster effects.

### Known simplifications

- Sprites for a scanline are evaluated and fetched all at once at dot 257,
  rather than over dots 65-320. Sprite overflow lacks the hardware's bug.
- MMC3's IRQ counter is clocked once per scanline at dot 260 instead of by
  watching PPU address line A12.
- Interrupts are polled between instructions, not on the exact cycle.
- DMC sample fetches always steal 3 CPU cycles.
- MMC1 does not ignore writes on consecutive cycles.

### Save states

Every component has one `sync(&mut self, &mut State)` method that lists its
fields. The same method saves and loads, depending on how the `State` was
created, so the two directions cannot get out of step. A finished state is:

```
"NESRSST" | version | ROM hash (8) | component data ... | checksum (8)
```

The layout is positional, so any change to a `sync` method must bump `VERSION`
in `state.rs`. The framebuffer and the audio resampler are deliberately left
out: one is redrawn within a frame, the other belongs to the host.

## The desktop frontend (`crates/nesrs`)

- `app` — winit event loop. Emulates frames against a wall-clock deadline
  (60.0988 Hz) and presents them with softbuffer.
- `audio` — cpal output stream fed from a queue. The core's sample rate is
  nudged by up to 0.5% each frame to hold the queue near 60 ms, which absorbs
  the drift between the sound card's clock and the system clock.
- `session` — a loaded game and its files (battery save, save state slots).
- `osd` — status messages drawn with a built-in bitmap font.
- `video` — scaling and PPM screenshots.

## The web frontend (`crates/nesrs-web`)

The Rust side is a thin wasm-bindgen wrapper (`Emulator`) around `Nes`. All
browser work is plain JavaScript in `www/`, with no bundler or dependencies:

- `main.js` — loads ROMs, maps keys and touch buttons, runs frames from
  `requestAnimationFrame` against a 60.0988 Hz clock (displays refresh at many
  rates), and stores saves in `localStorage`.
- `audio-worklet.js` — an `AudioWorklet` that plays queued samples and reports
  its queue length, which `main.js` uses for the same rate control as desktop.

`scripts/build-web.sh` compiles the crate and writes the generated glue to
`www/pkg/`, which is not checked in.
