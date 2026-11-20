# Contributing

## Branches

`main` is always buildable and passing tests. Work happens on short-lived
branches merged into `main`:

| Prefix | For |
|---|---|
| `feat/` | New functionality |
| `fix/` | Bug fixes |
| `docs/` | Documentation only |
| `chore/` | Tooling, CI, dependencies |
| `refactor/` | Restructuring without behaviour change |

## Commits

[Conventional Commits](https://www.conventionalcommits.org) with a scope where
it helps: `feat(ppu): ...`, `fix(mmc3): ...`, `docs: ...`.

## Before pushing

```bash
cargo fmt --all
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

Emulation fixes should come with a test: a unit test next to the code, or a
small hand-assembled ROM in `crates/nesrs-core/tests/`.

## ROMs

Never commit commercial ROMs. `.gitignore` blocks `*.nes` outside `test-roms/`,
and only freely redistributable test ROMs belong there.
