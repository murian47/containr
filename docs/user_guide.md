# containr Benutzerhandbuch / User Guide

> Dieses Handbuch ist zweisprachig aufgebaut: Abschnitte enthalten Deutsch (DE) und Englisch (EN).

---

## 1. Überblick / Overview
- **DE**: containr ist ein Terminal-UI (TUI) zur Verwaltung von Docker/Podman-Hosts über SSH. Es bietet Ansichten für Stacks, Container, Images, Volumes, Netzwerke, Templates, Registries, Logs, Inspect, Themes sowie Nachrichten.
- **EN**: containr is a TUI for managing Docker/Podman hosts over SSH. Views cover stacks, containers, images, volumes, networks, templates, registries, logs, inspect, themes, and messages.

---

## 2. Voraussetzungen / Requirements
- Docker oder Podman auf den Ziel-Hosts, per SSH erreichbar.
- SSH-Schlüsselzugang (passwortlos empfohlen).
- Rust-Toolchain zum Bauen; OS-Keyring-Unterstützung (macOS Keychain, Linux Secret Service, Windows Credential Manager).
- Standardpfade: Config `~/.config/containr/config.json`; Registries `~/.config/containr/registries.json`; Templates `~/.config/containr/templates/{stacks,networks}`.

---

## 3. Aufbau & Navigation / Layout & Navigation
- **Views**: Dashboard, Stacks, Containers, Images, Volumes, Networks, Templates, Registries, Inspect, Logs, Help, Messages, Themes.
- **Fokus**: Sidebar ↔ Liste ↔ Details ↔ Dock (Logs). Umschalten über `Tab`/`Shift-Tab`, `b` (Sidebar), `d` (Details).
- **Layout**: `C-p` toggelt Split (horizontal/vertikal) pro View.
- **Sidebar**: Serverliste, Module, Aktionen. Shortcuts a–z für Server.
- **Quit**: `:q` / `:quit` / `:q!` oder `F10` (falls gemappt).

---

## 4. Server & Verbindungen / Servers & Connections
- **Server anlegen**: `:server add <name> --target <ssh_target> [--cmd "<docker_cmd>"] [--identity <path>]`
  - `<ssh_target>` z.B. `user@host` oder `ssh://user@host:port`.
  - `--cmd` Standard: `docker`; Beispiel mit spaces: `--cmd "sudo docker --config '/etc/docker'"`.
- **Server auswählen**: Sidebar, Enter oder Shortcut.
- **Neu verbinden**: `:server reconnect`
- **Shell**: `:server shell` (öffnet Remote-Shell per SSH, falls konfiguriert).

---

## 5. Stacks
- **Liste**: View „Stacks“. Gestoppte Stacks gedimmt.
- **Aktionen**: Start, Stop, Restart, Delete (`C-s` Start, `C-e` Stop, `C-r` Restart, `C-d` Delete – je nach Keymap; siehe `:help`).
- **Recreate**: `:stack recreate [--pull]` (optional Images vorher ziehen).
- **Update**: `:stack update` aktualisiert alle Container eines Stacks nach aktuellem Template.
- **Details**: Zeigt Container, Netzwerke, Statusindikatoren (laufend, Aktion in progress).

---

## 6. Container
- **Liste**: View „Containers“.
- **Aktionen**: Start, Stop, Restart, Delete, Logs (`l`), Inspect (`i`), Console (`c`), Mark/Unmark (Space).
- **Mehrfachauswahl**: Markieren mit Space, Aktionen wirken auf Markierte.
- **Recreate**: über Stack oder Template (empfohlen), nicht einzeln.

---

## 7. Images
- **Liste**: View „Images“.
- **Aktionen**: Remove, Untag, Inspect.
- **Update-Check**: `:image check` (TTL 24h). Marker: Grün=aktuell, Gelb=Update, Rot=Fehler, Blau=Rate-Limit.

---

## 8. Volumes & Networks
- **Volumes**: Remove, Inspect, Details.
- **Networks**: Remove, Inspect; Stacks zeigen genutzte Netzwerke.

---

## 9. Templates (Compose)
- **Pfad**: `~/.config/containr/templates/stacks` und `.../networks`.
- **Identifikation**: Erste Zeile `# containr_template_id=<uuid>` für Deploy-Tracking.
- **Liste**: View „Templates“. Details zeigen Beschreibung (`#description ...`), Pfad, Deploy-Historie.
- **Deploy**: `:template deploy [--recreate] [--pull]` (nutzt aktuelle Auswahl). Ziel-Server = aktiver Server.
- **Editieren**: Enter öffnet Template im Editor (Konfig: `editor_cmd`, sonst `$EDITOR`, sonst `vi`).
- **Git**: Templates-Verzeichnis kann Git-Autocommit nutzen (siehe Abschnitt 14).

---

## 10. Registries & Auth
- **Datei**: `registries.json`.
- **Auth-Typen**: `anonymous`, `basic`, `bearer-token`, `github-pat`.
- **Keyring bevorzugt**:
  1. Secret im OS-Keyring speichern: `keyring set containr "<host>/<label>"`
  2. UI: `:registry set <host> secret-keyring "<host>/<label>"`
  3. User/Type setzen: `:registry set <host> auth basic`, `:registry set <host> user <name>`
- **Fallbacks**: ENV mit gleichem Namen (bei Keyring-Fehler), danach AGE-verschlüsseltes `secret` im File.
- **Test**: `:registry test <host>`; Warnungen in `:messages`.
- Details siehe `docs/registry_auth.md`.

---

## 11. Themes
- **Wechseln**: `:theme select` → Sidebar-Liste, Preview im Main-Pane.
- **Dateien**: `themes/*.toml` (import aus ghostty-Themes möglich).
- **Aktives Theme**: in `config.json` (`active_theme`).

---

## 12. Messages & Logs
- **Öffnen**: `C-g` oder `:messages`.
- **Speichern**: `:messages save <pfad>` (optional), Kopieren: Auswahl → `y`.
- **Log-Dock**: `log_dock_enabled` in Config; Anzeigen unterhalb der Views.

---

## 13. Image-Updates
- **Check**: `:image check` (Respektiert TTL 24h, Konfig `image_update_autocheck`, `image_update_concurrency`, `image_update_debug`).
- **Marker**: Grün=OK, Gelb=Update, Rot=Fehler, Blau=Rate-Limit.
- **Rate-Limit**: Banner für Docker Hub (Fenster 6h, Limit 100 anon Pulls); Auth reduziert Limit-Probleme.

---

## 14. Git-Autocommit (Templates)
- **Schalter**: `:git auto on|off` (Config `git_autocommit`, `git_autocommit_confirm`).
- **Anzeige**: Header „Commit: auto|manual“ (versteckt wenn Git nicht verfügbar).
- **Warnungen**: Wenn Repo nicht initialisiert oder Git nicht installiert → Messages Warnung, Aktionen laufen weiter.

---

## 15. Einstellungen / Settings (Config)
Wichtige Felder in `config.json`:
- `servers` (Name, target, port, identity, docker_cmd)
- `templates_dir`, `active_theme`, `editor_cmd`
- `git_autocommit`, `git_autocommit_confirm`
- `image_update_concurrency`, `image_update_autocheck`, `image_update_debug`
- `kitty_graphics`, `log_dock_enabled`, `log_dock_height`
- `view_layout` pro View (`horizontal|vertical`)
- `keymap` (eigene Shortcuts, z.B. `{"key":"F10","scope":"global","cmd":":q!"}`)

---

## 16. Keyboard-Shortcuts (Standard, Auszug)
- Navigation: `Tab`/`Shift-Tab` Fokus, `b` Sidebar, `d` Details.
- Layout: `C-p` Split toggeln.
- Views: `t` Templates, `c` Containers, `s` Stacks, `i` Images, `n` Networks, `v` Volumes, `r` Registries, `h` Help, `g` (C-g) Messages.
- Aktionen (je nach View aktiv):  
  - Start `S`, Stop `E`, Restart `R`, Delete `D`  
  - Logs `l`, Inspect `i`, Console `c`  
  - Mark `Space`
- Kommandolinie: `:` öffnet Command-Prompt.
- Quit: `:q`, `:q!`, ggf. `F10` (Keymap).

---

## 17. Troubleshooting
- Keine Verbindung: `:messages` prüfen; `docker_cmd`/SSH-Identity kontrollieren.
- Panics: sollten vermieden sein; bitte Log aus `:messages` melden.
- Image-Check rot/blau: Rate-Limit oder fehlende Digests; ggf. Registry-Auth setzen.
- Template nicht „deployed“ markiert: `# containr_template_id` im Template fehlt → Redeploy erzeugt ID.

---

## 18. Tests
- Lokal: `cargo test --locked`
- Optional Integration (mit SSH-Host): Feature `integration`, Runner/Host per ENV (siehe Projekt-Doku, falls aktiviert).

---

## 19. Sicherheit / Security
- Secrets in Keyring speichern, nicht im Klartext.
- AGE-verschlüsselte Secrets werden unterstützt, aber nachrangig.
- Rate-Limits respektieren; Banner beachten.

---

## 20. Roadmap
- Pre-Release-Sicherheit & Stabilität: siehe `docs/release_plan_security.md`
- UI-Refactor (Render-Splitting, Services): siehe `docs/ui_refactor_plan.md`
