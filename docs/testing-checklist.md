# Testing Checklist

This checklist is intended for release candidates and smoke checks.

## Core navigation
- App starts without configured servers (no-server screen).
- Switch between views (dashboard, containers, images, networks, stacks, templates).
- Sidebar focus changes highlight correctly.
- Messages view opens/closes (`C-g`, `q`).

## Servers
- `:server add` creates entry; persists after restart.
- `:server select` switches target and refreshes data.

## Containers
- List renders; selection moves with arrows.
- Actions: start/stop/restart/delete (when supported).
- Inspect/logs open and close correctly.

## Stacks
- List shows stacks and dimmed when empty.
- Stack details list all components (containers/networks).
- Actions: start/stop/restart/delete.

## Templates
- List shows templates with description.
- Details open; `:template edit` respects configured editor.
- `:template deploy` works; `--pull` and `--recreate` behave as expected.

## Image updates
- `:container check` / `:stack check` / `:template check` enqueue checks.
- Update marker colors: green (up-to-date), yellow (update), red (error), blue (rate limit).
- Rate limit banner appears and centers when applicable.

## Git integration (templates only)
- `:git status`, `:git diff`, `:git log` work in templates context.
- Autocommit indicator shows `Commit: auto` or `Commit: manual`.

## Editor
- Configured editor → `$EDITOR` → `vi` fallback works.

## Themes
- Theme loads without errors; highlight colors legible.

## Misc
- `:help` and `:map list` render.
- `:messages save` writes file.
