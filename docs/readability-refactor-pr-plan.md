# Readability Refactor PR Plan

Goal: make the codebase easier to read, navigate, and review without changing behavior.

Scope: `src/ui/*` first, then cross-cutting cleanup in `src/*`.

Principles:
- small, reversible PRs
- no feature changes during structure PRs
- each PR must pass `cargo test`
- move first, optimize second

## Current Status (2026-02-27)

Completed:
- PR 1 (`ui/mod.rs` thinning): done in multiple steps (state/shell type extraction and reduced central wiring burden).
- PR 2 (`core/run.rs` split, spawn): done (`run_spawn` extraction).
- PR 3 (`core/run.rs` split, apply handlers): done (`run_apply` extraction).
- PR 4 (input split by context): done (dispatcher + focused input modules).
- PR 5 (templates ops split): done (ops tree split by concern).
- PR 6 (details render split): done (view-focused detail render modules).
- PR 7 (command normalization): mostly done (shared helper patterns, consistent handler shape).
- PR 8 (visibility tightening): done.
- Optional PR 9 (focused contributor docs): done (`docs/code-map-ui.md`).

Open:
- Follow-up cleanup only (no blocker for this readability plan):
  - continue reducing remaining broad `pub(in crate::ui)` where feasible
  - keep pruning re-exports in `ui/mod.rs` as modules are touched

Notes:
- Refactor sequence so far stayed behavior-preserving and test-backed.
- Remaining work is mostly API-surface cleanup.

## PR 1: Thin `ui/mod.rs` Further

Target:
- Keep `src/ui/mod.rs` as wiring + top-level type declarations only.
- Move key parsing/key types to `ui/core/key_types.rs`:
  - `KeySpec`, `KeyScope`, `KeyCodeNorm`
  - `parse_key_spec`, `parse_scope`, default keymap builder

Why:
- `mod.rs` is still a central navigation bottleneck.

Acceptance:
- `mod.rs` size reduced significantly.
- No behavior change in key bindings.
- `cargo test` green.

## PR 2: Split `core/run.rs` (Stage 1)

Target:
- Extract task spawning into `src/ui/core/run_spawn.rs`.
- Keep event loop in `run.rs`.

Why:
- `run.rs` is the largest file and hardest to reason about.

Acceptance:
- `run.rs` line count reduced.
- Spawned tasks are easier to locate by name.
- `cargo test` green.

## PR 3: Split `core/run.rs` (Stage 2)

Target:
- Extract async receive/apply handlers into `src/ui/core/run_apply.rs`:
  - action result apply
  - dashboard result apply
  - inspect/logs/image update apply

Why:
- Event handling and state mutation are currently mixed.

Acceptance:
- Main loop reads as orchestration only.
- Apply logic grouped by event domain.
- `cargo test` green.

## PR 4: Split `input.rs` by mode/context

Target:
- Create:
  - `src/ui/input/nav.rs`
  - `src/ui/input/cmdline.rs`
  - `src/ui/input/logs.rs`
  - `src/ui/input/inspect.rs`
- Keep a small dispatcher in `src/ui/input.rs`.

Why:
- Current input handling is too large for fast comprehension.

Acceptance:
- Clear one-file-per-input-context layout.
- Keypath lookup is straightforward.
- `cargo test` green.

## PR 5: Split templates ops by concern

Target:
- Break `src/ui/features/templates/ops.rs` into:
  - `fs.rs` (create/delete/load template files)
  - `scaffold.rs` (default content/templates)
  - `deploy.rs` (deploy/redeploy helpers)
  - `git.rs` (template git metadata helpers)

Why:
- Templates module mixes filesystem, deployment, and git concerns.

Acceptance:
- Each file has one dominant responsibility.
- No command behavior changes.
- `cargo test` green.

## PR 6: Render readability pass

Target:
- Split `src/ui/render/details.rs` into per-view renderers:
  - `details_containers.rs`
  - `details_images.rs`
  - `details_volumes.rs`
  - `details_networks.rs`
  - `details_templates.rs`
- Keep table/layout helpers shared.

Why:
- Details rendering is currently a large mixed view.

Acceptance:
- View-specific rendering code is isolated.
- Same visual output as before.
- `cargo test` green + manual smoke check for all main views.

## PR 7: Command module normalization

Target:
- Standardize command file shape:
  - parse/validate at top
  - execute in middle
  - helper functions at bottom
- Add short module docs to each command file.
- Ensure aliases live in one place (`commands/mod.rs` or dedicated alias map).

Why:
- Commands are readable individually but not yet uniform.

Acceptance:
- Consistent layout across command modules.
- Alias behavior unchanged.
- `cargo test` green.

## PR 8: Public API and visibility tightening

Target:
- Replace broad `pub(in crate::ui)` where possible with `pub(super)` or private.
- Add `pub use` only for explicit module APIs.
- Remove obsolete re-exports.

Why:
- Lower coupling and make ownership boundaries explicit.

Acceptance:
- Reduced symbol surface.
- No feature regression.
- `cargo test` green.

## Optional PR 9: Focused docs for contributors

Target:
- Add `docs/code-map-ui.md` with:
  - where to change rendering
  - where to change key handling
  - where to change async/background behavior
  - where to change template/deploy behavior

Why:
- Fast onboarding for contributors before GitHub release.

Acceptance:
- New contributor can locate a change area in under 2 minutes.

## Suggested Execution Order

1. PR 1
2. PR 2
3. PR 3
4. PR 4
5. PR 5
6. PR 6
7. PR 7
8. PR 8
9. Optional PR 9

## Definition of Done (for this plan)

- No behavior regressions in core flows (dashboard, stacks, containers, templates, registries).
- `cargo test` green after every PR.
- Largest files (`run.rs`, `input.rs`, `templates/ops.rs`, `render/details.rs`) reduced and split.
- `ui/mod.rs` is primarily topology/wiring.
