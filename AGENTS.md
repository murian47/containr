# Agent Instructions – containr

## Project Scope
- TUI-based Docker/stack/template manager in Rust.
- Main path: repository root `.`.
- Network access: follow the CLI constraints for this session (currently: restricted).

## Build & Test
- Local build/test: `cargo test`
- Run (debug): `cargo run`
- Do not run destructive Git commands unless explicitly requested; only bump the version for code or theme changes.

## Structure Notes
- The UI lives under `src/ui/` with split modules for rendering, input, state, features, and commands.
- Domain, runner, SSH, and Docker logic are still intertwined with the UI in places; check the current docs before larger refactors.
- Themes live under `themes/`; templates live under `~/.config/containr/templates`.

## Working Rules
- User-facing chat may be in German, but repository files, documentation, and code comments should be in English unless explicitly requested otherwise.
- Ask when a destructive action may be required.
- Perform refactors incrementally and keep the application runnable.
- Before committing code changes, bump the patch version by 1 unless the version was already explicitly set for the same change set.
- After larger changes, run tests (`cargo test`) before continuing or committing.
- Keep utility helpers in `src/ui/render/utils.rs` for now; further splitting into text/scroll/fs helpers can happen later.
