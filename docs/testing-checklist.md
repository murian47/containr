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

- [ ] App starts without configured servers and shows the no-server screen.
- [ ] App starts with configured servers and lands in a valid initial view.
- [ ] Switching between main views works (`dashboard`, `containers`, `images`, `networks`, `stacks`, `templates`).
- [ ] Sidebar focus and selection highlight remain correct while switching views.
- [ ] Messages view opens and closes correctly (`C-g`, `q`).

## P0 Server handling

- [ ] `:server add` creates a server entry.
- [ ] Added server entry persists after restart.
- [ ] `:server select` switches target and refreshes data.
- [ ] Invalid or unreachable server shows a clear error without leaving stale dashboard state behind.

## P0 Containers

- [ ] Container list renders and selection moves correctly.
- [ ] `start`, `stop`, `restart`, and `delete` work where supported.
- [ ] Logs open and close correctly.
- [ ] Inspect opens and close correctly.
- [ ] Console opens correctly for a running container.

## P0 Stacks

- [ ] Stack list renders and empty stacks are visually distinguishable.
- [ ] Stack details include all relevant components (containers, networks).
- [ ] `start`, `stop`, `restart`, and `delete` work.
- [ ] `update` works.
- [ ] `update --all` works.

## P0 Templates

- [ ] Template list renders with description and current status.
- [ ] Details open and remain readable.
- [ ] `:template edit` uses the configured editor flow correctly.
- [ ] `:template deploy` works.
- [ ] `:template deploy --pull` works.
- [ ] `:template deploy --recreate` works.
- [ ] `:template rm` works.

## P0 Git integration (templates)

- [ ] `:git status` works in templates context.
- [ ] `:git diff` works in templates context.
- [ ] `:git log` works in templates context.
- [ ] `:git commit` updates template git state correctly.
- [ ] Autocommit indicator shows `Commit: auto` or `Commit: manual` as expected.

## P1 Image update checks

- [ ] `:container check` enqueues update checks.
- [ ] `:stack check` enqueues update checks.
- [ ] `:template check` enqueues update checks.
- [ ] Marker colors are correct:
  - green = up-to-date
  - yellow = update available
  - red = error
  - blue = rate limit
- [ ] Rate limit banner appears and is visually centered when applicable.

## P1 Themes

- [ ] Built-in themes load without manual installation.
- [ ] User override themes still load correctly.
- [ ] Theme selector preview works.
- [ ] Highlight colors remain legible in active views.

## P1 Editor fallback

- [ ] Explicitly configured editor works.
- [ ] `$EDITOR` fallback works.
- [ ] `vi` fallback works when nothing else is configured.

## P1 Mapping and command line

- [ ] `:help` renders.
- [ ] `:map list` renders.
- [ ] Scoped mappings still work after restart.
- [ ] Command-line history works.

## P2 Miscellaneous confidence checks

- [ ] `:messages save` writes a file.
- [ ] Log dock can be enabled and remains stable while switching views.
- [ ] Dashboard and all-servers dashboard render without layout glitches after refresh.
- [ ] Theme switching does not leave stale dashboard graphics behind.
