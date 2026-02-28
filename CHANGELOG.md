# Changelog

All notable changes to this project will be documented in this file.

## [0.4.171] - Unreleased

### Added
- Multi-server dashboard view with compact card layout.
- Template Git metadata and status indicators in list and details views.
- Interactive AI handoff for template editing via external agent command.
- Registry management with default registry support.
- Docked messages/log view with persisted layout state.
- Theme selector with live preview.

### Changed
- Major UI refactor to split rendering, input, commands, state and feature code into smaller modules.
- Improved dashboard rendering, including kitty image bars, UTF-8 fallback bars and better refresh behavior.
- Stack update and redeploy flows expanded with pull/recreate support and better progress reporting.
- Template and network template workflows aligned around local files plus remote deploy state.
- Documentation reorganized into release, roadmap, feature status and user guide documents.

### Fixed
- Clippy warnings across the UI and test code paths under `--all-targets --all-features`.
- Test wiring after UI refactors so `cargo test` remains green.
- Local Docker handling on macOS for deploy/dashboard edge cases.
- Template Git status refresh and modified/deployed marker handling.
- Sidebar, dashboard and docked-log rendering regressions introduced during the UI rewrite.

### Verification
- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`
