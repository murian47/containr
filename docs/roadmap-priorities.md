# Roadmap: Priorities & Sequencing

Status: 2025-12-21

This document captures the agreed sequencing for upcoming work so we avoid rework
on themes and UI previews.

Note: For reliable key mappings (function keys, Ctrl+Shift), ensure your terminal
reports `xterm-256color` (e.g. iTerm2 via Profile → Terminal → Report Terminal Type).

## Versioning policy

- Pre-1.0: Patch for fixes/docs/internal changes, minor for user-facing features.
- First public release target: 0.8.0 (when the feature set is stable).
- Post-0.8.0 public release: Breaking changes bump at least the minor version.

## Priority Order (Remaining)

1. Deployment metadata
   - Show last deploy target per template in the Templates list

2. Update/Rollback + registry auth
   - Rollback/history UI for template deploys
   - Registry auth via keyring (primary) + optional age fallback for headless/WSL
   - Stack update (image-based recreate): see `concept-stack-update.md`

3. Command placeholders
   - Implement placeholder expansion for command line + keybindings
   - Integrate with new views/context (selection, marks, stack)

4. Theme regeneration (post schema)
   - Regenerate Ghostty themes only after schema is stable

5. AI support (beyond interactive edits)
   - Assist with template creation/refactors, summaries, and guidance

## Rationale

- Deployment metadata unblocks visibility in Templates.
- Registry auth improves update checks without rate-limit friction.
- Placeholders add strong UX leverage across all commands.
- Theme selection + mass theme generation should happen after schema changes
  to avoid double work.
