# Changelog

All notable changes to this project will be documented in this file.

## [0.5.0] - Unreleased

### Added
- Multi-server dashboard view with compact card layout.
- Template Git metadata and status indicators in list and details views.
- Interactive AI handoff for template editing via external agent command.
- Registry management with default registry support.
- Docked messages/log view with persisted layout state.
- Theme selector with live preview.
- Installation and uninstall scripts for Linux and macOS.
- Smoke-tested release signoff checklist.

### Changed
- Major UI refactor to split rendering, input, commands, state and feature code into smaller modules.
- Improved dashboard rendering, including kitty image bars, UTF-8 fallback bars and better refresh behavior.
- Stack update and redeploy flows expanded with pull/recreate support and better progress reporting.
- Template and network template workflows aligned around local files plus remote deploy state.
- Documentation reorganized into release, roadmap, feature status and user guide documents.
- README reduced to a release-facing quick-start and installation guide.

### Fixed
- Clippy warnings across the UI and test code paths under `--all-targets --all-features`.
- Test wiring after UI refactors so `cargo test` remains green.
- Local Docker handling on macOS for deploy/dashboard edge cases.
- Template Git status refresh and modified/deployed marker handling.
- Sidebar, dashboard and docked-log rendering regressions introduced during the UI rewrite.
- Range selection and copy in logs and docked/full messages views.
- Theme changes now rebuild dashboard graphics correctly.
- Message saving resolves bare file names into `$HOME`.
- Transient inspect races after recreate no longer spam warnings.
- More Docker manifest output variants are parsed during image update checks.

### Verification
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`
- manual smoke checklist in `docs/testing-checklist.md`
