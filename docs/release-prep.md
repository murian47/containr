# Release Prep (GitHub)

## Refactor / Structure
- Shrink `src/ui/render.inc.rs` further; move remaining helpers to focused modules.
- Reduce `src/ui/mod.rs` monolith (split by domain/view where practical).
- Keep UI logic separated from domain ops where possible.
- Execute readability plan: `docs/readability-refactor-pr-plan.md`.

## Stability Pass
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

## Polish / Edge Cases
- Reduce log noise and confusing warnings.
- Confirm behavior on empty server list and malformed config/theme.
- Ensure errors are visible and actionable (messages + UI marker).

## Version & Tag
- Bump version and create tag (e.g., `v0.5.0`).
