# Features & Status

Status: 2025-12-22

This list reflects the current implementation and what is planned next.

## Implemented

- UI shell: sidebar, header, status/command line, messages view, themes
- Theme selector UI with preview
- Keymaps with scopes (`always`, `global`, `view:<name>`) via `:map`/`:unmap`
- Servers: list/use/add/remove, SSH shell (`:server shell`)
- Views: Dashboard, Containers, Images, Volumes, Networks, Templates, Logs, Inspect, Registries, Stacks
- List/details split (horizontal/vertical), per-view layout persisted
- Containers: start/stop/restart/rm/check, console, bulk selection
- Stack tree view for containers
- Logs viewer: search (regex/literal), highlight, save, line numbers, jump-to-line, copy
- Inspect viewer: tree/folding, search, save, copy value/path
- Templates: stacks/networks, add/edit/rm, deploy, generate from stack/container
- Git integration for templates repo (status/diff/log/commit/pull/push/init/clone)
- Registries: config/test, age-backed secrets, registry view

## Partial / Limited

- Registry auth UX (keyring integration) not complete
- Template deploy metadata (last deploy target) not shown in list yet

## Planned (Roadmap)

- Update/rollback UI + registry auth via keyring (age fallback)
- AI-assisted template creation/editing

## Deferred

- Image update checks (manual); full update/rollback workflow not complete
- Template deployment metadata in list view
- Command placeholders for CLI + keybindings (server/container/selection vars)
