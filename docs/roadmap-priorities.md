# Roadmap: Priorities & Sequencing

Status: 2025-12-21

This document captures the agreed sequencing for upcoming work so we avoid rework
on themes and UI previews.

## Priority Order

1. Stacks/Templates core functionality (M1–M3)
   - M1: Templates view + Git actions + $EDITOR integration
   - M2: Stacks view + lifecycle actions (start/stop/restart, etc.)
   - M3: Deploy flow (render, copy, compose up) + state/history (track last deploy target)
   - M4: Update/Rollback + image updates (check, pull, recreate, history UI)
   - M5: AI support (assistant-driven template creation/editing)

2. Image update visibility + recreate workflows
   - Check local images against remote registries (opt-in or cached)
   - Show updates in UI (per container + per stack)
   - Recreate container/stack (with optional pull) via compose or docker

3. Command placeholders
   - Implement placeholder expansion for command line + keybindings
   - Integrate with new views/context (selection, marks, stack)

4. Theme selection UI + theme regeneration
   - Add :theme select UI with sidebar list + main preview
   - Regenerate Ghostty themes only after schema is stable

## Rationale

- Stacks/Templates are high-value and likely to influence theme schema.
- Placeholders add strong UX leverage across all commands.
- Theme selection + mass theme generation should happen after schema changes
  to avoid double work.
