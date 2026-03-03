# UI Code Map (Contributor Guide)

Purpose: fast navigation for contributors working on the TUI.

Scope: `src/ui/*` and direct integration points in `src/main.rs`, `src/config.rs`, `src/docker.rs`, `src/runner.rs`.

## 1) Where To Change What

### Rendering
- Root orchestration: `src/ui/render/root.rs`
- Main shell composition (header/body/footer/cmdline): `src/ui/render/shell.rs`
- Body layout and split logic: `src/ui/render/layout.rs`
- Sidebar: `src/ui/render/sidebar.rs`
- Main list tables (containers/images/volumes/networks): `src/ui/render/tables/*`
- Details pane renderers: `src/ui/render/details/*`
- View wrappers: `src/ui/views/*`

### Key handling and interaction
- Input dispatcher: `src/ui/input/mod.rs`
- Mode-specific key handling:
  - command line: `src/ui/input/cmdline.rs`
  - global keys: `src/ui/input/global.rs`
  - mode toggles/edit mode handling: `src/ui/input/modes/*`
  - navigation/focus/sidebar/list keys: `src/ui/input/navigation.rs`, `src/ui/input/views/*`
- Key parsing/scopes: `src/ui/core/key_types.rs`
- Keymap rebuild and destructive command checks: `src/ui/core/keymap.rs`

### Async/background behavior
- Event loop: `src/ui/core/run.rs`
- Background task spawning: `src/ui/core/run_spawn.rs`
- Applying async results into app state: `src/ui/core/run_apply.rs`
- Runtime command execution helpers (ssh/local/interactive): `src/ui/core/runtime.rs`

### Commands
- Command dispatcher + module map: `src/ui/commands/mod.rs`
- Command parser/executor: `src/ui/commands/cmdline_cmd/mod.rs`
- Domain commands: `src/ui/commands/*_cmd.rs`
- Shared command helper utilities: `src/ui/commands/common.rs`

### Templates and deployment
- Template state/indexing/loading: `src/ui/features/templates/state.rs`
- Template operations:
  - shared ops helpers: `src/ui/features/templates/ops/common.rs`
  - file operations: `src/ui/features/templates/ops/template_fs.rs`
  - export helpers: `src/ui/features/templates/ops/export.rs`
- Deployment orchestration actions: `src/ui/ui_actions.rs`
- Lower-level deploy/update operations: `src/ui/core/background_ops.rs`

### Persistence and config
- UI config + local state persistence: `src/ui/state/persistence.rs`
- Runtime shell/ui state types: `src/ui/state/shell_types.rs`
- Cross-view derived state (image update view state): `src/ui/state/image_updates.rs`

## 2) Typical Change Recipes

### Add or change a command
1. Implement behavior in `src/ui/commands/<domain>_cmd.rs`.
2. Wire alias/dispatch in `src/ui/commands/cmdline_cmd/mod.rs` or `src/ui/commands/mod.rs`.
3. Update help text in `src/ui/render/help/mod.rs` and `src/ui/render/help/sections.rs`.
4. Add/adjust tests in `src/tests/ui_tests.rs`.

### Add a new keybinding
1. Add default binding in `src/ui/keys.inc.rs` (default keymap builder).
2. Ensure scope handling is correct in `src/ui/core/key_types.rs`.
3. Verify dispatcher path in `src/ui/input/global.rs`, `src/ui/input/navigation.rs`, or `src/ui/input/views/*`.
4. Update help in `src/ui/render/help/mod.rs` and `src/ui/render/help/sections.rs`.

### Add/adjust a list column
1. Table structure/columns in `src/ui/render/tables/*` (or view-specific renderer).
2. Details data in `src/ui/render/details/*` if needed.
3. Derived data/state updates in `src/ui/features/*` or `src/ui/state/*` if required.

### Add background refresh data
1. Spawn request in `src/ui/core/run_spawn.rs`.
2. Channel receive/apply in `src/ui/core/run_apply.rs`.
3. Render path in `src/ui/render/*` and/or `src/ui/views/*`.
4. Persist additional state in `src/ui/state/persistence.rs` if needed.

## 3) Boundaries (Keep These Stable)

- UI render modules should not run commands directly; they read `App` and draw.
- Input modules decide intent; commands/actions execute behavior.
- Async workers return results through channels; `run_apply` mutates state.
- `ui/mod.rs` is wiring + shared types, not business logic.

## 4) Quick Entry Points

- Start from user action (key): `src/ui/input/mod.rs`
- Start from command text (`:...`): `src/ui/commands/cmdline_cmd/mod.rs`
- Start from visual issue in a pane: `src/ui/render/shell.rs` -> related `src/ui/render/*`
- Start from template deploy issue: `src/ui/commands/templates_cmd.rs` -> `src/ui/ui_actions.rs` -> `src/ui/core/background_ops.rs`

## 5) Validation Checklist

- `cargo test`
- Manual smoke:
  - sidebar + focus switching
  - one action in each main domain (stacks/containers/templates/registries)
  - help view updated when key/command behavior changes
