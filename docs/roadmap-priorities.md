# Roadmap: Priorities & Sequencing

Status: 2026-02-27

This document captures the agreed sequencing for remaining work.

## Versioning policy

- Pre-1.0:
  - Patch: fixes/docs/internal refactors
  - Minor: user-visible feature additions
- Do not auto-bump patch for pure doc-only commits.

## 1.0 Definition of Done

Version `1.0.0` is not "all ideas implemented". It means the product is stable, coherent, and
complete in its core operating model.

Required before `1.0.0`:

1. Core workflows are stable and complete enough for daily use:
   - server selection and dashboard views
   - stack and container operations
   - template create/edit/deploy/redeploy flows
   - template git workflows
   - registry management and auth setup
2. No major UX gaps remain in core administration flows:
   - deployment history is visible
   - rollback/recovery workflow is practical
   - registry auth errors and setup steps are understandable from within the UI
3. Release hardening is complete:
   - manual smoke checklist passed and signed off
   - CI baseline enforced (`test`, `fmt --check`, `clippy -D warnings`)
   - packaging and install/runtime paths behave correctly on supported targets
4. Documentation is complete enough for external users:
   - install and quick start are documented
   - config/theme/template locations are documented
   - key workflows are discoverable without prior project history

Not required for `1.0.0`:
- speculative placeholder systems
- large future feature branches that do not close a core workflow gap
- total elimination of every internal refactor opportunity

## Priority Order (Remaining)

1. Deployment history + rollback UX
   - improve history visibility in templates/stacks views
   - add practical rollback workflow on top of current deploy metadata

2. Registry auth UX hardening
   - clearer setup flow for keyring/ENV/age fallback
   - clearer error/action hints in UI and messages

3. Release hardening
   - final smoke checklist pass
   - CI baseline (`test`, `fmt --check`, `clippy -D warnings`)
   - package/docs polish

## Deprioritized / Out of scope for current release

- Command placeholders (`${server.*}`, `${selection.*}`, `${marks.*}`, `${view}`)
- Config option for server-switch behavior:
  - keep current view on server change
  - or force switch to dashboard
  - target: post-`0.5.0`, not a release blocker
- Image update check rework:
  - deduplicate identical image-ref checks across stacks/servers
  - prefer cached/stale status over immediate live re-checks
  - expose "last checked" or stale semantics in the UI
  - separate normal cached checks from explicit forced live refreshes
  - target: post-`0.5.0`, not a release blocker

## Recently completed (for context)

- PR8 visibility tightening pass
- PR9 contributor code map (`docs/code-map-ui.md`)
- render decomposition (legacy `render.inc.rs` removed)
- dashboard all-servers view and stack update workflows
