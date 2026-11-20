# Test ROMs

Drop test ROMs here to enable the optional accuracy tests. Nothing in this
directory is required for `cargo test` to pass; tests whose ROM is missing are
skipped.

| File | Used by | Source |
|---|---|---|
| `nestest.nes`, `nestest.log` | `crates/nesrs-core/tests/nestest.rs` | kevtris's nestest, linked from the NESdev wiki "Emulator tests" page |
