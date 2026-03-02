# Release Prep (GitHub)

## Refactor / Structure
- Keep module boundaries clean and avoid new UI monoliths.
- Keep UI logic separated from domain ops where possible.
- Contributor code map: `docs/code-map-ui.md`.

## Stability Pass
- Automated baseline complete:
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test`
- Manual smoke test of core flows complete:
  - Stacks: start/stop/restart/update, update-all.
  - Containers: logs/inspect/console, start/stop/restart/remove.
  - Templates: add/edit/deploy/redeploy/remove, git status/commit/push.
  - Registries: test, default selection.
- Verify keybindings + command-line mappings (map/unmap, scoped mappings).

## Docs & Packaging
- README: install, quick start, keybindings, commands, config/theme locations.
- Changelog: short list of notable features/changes.
- License + contribution guidelines.
- Docs relevance matrix: `docs/docs-status.md`.
- Supported platform statement for `v0.5.0`: Linux + macOS only.

Current gap before public release:
- final version bump and tag for `v0.5.0`
- publish release artefacts / packaging metadata

## Polish / Edge Cases
- Reduce log noise and confusing warnings.
- Confirm behavior on empty server list and malformed config/theme.
- Ensure errors are visible and actionable (messages + UI marker).

## Version & Tag
- Bump version and create tag (e.g., `v0.5.0`).

## 1.0 Gate
- `v0.5.x` can be used for the first public release line while core workflows are hardened.
- `v1.0.0` should be reserved for the point where:
  - deploy/history/rollback UX is no longer a known gap
  - registry auth setup and failure handling are clear in normal use
  - smoke, packaging, docs, and install/runtime paths are explicitly signed off
