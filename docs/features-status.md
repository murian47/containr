# Features & Status

Status: 2026-02-27

This file reflects the current implementation status.

## Implemented

- Modular TUI shell with sidebar, header, status/command line, messages view, optional log dock
- Theme system with selector + preview, persisted active theme
- Scoped keymap system (`always`, `global`, `view:<name>`) via `:map`/`:unmap`
- Server management (`ssh` + `local`) including `:server shell`
- Views: dashboard (single/all), stacks, containers, images, volumes, networks, templates, registries, logs, inspect, help, messages
- Per-view split layout persistence (horizontal/vertical)
- Container and stack actions, bulk selection, tree view, stack update/recreate flows
- Logs/inspect viewers with search, save, copy, and navigation helpers
- Templates (stacks + networks): create/edit/remove/deploy/redeploy/import from stack/container/network
- Template Git integration: status/diff/log/commit/pull/push/init/clone + optional autocommit
- Registry management and auth testing (keyring/ENV/age fallback chain)
- External AI-agent integration for interactive template editing (`CONTAINR_AI_CMD`)

## Partial / Limited

- Advanced rollback/history UX for deployments is still open
- Further UI/domain decoupling can still be improved (service boundaries and view-model cleanup)

## Open TODOs (Near-term)

1. Expand deployment history/rollback UX in templates/stacks workflows.
2. Polish registry auth UX (clearer guidance/error states for keyring/ENV/age setup).
3. Final release hardening pass (CI gates, docs polish, smoke checklist execution).

## Deprioritized

- Command placeholder expansion (`${server.*}`, `${selection.*}`, `${marks.*}`) is currently out of scope.
