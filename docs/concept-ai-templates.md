# AI Template Agent: Concept (Current)

## Use-case
Allow a user to edit an existing compose template via an external AI agent. The agent runs in an interactive shell, can read the template file directly, and is expected to edit it in place.

## Preconditions
- AI agent configured via environment variable (see Configuration).
- Template exists in the local templates repo (Git-backed).
- The user can exit the agent to return to containr.

## Flow (interactive agent)
1) User triggers the AI action from the Templates view (AI menu).
2) containr opens an interactive shell and runs the configured command.
3) The agent receives context (which file to edit and what the task is).
4) The agent edits files in the templates repo directly.
5) Agent exits -> containr returns and checks for local changes.
6) Templates list shows modified state based on Git status.

## Failure cases
- No agent configured -> AI menu is hidden (or a warning is shown).
- Agent exits with non-zero -> log error in Messages.
- No changes detected -> no modified marker is shown.

## Configuration
- `CONTAINR_AI_CMD` environment variable, e.g.
  - `CONTAINR_AI_CMD="codex exec --model gpt-5.2-codex"`
- The command is run in a shell, stdin receives a task prompt.
- Secrets via ENV only, not in config.

## Rationale
- Interactive agent avoids fragile diff parsing.
- The templates repo is the single source of truth.
- Git status provides a simple, consistent "modified" signal.
