# AI Agents In Project Work

Purpose: explain how AI agents are used while developing and maintaining `containr`.

This document is about contributor workflow. It is not about `containr` product features.

## Scope

AI agents may help with project work such as:

- reading and summarizing the codebase
- proposing or implementing refactors
- writing or updating documentation
- reviewing changes for regressions or missing tests
- preparing releases, checklists, or status summaries

AI agents are assistants for maintainers. They do not define project policy on their own.

## Current Role In This Project

In this repository, AI agents are mainly used for:

- repository navigation and fast context building
- small to medium code changes under maintainer supervision
- documentation maintenance
- review support and risk spotting
- housekeeping tasks such as path fixes, changelog edits, and release prep follow-ups

The maintainer remains responsible for deciding what is correct, what gets merged, and what gets tagged or released.

## Working Rules

When using an AI agent in this project:

1. Treat generated output as a draft until it has been checked against the real code.
2. Prefer small, reviewable changes over broad speculative rewrites.
3. Keep the application buildable after each step.
4. Run appropriate verification for the type of change.
5. Do not trust stale architectural assumptions; inspect the current repository first.
6. Keep documentation and code comments in English.
7. Keep user-facing collaboration in German for this project session unless explicitly changed.

## Good Fit

AI agents work well for:

- locating the right files for a change
- updating docs after refactors
- generating first-pass test cases
- finding mismatches between docs and code
- reviewing command, state, and rendering flow after modular refactors
- handling repetitive edits with clear acceptance criteria

## Poor Fit

AI agents are a weak fit for:

- making release decisions without human review
- large architectural rewrites without incremental checkpoints
- security-sensitive assumptions without explicit verification
- inferring behavior from old docs when the code says otherwise
- destructive git actions without explicit approval

## Review Expectations

Changes produced with AI assistance should still be reviewed like any other change:

- validate the affected paths and behavior
- check for regressions in neighboring flows
- confirm docs still match the real repository layout
- verify tests were run when the change justifies it
- make sure commit and tag operations reflect the intended release state

## Typical Workflow

1. Read the relevant docs and current code paths.
2. Compare documentation against the live repository state.
3. Apply a minimal patch.
4. Run checks proportional to the change.
5. Review the diff before commit.
6. Commit and tag only after the maintainer confirms the state is correct.

## Related Files

- `AGENTS.md`
- `codex-infos-and-rules.md`
- `docs/code-map-ui.md`
- `docs/testing-approach.md`
- `docs/release-prep.md`
