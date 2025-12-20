# Roadmap: Priorities & Sequencing

Status: 2025-12-20

This document captures the agreed sequencing for upcoming work so we avoid rework
on themes and UI previews.

## Priority Order

1. Stacks/Templates core functionality (M1–M3)
   - M1: Templates view + Git actions + $EDITOR integration
   - M2: Stacks view + lifecycle actions (start/stop/restart, etc.)
   - M3: Deploy flow (render, copy, compose up) + state/history

2. Command placeholders
   - Implement placeholder expansion for command line + keybindings
   - Integrate with new views/context (selection, marks, stack)

3. Theme selection UI + theme regeneration
   - Add :theme select UI with sidebar list + main preview
   - Regenerate Ghostty themes only after schema is stable

## Rationale

- Stacks/Templates are high-value and likely to influence theme schema.
- Placeholders add strong UX leverage across all commands.
- Theme selection + mass theme generation should happen after schema changes
  to avoid double work.
