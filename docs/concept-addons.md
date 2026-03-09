# Concept: External Addons for containr

Status: draft

This document defines a practical addon model for containr where addons are registered through metadata, receive structured input via `stdin` as JSON, and return structured JSON responses via `stdout`.

## Goals

- Extend containr capabilities without hard dependency on vendor-specific tools.
- Keep core flows stable and independent from addon implementations.
- Enable optional functionality (template helpers, diagnostics, validation, etc.).
- Make failures explicit, logged, and non-blocking for core workflows.

## Non-goals

- No in-process dynamic library loading.
- No mandatory addon execution for core operations.
- No hidden command execution without explicit registration and consent.

## Architecture Overview

- containr is the **host**.
- Addons are external executables launched by containr as separate processes.
- Communication is JSON-over-`stdin`/`stdout` for one request and one response.
- `stderr` is captured for diagnostics and surfaced in messages.

## Addon Registration

Addons are declared in configuration with machine-readable metadata.

```json
{
  "id": "com.example.template-linter",
  "name": "Template Linter",
  "version": "0.1.0",
  "entry": ["template-linter", "--json"],
  "commands": [
    {
      "name": "template-lint",
      "description": "Validate the selected template",
      "capability": "template.validate",
      "action": "validate_template",
      "default_scope": ["templates"],
      "shortcut_allowed": true,
      "requires_confirmation": false
    }
  ],
  "capabilities": ["template.validate", "template.suggest"],
  "scope": ["templates", "stacks"],
  "stdin_protocol": "containr.addon.v1",
  "timeout_secs": 45,
  "working_dir_policy": "templates",
  "max_payload_bytes": 262144,
  "env_allowlist": ["EDITOR", "HOME", "PATH"],
  "trusted": false
}
```

### Registry field suggestions

- `id`: unique, stable addon identifier.
- `name`: user-facing label.
- `entry`: executable + arguments (no shell expansion).
- `commands`: named addon commands surfaced by the host.
- `capabilities`: feature tags this addon can handle.
- `scope`: contexts where the capability is relevant.
- `stdin_protocol`: protocol/version negotiation marker.
- `timeout_secs`: per-addon request timeout.
- `working_dir_policy`: where the addon should run (for example `templates` or explicit path).
- `max_payload_bytes`: input size guard for large templates.
- `env_allowlist`: environment variables passed through.
- `trusted`: true only for addons with explicit user trust.

## Execution Flow

1. User action maps to a capability (for example `template.validate`).
2. containr resolves either:
   - command invocation (direct addon command name), or
   - mapped shortcut that resolves to an addon command.
3. containr validates the addon is enabled and allowed in current scope.
4. containr serializes a request object and writes it to the addon process `stdin`.
5. containr reads one JSON response from `stdout`.
6. containr converts result into UI messages, view updates, and optional file refresh hints.
7. On timeout/invalid JSON/exit failure, containr marks action as failed and shows diagnostics.

## Addon Command Model

Addons register own command entries in metadata and become addressable by name.

- Direct execution:
  - `:addon run template-lint`
  - optional explicit command family like `:template-lint`
- Shortcut execution:
  - any command can be mapped in existing `:map` bindings, e.g. `:map C-l template-lint`
- Command names are human-readable but resolved to addon+action internally.

### Command resolution and conflicts

- Resolution starts with exact command name lookup in registered addons.
- If no direct hit exists, fallback behavior is unchanged for existing built-in commands.
- If multiple addons expose the same command name, host applies stable priority:
  - explicit addon priority in config (first),
  - then declaration order.
- Unknown command is always surfaced to the user with a clear message.

### AI integration as addon candidate

AI should stay outside the core by registering an addon command.

- Core action: `:template edit` stays unchanged and handles local editing workflow.
- AI assist becomes command-driven:
  - `id`: `containr-template-ai`
  - command: `template-edit-assist`
  - capability: `template.edit.assist`

## JSON Request Schema

```json
{
  "protocol": "containr.addon.v1",
  "request_id": "req-2026-03-06-001",
  "addon_id": "com.example.template-linter",
  "capability": "template.validate",
  "action": "validate_template",
  "context": {
    "view": "templates",
    "selected_server": "rpi5",
    "selected_template": "wordpress",
    "file_paths": [
      "/home/user/.config/containr/templates/stacks/wordpress/compose.yaml"
    ],
    "template_dir": "/home/user/.config/containr/templates/stacks/wordpress",
    "template_meta": {
      "name": "wordpress",
      "template_ref": "main",
      "source": "local"
    },
    "raw_input": null
  },
  "permissions": {
    "read_paths": [
      "/home/user/.config/containr/templates/stacks/wordpress"
    ],
    "write_paths": []
  },
  "ui": {
    "needs_confirmation": true,
    "message_level_hint": "normal"
  },
  "env_hints": {
    "EDITOR": "vim"
  }
}
```

## JSON Response Schema

```json
{
  "protocol": "containr.addon.v1",
  "request_id": "req-2026-03-06-001",
  "addon_id": "com.example.template-linter",
  "status": "ok",
  "status_code": 0,
  "summary": {
    "title": "Template validation complete",
    "message": "No blocking issues found."
  },
  "issues": [
    {
      "severity": "warning",
      "file": "compose.yaml",
      "line": 12,
      "column": 3,
      "rule_id": "PORT_EXPOSED",
      "message": "Port 80 is public without HTTPS recommendation."
    }
  ],
  "artifacts": {
    "updated_files": [],
    "generated_files": []
  },
  "ui_hints": {
    "actions": [
      { "id": "open_log", "label": "Open details" },
      { "id": "rerun", "label": "Run again" }
    ],
    "refresh_views": ["templates", "stacks"]
  },
  "runtime": {
    "ms": 180
  }
}
```

### Status values

- `ok`: success.
- `warning`: completed with non-blocking issues.
- `error`: failed action or malformed output.
- `cancelled`: user-cancelled before completion.

## Security and Safety

- Addon execution is opt-in and disabled by default.
- Addons are launched with explicitly allowed environment and no implicit shell interpolation.
- Optional global guardrails: process timeouts, payload limits, path allowlist.
- `stderr` is never interpreted as protocol data, only logged.
- Addons without matching capability are never invoked.
- Sensitive values must be redacted in logs/messages.

## UI Integration

- Add command entry points for addon management:
  - `:addon list`
  - `:addon enable <addon_id>`
  - `:addon disable <addon_id>`
  - `:addon reload [--all|<addon_id>]`
- `:addon status [<addon_id>]`
- `:addon check [--all|<addon_id>] [--json]`
- `:addon run <command_name> [--raw ...]`
- Action menus only show addon-backed commands when a matching capability exists.
- Addon failures are shown with clear status and raw output summary.
- Warning/error issues can be surfaced in messages and optionally annotated in relevant views.

Management behavior:

- `:addon list` shows registered addons with state (`enabled`/`disabled`), version, command count and status.
- `:addon enable` and `:addon disable` only flip runtime enablement; full uninstall/install remains out of scope for this concept.
- `:addon status` provides last run result, last error, and runtime metadata for each enabled addon.
- `:addon reload` reloads addon registry metadata and optional runtime cache without restarting the app.
- `:addon check` validates executable availability, JSON contract version, and command declaration integrity.

### Addon check details

- Command grammar:
  - `:addon check` checks all enabled addons.
  - `:addon check <addon_id>` checks one addon.
  - `:addon check --all` checks registered addons independent of current enabled state.
  - `:addon check --json` returns machine-parseable result objects instead of normal message rendering.

- Per-addon checks:
  - metadata schema validation for required fields.
  - command registry integrity and duplicate command-name resolution rules.
  - executable resolution for `entry` and permission to execute.
  - startup probe for protocol banner/handshake.
  - response schema parseability.
  - optional security policy checks (env allowlist, timeout bound, payload bounds).
  - optional command-scope sanity check against current UI view context.

- Result model:
  - `ok` all checks passed.
  - `warn` non-blocking issues detected, addon remains callable with caution.
  - `fail` critical failure, addon call path is blocked until resolved.

Example human output:

- `template-linter`: `ok`
- `template-ai`: `warn` (missing optional dependency `codex`, addon still callable)
- `stack-auditor`: `fail` (entry not executable)

- For `--json`, emit:

```json
{
  "command": "addon check",
  "request_id": "chk-2026-03-09-001",
  "results": [
    {
      "addon_id": "com.example.template-linter",
      "status": "ok",
      "checks": [
        { "name": "metadata", "result": "ok" },
        { "name": "entry", "result": "ok" },
        { "name": "handshake", "result": "ok" }
      ],
      "message": "Addon ready",
      "warnings": [],
      "errors": []
    },
    {
      "addon_id": "com.example.stack-auditor",
      "status": "fail",
      "checks": [
        { "name": "metadata", "result": "ok" },
        { "name": "entry", "result": "fail", "detail": "file not executable" }
      ],
      "message": "Addon is disabled for this session",
      "warnings": [],
      "errors": ["entry not executable: /usr/local/bin/stack-auditor"]
    }
  ]
}
```

## Config Integration

Addons are added as a dedicated config section:

```json
{
  "addons": {
  "enabled": true,
  "default_timeout_secs": 60,
  "entries": [
      {
        "id": "...",
        "enabled": true,
        "...": "..."
      }
    ]
  }
}
```

Per-entry `enabled` is authoritative for runtime state and is toggled by `:addon enable` / `:addon disable`.

## Example Addon

A concrete Codex-based example addon is provided at:

- `addons/examples/codex-template-edit/`

It includes:

- `addon.json`: metadata to copy into config
- `run.sh`: stdin-json addon runner
- `README.md`: setup and usage notes


## Incremental Rollout (Recommended)

1. Implement the addon host and JSON protocol primitives.
2. Add one capability-backed action for template editing via existing AI flow.
3. Add `:addon list` and `:addon check`.
4. Add runtime status/error handling and timeout/limits.
5. Add second capability (for example `template.validate`) to prove generality.

## Open Questions

- Global vs per-server addon configuration?
- Shared cache for successful validations and stale-state indicators?
- Should high-risk capabilities require explicit confirmation every run?
- Do we need signature verification for addon executables?
