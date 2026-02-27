# containr – Übersicht (DE) / Overview (EN)

## Was ist containr? (DE)
Ein TUI-Client für Docker/Podman-Umgebungen über SSH: Container, Stacks, Images, Volumes, Netzwerke und Templates verwalten, inklusive Git-Tracking, Theme-Support und Image-Update-Prüfungen.

## What is containr? (EN)
A terminal UI client for Docker/Podman over SSH: manage containers, stacks, images, volumes, networks, and templates with Git tracking, theming, and image update checks.

---

## Kernfunktionen / Core Features
- Mehrere Server per SSH, Hot-Switching.
- Stacks & Container starten/stoppen/restart/delete; Recreate mit optionalem Pull.
- Templates (Compose) verwalten, deployen, als Git-Repo versionieren.
- Image-Update-Checks mit TTL, Rate-Limit-Banner, optional Autocommit nach Aktionen.
- Registries mit Auth (Keyring bevorzugt, ENV-Fallback, AGE-verschlüsselte Datei möglich).
- Themes laden/wechseln; Sidebar-Theme-Selector.
- Messages-Log mit Save/Copy, Log-Dock optional.

---

## Voraussetzungen / Requirements
- Docker oder Podman auf Ziel-Hosts (remote per SSH).
- Rust-Toolchain (zum Bauen), keyring-Unterstützung des OS (macOS Keychain, Linux Secret Service / libsecret, Windows Credential Manager).
- SSH-Zugang zu den Hosts; `docker`/`podman` muss ohne interaktives Passwort laufen (oder via SSH-identity).

---

## Installation (DE/EN)
```
cargo build --release
# Binary: target/release/containr
```
Optional: Themes liegen unter `themes/`; Templates unter `~/.config/containr/templates` (Stacks/Networks).

---

## Konfiguration / Configuration
Datei: `~/.config/containr/config.json`
- `servers`: Name, target (SSH), optional `docker_cmd`, Identity.
- `templates_dir`: Pfad zu Templates (Stacks/Networks).
- `git_autocommit`, `git_autocommit_confirm`, `image_update_concurrency`, `image_update_autocheck`.
- `active_theme`, `kitty_graphics`, `log_dock_enabled`.
Registries: `~/.config/containr/registries.json` (siehe `docs/registry_auth.md`).

### Registry-Secrets (kurz)
1. Secret in Keyring speichern: `keyring set containr "docker.io/basic"`
2. Im UI: `:registry set docker.io secret-keyring "docker.io/basic"`
3. Auth-Typ/User setzen, testen: `:registry test docker.io`

---

## Bedienung / Usage
- Start: `containr`
- Navigation: Sidebar mit Servern/Views; Fokus wechseln (`b`, `Tab`), Layout Toggle (`C-p`).
- Aktionen per Hotkeys oder `:`-Kommandos (siehe `:help`).
- Messages: `C-g` öffnen/schließen, speichern über `:messages save`.

### Wichtige Kommandos (Auswahl)
- `:server add <name> --target <ssh> [--cmd "<docker_cmd>"]`
- `:stack update` / `:stack recreate --pull`
- `:container start|stop|restart|delete`
- `:template deploy [--recreate] [--pull]`
- `:dashboard (all|single|toggle)`
- `:image check` (TTL 24h) / Marker im UI: grün=ok, gelb=Update, rot=Fehler, blau=Rate-Limit.
- `:git auto on|off` (Autocommit), Anzeige „Commit: auto/manual“ in Header.
- `:theme select` für Theme-Liste/Preview.

---

## Templates
- Ablage: `~/.config/containr/templates/stacks` und `.../networks`.
- Jede `compose.yaml` hat eine `# containr_template_id=...`-Zeile für Deploy-Tracking.
- Deploy-Historie wird lokal protokolliert (Server, Zeit, optional Commit).

---

## Logging & Fehlersuche / Troubleshooting
- Runtime-Meldungen: `:messages` (kopieren/speichern).
- Image-Update-Debug (`image_update_debug=true` in Config) zeigt lokale/remote Digests.
- Registry-Warnungen bei fehlendem Keyring/ENV oder fehlenden Secrets.
- Rate-Limit-Banner für Docker Hub (TTL 6h).

---

## Tests
- Schneller Durchlauf: `cargo test --locked`
- Integration (mit SSH-Host): Feature `integration`, Runner/Server in ENV setzen.

---

## Sicherheit / Security Notes
- Geheimnisse nach Möglichkeit im OS-Keyring; keine Klartext-Secrets im Repo.
- AGE-verschlüsselte Secrets weiterhin möglich, aber nachrangig.
- Keine `expect` in Netzwerk/Parsing-Pfaden (Crash-Vermeidung).

---

## Roadmap (Kurz)
- Siehe `docs/release_plan_security.md` (Pre-Release Check),
  `docs/roadmap-priorities.md` (aktuelle offene Punkte) und
  `docs/readability-refactor-pr-plan.md` (Refactor-Status).
