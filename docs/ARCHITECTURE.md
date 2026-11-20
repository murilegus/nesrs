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

## The desktop frontend (`crates/nesrs`)

- `app` — winit event loop. Emulates frames against a wall-clock deadline
  (60.0988 Hz) and presents them with softbuffer.
- `audio` — cpal output stream fed from a queue. The core's sample rate is
  nudged by up to 0.5% each frame to hold the queue near 60 ms, which absorbs
  the drift between the sound card's clock and the system clock.
- `video` — scaling and PPM screenshots.
