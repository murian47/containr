# containr UI/Logic Separation (Incremental Plan)

Goal: decouple UI (ratatui) from domain logic so we can maintain TUI easily and prepare future GUIs (Swift/Avalonia/etc). Each step should leave the app runnable.

## Principles
- Keep the app running after every step (small, reversible commits).
- Move shared logic into UI-agnostic modules/crates (no ratatui types).
- UI renders *state*; services/controllers mutate state via events/messages.
- Avoid “half moves”: migrate a feature end-to-end before tackling the next.

## Phase 1: Stabilize TUI Structure
1) Finish render decomposition: lists, details, overlays, helpers already split; keep shrinking `render.inc.rs` into focused modules without changing behavior.
2) Isolate UI state vs. domain data in `App`: mark UI-only fields (scroll, focus, selection) vs. domain (containers, stacks, templates, registries, actions, errors).
3) Introduce a thin ViewModel layer (structs independent of ratatui) for things like “display rows” (status text, update markers, inflight/error flags) so rendering only maps ViewModel → widgets.

## Phase 2: Extract Domain Types
1) Define domain models (Container, Stack, Image, Volume, Network, Template, Registry, Actions/Errors/Statuses) in a UI-free module (new `domain` mod or crate).
2) Define “op state” structs for long-running actions (inflight markers, last errors, deploy/check results, update status) in that module.
3) Replace direct ratatui use in data structs with plain Rust types (no Style, no Span).

## Phase 3: Service/Controller Layer
1) Introduce service traits for:
   - Docker interactions (list, inspect, pull, recreate, start/stop/remove, image update checks).
   - SSH transport (exec, copy).
   - Template/Registry management (deploy, check, test auth, git autocommit).
2) Implement current logic behind those traits (wrapping existing runner/task code), returning typed results/events (no UI).
3) Route commands/keys to services via a thin controller that emits events; UI subscribes and updates state.

## Phase 4: Eventing and State Updates
1) Replace ad-hoc App mutations with an event queue:
   - Input → Command → Service → Event (success/failure/progress).
   - Event reducer updates App state (domain + UI bits).
2) Keep ratatui rendering read-only over state.

## Phase 5: Cleanup & Prep for Other UIs
1) Move TUI-only helpers (styles, keymaps, widgets) behind a `ui::tui` module; keep domain/services UI-agnostic.
2) Add a minimal “headless” harness that exercises services without rendering (for tests/CI).
3) Document the public surface of services/events/state for alternative frontends.

## Sequencing (runnable after each step)
1) Keep TUI rendering intact while moving code into modules (Phase 1).
2) Introduce domain structs and migrate one area at a time (e.g., Containers → Images → Networks → Templates → Registries).
3) Add service traits and adapt existing code feature-by-feature; keep the old path until the new one works.
4) Flip rendering to ViewModels once a feature’s data/events are migrated.
5) Remove legacy hooks after parity is confirmed.

## Short-term fast-track (keep app runnable)
- Catalog render.inc.rs helpers: split pure formatting/badge/table helpers from logic-heavy parts.
- Extract pure render helpers into dedicated modules (`render/format.rs`, `render/badges.rs`, `render/table.rs`), adjust call sites, then `cargo test`.
- Move status/marker/update derivation into UI-free state/domain helpers so rendering consumes prepared data only.
- Refine `render/utils.rs` gradually (text/scroll/fs), avoid new monoliths; remove duplicates.
- After each chunk: document briefly, keep version patch-bumped for code changes, run tests, then commit.

## Helper catalog (first pass – render.inc.rs)
- Pure formatting/render candidates (move to `render/format.rs`, `render/badges.rs`, `render/table.rs`):
  - text/layout: `wrap_text`, `pad_right`, `truncate_start`, `spinner_char`, `loading_spinner`
  - units/bars: `format_bytes_short`, `bar_spans_threshold`, `bar_spans_gradient`
  - styles/highlighting: `yaml_highlight_line`, `json_highlight_line`, `split_yaml_comment`, `split_yaml_key`
  - headers/footer: `header_logo_spans`, `shell_breadcrumbs`, `draw_rate_limit_banner`, `action_error_label`, `action_error_details`
  - tables: `render_detail_table` (plus table-specific style helpers)
- Logic-heavy (should move to state/domain before render depends on them):
  - image update/digest normalization: `normalize_*`, `image_update_*`, `manifest_*`, `local_repo_digest`, `is_rate_limit_error`
  - template/build helpers: `build_compose_yaml`, `write_stack_template_compose`, `create_template`, `create_net_template`, `deploy_*`, `delete_*`, `maybe_autocommit_templates`
  - command/exec glue: `shell_exec_*`, `shell_check_image_updates`, `shell_execute_action`, `shell_open_console`
  - parse/escape: `parse_cmdline_tokens`, `shell_escape_*`, `shell_is_safe_token`, `shell_escape_double_quoted`
- Inspect/log view helpers (could move to dedicated module later):
  - `highlight_log_line_*`, `build_inspect_lines*`, `summarize`, `collect_*` helpers.

## Notes
- No version bump for doc-only changes.
- Keep commits small; run `cargo test` after each migration step.
- Prefer adding adapters instead of changing service behavior mid-step; remove adapters once all callers are moved.
