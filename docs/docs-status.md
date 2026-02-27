# Docs Status Matrix

Status: 2026-02-27

Purpose: quick relevance map for all files in `docs/`.

## Active (current operational docs)

- `overview.md`
- `user_guide_de.md`
- `user_guide_en.de`
- `features-status.md`
- `roadmap-priorities.md`
- `release-prep.md`
- `release_plan_security.md`
- `registry_auth.md`
- `testing-approach.md`
- `testing-checklist.md`
- `code-map-ui.md`
- `readability-refactor-pr-plan.md`

## Active (feature concepts / implementation guidance)

- `concept-ai-templates.md`
- `concept-stack-update.md`
- `concept-stacks-templates.md`
- `plan-stack-update-mvp.md`

## Deprioritized concepts

- `concept-command-placeholders.md` (currently out of scope)

## Historical / superseded planning context

- `ui_refactor_plan.md` (legacy plan around `render.inc.rs`, kept for traceability)
- `ui-logic-separation-plan.md` (high-level long-term architecture notes)
- `ui_mod_refactor_plan.md` (historical plan; still partially relevant for remaining modularization)

## Open TODO Summary

1. Finish `ui/mod.rs` decomposition and reduce central wiring surface.
2. Extend deployment history/rollback UX.
3. Improve registry auth UX (keyring/ENV/age setup guidance and error clarity).
4. Execute release hardening checklist and CI baseline.
