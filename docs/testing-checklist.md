# Release Smoke & Signoff Checklist

This checklist is intended for release candidates and final signoff before publishing.

Legend:
- `[ ]` not checked yet
- `[x]` passed
- `[!]` failed / must be fixed before release
- `P0` release blocker
- `P1` should pass before release
- `P2` useful confidence check, but not a hard blocker by itself

## Signoff rule

Release signoff requires:
- all `P0` items marked `[x]`
- no unresolved crash, data-loss, or deploy-blocking regressions
- any `P1` exceptions documented explicitly in the release notes

## P0 Core startup and navigation

- [X] App starts without configured servers and shows the no-server screen.
- [X] App starts with configured servers and lands in a valid initial view.
- [X] Switching between main views works (`dashboard`, `containers`, `images`, `networks`, `stacks`, `templates`).
- [X] When switching views from the sidebar, the active view highlight updates and focus remains in the sidebar.
- [X] Messages view opens and closes correctly (`C-g`, `q`).

## P0 Server handling

- [X] `:server add` creates a server entry.
- [X] Added server entry persists after restart.
- [X] `:server select` switches target and refreshes data.
- [X] Invalid or unreachable server shows a clear error without leaving stale dashboard state behind.

## P0 Containers

- [X] Container list renders and selection moves correctly.
- [X] `start`, `stop`, `restart`, and `delete` work where supported.
- [X] Logs open and close correctly.
- [X] Inspect opens and close correctly.
- [X] Console opens correctly for a running container.

## P0 Stacks

- [X] Stack list renders and empty stacks are visually distinguishable.
- [X] Stack details include all relevant components (containers, networks).
- [X] `start`, `stop`, `restart`, and `delete` work.
- [X] `update` works.
- [X] `update --all` works.

## P0 Templates

- [X] Template list renders with description and current status.
- [X] Details open and remain readable.
- [X] `:template edit` uses the configured editor flow correctly.
- [X]`:template deploy` works.
- [X] `:template deploy --pull` works.
- [X] `:template deploy --recreate` works.
- [X] `:template rm` works.

## P0 Git integration (templates)

- [X] `:git status` works in templates context.
- [X] `:git diff` works in templates context.
- [X] `:git log` works in templates context.
- [X] `:git commit` updates template git state correctly.
- [X] Autocommit indicator shows `Commit: auto` or `Commit: manual` as expected.

## P1 Image update checks

- [X] `:container check` enqueues update checks.
- [X] `:stack check` enqueues update checks.
- [X] Marker colors are correct:
  - green = up-to-date
  - yellow = update available
  - red = error
  - blue = rate limit
- [X] Rate limit banner appears and is visually centered when applicable.

## P1 Themes

- [X] Built-in themes load without manual installation.
- [X] User override themes still load correctly.
- [X] Theme selector preview works.
- [X] Highlight colors remain legible in active views.

## P1 Editor fallback

- [X] Explicitly configured editor works.
- [X] `$EDITOR` fallback works.
- [X] `vi` fallback works when nothing else is configured.

## P1 Mapping and command line

- [X] `:help` renders.
- [X] `:map list` renders.
- [X] Scoped mappings still work after restart.
- [X] Command-line history works.

## P2 Miscellaneous confidence checks

- [X] `:messages save` writes a file.
- [X] Log dock can be enabled and remains stable while switching views.
- [X] Dashboard and all-servers dashboard render without layout glitches after refresh.
- [X] Theme switching does not leave stale dashboard graphics behind.
