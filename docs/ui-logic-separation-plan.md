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

## Notes
- No version bump for doc-only changes.
- Keep commits small; run `cargo test` after each migration step.
- Prefer adding adapters instead of changing service behavior mid-step; remove adapters once all callers are moved.
