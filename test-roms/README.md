# Test ROMs

Drop test ROMs here to enable the optional accuracy tests. Nothing in this
directory is required for `cargo test` to pass; tests whose ROM is missing are
skipped.

| File | Used by | Source |
|---|---|---|
| `nestest.nes`, `nestest.log` | `crates/nesrs-core/tests/nestest.rs` | kevtris's nestest: <https://www.qmtpro.com/~nes/misc/nestest.nes> and <https://www.qmtpro.com/~nes/misc/nestest.log> |

The files are git-ignored: their licences do not clearly allow redistribution,
so each developer downloads them locally.

## Results

| Test | Status |
|---|---|
| nestest (all 8991 logged instructions: registers, flags, cycle count and PPU position) | Pass |

## Checking a game by hand

Headless mode can play a scripted input sequence and save a screenshot, which
is handy for reproducing a rendering bug:

```bash
cargo run --release -- --headless --frames 1000 --input 420:S,430:,700:R --screenshot out.ppm game.nes
```
