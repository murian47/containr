# AI Template Patch: Concept

## Use-case
Allow a user to edit an existing compose template via AI. The AI returns a unified diff. containr can show the diff and, if Git is active and the user allows it, apply the patch automatically.

## Preconditions
- AI provider configured via CLI (for example, `codex exec`).
- Template exists in the local templates repo.
- Git integration is optional but required for auto-apply.

## Flow (manual apply)
1) User triggers the AI action (for example `:ai edit`).
2) containr builds a prompt from `compose.yaml` plus context and runs the CLI.
3) containr extracts the last unified diff block from the CLI output.
4) The diff is shown for review.
5) User confirms -> patch is applied.

## Flow (auto-apply with Git)
1) User triggers the AI action.
2) containr runs the CLI and extracts the diff.
3) containr runs `git apply --check` on the diff.
4) If OK -> `git apply` automatically.
5) If check fails -> show diff for manual review.
6) Optional: offer `git commit` with an AI-suggested message.

## Failure cases
- No diff found -> warn and stop.
- Diff does not apply -> warn and show diff.
- CLI error -> report via Messages.

## Configuration
- `ai.provider = "codex_cli"`
- `ai.cmd = "codex exec --model gpt-5.2-codex"`
- `ai.auto_apply = true|false`
- Secrets via ENV only, not in config.

## Rationale
- Diff-first keeps changes explicit and reviewable.
- Auto-apply only when Git is active and enabled.
- Works without Git (manual preview/apply).
