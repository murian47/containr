# Codex Template Edit Addon (Example)

This is a minimal example addon that demonstrates how containr can delegate
template editing to an external Codex command.

The addon:
- receives addon payload JSON from `stdin`
- resolves the selected template file (`compose.yaml` or `network.json`)
- builds a prompt
- executes a user-provided Codex command template
- runs interactively when command metadata sets `"interactive": true`

## 1. Register the addon

Copy the object from `addon.json` into your containr config under `addons`.

Important:
- replace the `entry` path with your absolute local path to `run.py`
- keep `capability` as `template-edit` if you want containr to track edits

## 2. Provide a Codex command template

Set `CONTAINR_CODEX_CMD` before starting containr.

The template must include:
- `{file}` placeholder for the selected template file path
- `{prompt}` placeholder for the generated instruction

Example:

```bash
export CONTAINR_CODEX_CMD='codex --file {file} --prompt {prompt}'
```

If `CONTAINR_CODEX_CMD` is not set, the addon runs in `dry-run` mode and prints
a JSON hint instead of executing anything.

Because this example command is marked as interactive in `addon.json`, containr
starts it through the interactive local-command path (TTY attached).

## 3. Enable and run

In containr:

```text
:addon enable dev.example.codex-template-edit
:addon run codex-template-edit "Add comments and improve readability."
```

You can also map the command with `:map` like any other addon command.

## JSON contract notes

Input (from containr) includes:
- `protocol`
- `command`
- `args`
- `context.templates_dir`
- `context.selected[]` with selected entities

Output is free-form text/JSON. This example returns compact JSON status objects.

## Why Python here?

The example intentionally uses a standalone `python3` script to show that addons
are language-agnostic and do not depend on the containr implementation language.
