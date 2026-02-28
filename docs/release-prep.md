# Release Prep (GitHub)

## Refactor / Structure
- Keep module boundaries clean and avoid new UI monoliths.
- Keep UI logic separated from domain ops where possible.
- Track refactor completion in: `docs/readability-refactor-pr-plan.md`.
- Contributor code map: `docs/code-map-ui.md`.

## Stability Pass
- Automated baseline complete:
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test`
- Manual smoke test of core flows:
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

Current gap before public release:
- Manual smoke checklist in `docs/testing-checklist.md` still needs an explicit pass and sign-off.

## Polish / Edge Cases
- Reduce log noise and confusing warnings.
- Confirm behavior on empty server list and malformed config/theme.
- Ensure errors are visible and actionable (messages + UI marker).

## Version & Tag
- Bump version and create tag (e.g., `v0.5.0`).
