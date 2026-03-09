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

## Next 3 PRs (Execution Plan)

### PR1: Deployment History Visibility (read-only first)

Scope:
- Add a clear history panel/section in Templates and Stacks details.
- Show per deploy entry:
  - server name
  - timestamp
  - template id/name
  - optional commit id (if available)
- Keep this PR read-only (no rollback action yet).

Acceptance:
- History is visible from normal workflows without command-line fallback.
- Empty-state messaging is explicit ("no deployment history yet").
- No regression in current deploy/redeploy flow.

### PR2: Rollback MVP (safe and explicit)

Scope:
- Add rollback command/action from history selection.
- Rollback target is a previously known deployment entry.
- Confirmation dialog required by default.
- Clear success/failure messages and markers.

Acceptance:
- Rollback can be triggered from UI history selection.
- Rollback failure paths are actionable in messages.
- Smoke-tested for: success, missing artifact/entry, remote failure.

### PR3: Registry Auth UX Hardening

Scope:
- Improve inline guidance for auth setup (keyring/ENV/age chain).
- Add clearer error-to-action hints in messages and detail panel.
- Improve `:registry test` output wording (what failed, what to do next).

Acceptance:
- Users can resolve common auth failures without external docs.
- Error messages reference next step, not only raw failure text.
- No change in existing auth backend order/behavior.

## After PR1-PR3

- Run final release hardening pass:
  - full smoke checklist
  - CI baseline verification
  - docs/packaging polish

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
