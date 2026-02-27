# Roadmap: Priorities & Sequencing

Status: 2026-02-27

This document captures the agreed sequencing for remaining work.

## Versioning policy

- Pre-1.0:
  - Patch: fixes/docs/internal refactors
  - Minor: user-visible feature additions
- Do not auto-bump patch for pure doc-only commits.

## Priority Order (Remaining)

1. `ui/mod.rs` final decomposition
   - keep shrinking central wiring surface
   - move remaining mixed concerns into focused modules

2. Deployment history + rollback UX
   - improve history visibility in templates/stacks views
   - add practical rollback workflow on top of current deploy metadata

3. Registry auth UX hardening
   - clearer setup flow for keyring/ENV/age fallback
   - clearer error/action hints in UI and messages

4. Release hardening
   - final smoke checklist pass
   - CI baseline (`test`, `fmt --check`, `clippy -D warnings`)
   - package/docs polish

## Deprioritized / Out of scope for current release

- Command placeholders (`${server.*}`, `${selection.*}`, `${marks.*}`, `${view}`)

## Recently completed (for context)

- PR8 visibility tightening pass
- PR9 contributor code map (`docs/code-map-ui.md`)
- render decomposition (legacy `render.inc.rs` removed)
- dashboard all-servers view and stack update workflows
