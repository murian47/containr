# Testing Approach

This document describes a lightweight testing strategy for containr.

## Goals
- Catch regressions without a dedicated QA team.
- Keep tests fast and deterministic.
- Use manual checks only for high-risk workflows.

## Test layers

### Unit tests (default)
- Parsing and normalization (commands, args, image refs).
- State transitions and command routing.
- Rendering helpers (small, deterministic buffers).

### Integration tests (selective)
- Docker/Podman CLI wrappers (mocked or recorded fixtures).
- SSH command formation and error handling.
- Remote integration tests run only when explicitly enabled.

### Manual smoke checks (release candidates)
- Follow `docs/testing-checklist.md`.
- Limit to a short set of flows that cover the highest risk paths.

## CI gates
- `cargo test`
- Optional: `cargo clippy -- -D warnings`
- Optional: `cargo fmt --check`

## Running integration tests
- `CONTAINR_IT=1 cargo test --features integration`
- Optional override: `CONTAINR_IT_TARGET=mag@rpi47.local47.de`

## Canary testing
- Use 1–2 real hosts with simple templates and small stacks.
- Exercise deploy/recreate/update checks regularly.
