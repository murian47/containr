# UI `mod.rs` Refactor Plan

Status: active (partially completed).
Current companion docs:
- `docs/readability-refactor-pr-plan.md` (execution status)
- `docs/code-map-ui.md` (contributor navigation)

Goal: reduce `src/ui/mod.rs` from a monolith to a thin entrypoint that wires modules together.

## Current pain points
- `mod.rs` mixes concerns:
  - type declarations (`App`, enums, state structs)
  - helper/parsing utilities
  - large `impl App` with unrelated domains
  - dashboard data collection/parsing
  - deploy/registry/crypto helpers
  - runtime orchestration (`run_tui`, async tasks, channels)
- This slows down navigation, review, and safe edits.

## Target architecture
- `mod.rs` contains only:
  - module declarations and re-exports
  - high-level entrypoint wiring
- Feature logic is split by domain and responsibility:
  - `ui/app/*` for `impl App` methods
  - `ui/runtime/*` for terminal/event loop/task orchestration
  - `ui/services/*` for docker/registry/deploy/dashboard data logic
  - `ui/types/*` (or focused files) for shared state/types

## Execution phases

### Phase 1: Low-risk extraction (no behavior change)
1. Move foundational small units out of `mod.rs`:
   - `CmdHistory`
   - text-edit helpers (`clamp_cursor_to_text`, insert/delete/backspace helpers)
2. Keep existing call sites via re-exports to avoid churn.
3. Validate with `cargo test`.

### Phase 2: App method grouping
1. Split `impl App` into thematic files:
   - `app/logging.rs`
   - `app/selection.rs`
   - `app/tree.rs`
   - `app/logs.rs`
   - `app/inspect.rs`
   - `app/templates.rs`
   - `app/dashboard.rs`
2. Move methods without changing signatures.
3. Validate after each chunk.

### Phase 3: Service extraction
1. Move non-UI operations out of `mod.rs`:
   - dashboard command + parser
   - deploy/update/push flows
   - registry auth/test helpers
   - age crypto helpers
2. Place into `ui/services/*` with typed interfaces.
3. Keep UI-facing behavior unchanged.

### Phase 4: Runtime extraction
1. Move terminal/runtime orchestration to `ui/runtime/*`:
   - terminal setup/restore
   - interactive command launchers
   - async task spawning/channel pump logic
2. Leave a small coordinator in `mod.rs`.

### Phase 5: Final cleanup
1. Remove stale re-exports and duplicated helpers.
2. Ensure module ownership is obvious and documented.
3. Keep `mod.rs` thin (target: well below current size).

## Rules during migration
- No behavior changes unless explicitly intended.
- Small, reversible commits.
- Run `cargo test` after each meaningful step.
- Prefer moving code first, then refactoring internals in follow-up commits.
