# containr Benutzerhandbuch (DE)

## 1. Überblick
- containr ist ein Terminal-UI (TUI) zur Verwaltung von Docker/Podman-Hosts über SSH.
- Es bietet Ansichten für Stacks, Container, Images, Volumes, Netzwerke, Templates, Registries, Logs, Inspect, Themes und Messages.

## 2. Voraussetzungen
- Docker oder Podman auf den Ziel-Hosts, per SSH erreichbar.
- SSH-Schlüsselzugang (passwortlos empfohlen).
- Rust-Toolchain zum Bauen; OS-Keyring-Unterstützung (macOS Keychain, Linux Secret Service, Windows Credential Manager).
- Standardpfade:
  - Config: `~/.config/containr/config.json`
  - Registries: `~/.config/containr/registries.json`
  - Templates: `~/.config/containr/templates/{stacks,networks}`

## 3. Aufbau & Navigation
- Views: Dashboard, Stacks, Containers, Images, Volumes, Networks, Templates, Registries, Inspect, Logs, Help, Messages, Themes.
- Fokus: Sidebar ↔ Liste ↔ Details ↔ Dock (Logs), Wechsel über `Tab`/`Shift-Tab`, `b`, `d`.
- Layout: `C-p` toggelt Split (horizontal/vertikal) pro View.
- Quit: `:q` (mit y/n-Bestätigung) oder `:q!` (sofort).

## 4. Server & Verbindungen
- Server anlegen:
  - `:server add <name> --target <ssh_target> [--cmd "<docker_cmd>"] [--identity <path>]`
- Server auswählen: Sidebar + Enter oder Shortcut.
- Neu verbinden: `:server reconnect`
- Shell: `:server shell` (öffnet Remote-Shell per SSH, falls konfiguriert).

## 5. Stacks
- Liste: View `Stacks`.
- Aktionen: Start, Stop, Restart, Delete.
- Recreate: `:stack recreate [--pull]`
- Update: `:stack update`

## 6. Container
- Liste: View `Containers`.
- Aktionen: Start, Stop, Restart, Delete, Logs, Inspect, Console, Mark/Unmark.
- Mehrfachauswahl: Space markiert, Aktionen wirken auf Markierte.

## 7. Images
- Liste: View `Images`.
- Aktionen: Remove, Untag, Inspect.
- Update-Check: `:image check` (TTL 24h).

## 8. Volumes & Networks
- Volumes: Remove, Inspect, Details.
- Networks: Remove, Inspect.

## 9. Templates (Compose)
- Pfad: `~/.config/containr/templates/stacks` und `.../networks`.
- Identifikation: `# containr_template_id=<uuid>` für Deploy-Tracking.
- Deploy: `:template deploy [--recreate] [--pull]`
- Editieren: Enter öffnet im Editor (`editor_cmd` -> `$EDITOR` -> `vi`).
- Git: Templates-Verzeichnis kann Git-Autocommit nutzen.

## 10. Registries & Auth
- Datei: `registries.json`
- Auth-Typen: `anonymous`, `basic`, `bearer-token`, `github-pat`
- Keyring bevorzugt:
  1. `keyring set containr "<host>/<label>"`
  2. `:registry set <host> secret-keyring "<host>/<label>"`
  3. `:registry set <host> auth basic` / `:registry set <host> user <name>`
- Test: `:registry test <host>`

## 11. Themes
- Wechseln: `:theme select`
- Dateien: User-Overrides unter `~/.config/containr/themes/*.json`
- Built-in-Themes: aus `themes/` relativ zum Bundle/Workspace oder aus Systempfaden wie `/usr/share/containr/themes`
- Aktives Theme: `active_theme` in `config.json`

## 12. Messages & Logs
- Öffnen: `C-g` oder `:messages`
- Speichern: `:messages save <pfad>`
- Log-Dock: `log_dock_enabled` in Config

## 13. Image-Updates
- Check: `:image check`
- Marker: Grün=OK, Gelb=Update, Rot=Fehler, Blau=Rate-Limit

## 14. Git-Autocommit (Templates)
- `:git auto on|off`
- Header: `Commit: auto|manual`

## 15. Einstellungen (Config)
Wichtige Felder:
- `servers`, `templates_dir`, `active_theme`, `editor_cmd`
- `git_autocommit`, `git_autocommit_confirm`
- `image_update_concurrency`, `image_update_autocheck`, `image_update_debug`
- `kitty_graphics`, `log_dock_enabled`, `log_dock_height`
- `view_layout`, `keymap`

## 16. Keyboard-Shortcuts (Auszug)
- Navigation: `Tab`/`Shift-Tab`, `b`, `d`
- Layout: `C-p`
- Global: `F1` Help, `C-g` Messages, `C-b` Sidebar toggle
- Containers: `C-s`, `C-o`, `C-r`, `C-d`, `C-l`, `C-i`
- Stacks: `C-s`, `C-o`, `C-r`, `C-d`, `C-u`
- Templates: `C-e`, `C-n`, `C-y`, `C-S-Y`

## 17. Troubleshooting
- Verbindung: `:messages` prüfen; `docker_cmd`/SSH-Identity kontrollieren.
- Image-Check rot/blau: Rate-Limit oder fehlende Digests; Registry-Auth setzen.

## 18. Tests
- Lokal: `cargo test --locked`
- Optional Integration: Feature `integration`

## 19. Sicherheit
- Secrets in Keyring speichern, nicht im Klartext.
- AGE-verschlüsselte Secrets werden unterstützt, aber nachrangig.

## 20. Roadmap
- `docs/release_plan_security.md`
- `docs/roadmap-priorities.md`
- `docs/readability-refactor-pr-plan.md`
