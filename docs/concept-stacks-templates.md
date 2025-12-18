# Konzept: Stacks/Compose-Templates + Git + Deploy (Entwurf)

Stand: 2025-12-16

Dieses Dokument beschreibt das geplante Erweiterungskonzept fuer das Tool (aktueller Name: "containr"; vorheriger Arbeitsname: "mcdoc").
Ziel ist, zusaetzlich zu Monitoring/Aufraeumen auch Stacks/Compose-Projekte zu erstellen, zu versionieren (Git) und auf Servern zu deployen.

## Ziele

- Stacks/Container auf Servern (SSH und lokal) erstellen/aktualisieren/starten/stoppen.
- YAML-Templates lokal verwalten und per Git versionieren.
- Git wird nicht neu implementiert: Git-Operationen werden durch das Tool ausgefuehrt und im UI angezeigt.
- Deploy: Template wird lokal gerendert (inkl. Label-Injection), dann auf Zielmaschine kopiert und per `docker compose ... up -d` gestartet.
- Portainer-kompatible Projekt/Stack-Verwaltung durch passende Labels.
- Kein integrierter Editor: Bearbeitung erfolgt ueber `$EDITOR`.

## Grundannahmen / Rechte

- Auf Remote-Servern koennen wir nur sicher im Home-Verzeichnis des SSH-Users schreiben.
- Daher werden Remote-Deployments unter `$HOME/.config/<program>/...` abgelegt (kein `/opt`, kein `sudo`).
- Docker/Podman muss fuer den SSH-User ohne `sudo` nutzbar sein (Docker group / rootless Podman). Das Tool soll das pro Server pruefen und anzeigen.

## Begriffe / Datenmodell

### Server

- `name`
- `runner`: `ssh` oder `local`
- `target`: z.B. `user@host` oder `local`
- optional `identity`, `port`
- `docker_cmd`: z.B. `docker` oder `podman`
- optional `compose_cmd` (Default: `docker compose`)

### Template (lokal, Git-versioniert)

Ein Template ist ein Ordner im lokalen Templates-Repo, z.B.:

- `templates/<template-name>/compose.yaml` (oder mehrere Compose-Dateien)
- optional `.env.example` (in Git)
- optional `.env` (standardmaessig nicht in Git; optional fuer Deploy)
- optional `template.json` / `template.toml` fuer Metadaten (Beschreibung, Default-Stackname, Variablen, Hinweise)

### Stack (Deployment-Instanz)

- `stack_name`: Name des Deployments (Compose project name / Portainer stack name)
- `template_ref`: (template + git ref: commit/tag/branch)
- `server_ref`
- `remote_path`: Zielpfad unter dem Remote-Home (siehe Remote Layout)
- `state`: letzter Deploy (commit, zeit), letzter Fehler, optional history

## Lokales Layout

- Konfiguration: `$XDG_CONFIG_HOME/<program>/config.json`
- Zustand/History: `$XDG_STATE_HOME/<program>/state.json`
- Templates-Repo: `$XDG_CONFIG_HOME/<program>/templates/` (ein Git-Repo fuer alle Templates)

## Remote Layout (pro Server, pro SSH-User)

Basis:

- `$HOME/.config/<program>/`

Pro Stack:

- `$HOME/.config/<program>/apps/<stack_name>/compose.rendered.yaml`
- optional `$HOME/.config/<program>/apps/<stack_name>/.env` (nur opt-in)
- `$HOME/.config/<program>/apps/<stack_name>/meta.json` (deployed template/ref/timestamp, optional checksums)
- optional `$HOME/.config/<program>/apps/<stack_name>/data/` (spaeter, falls wir bind-mounts unter HOME anbieten wollen)

Compose-Ausfuehrung auf dem Remote-Host:

- `cd "$HOME/.config/<program>/apps/<stack_name>" && <compose_cmd> -f compose.rendered.yaml up -d`

## Label-Injection (Portainer/Compose-Kompatibilitaet)

Vor dem Deploy:

1. compose.yaml einlesen
2. Pro Service Labels setzen/ergaenzen

Ziel:

- Portainer/Compose sollen Stacks eindeutig erkennen und gruppieren koennen.
- Zusaetzlich soll nachvollziehbar sein, welches Template/ref deployed wurde.

Beispiel-Labels (finaler Satz ist konfigurierbar und wird spaeter festgezurrt):

- Compose/Project:
  - `com.docker.compose.project=<stack_name>` (Compose setzt vieles selbst; wir ergaenzen nur falls noetig)
- Portainer:
  - `io.portainer.stack.name=<stack_name>` (variabel; Portainer-Konventionen koennen je nach Version differieren)
- Tool-intern:
  - `app.<program>.template=<template-name>`
  - `app.<program>.commit=<git-sha>`

## UI/Views (TUI)

### Templates (lokal)

- Liste Templates
- Anzeige Git-Status: clean/dirty, branch, HEAD short sha
- Aktionen:
  - Open in `$EDITOR`
  - Git: status / diff / log / commit / pull --rebase / push
  - New template (scaffold)
  - optional Validate (z.B. `docker compose config` lokal, falls verfuegbar)

### Stacks (serverbezogen)

- Liste Stacks auf dem aktiven Server (Erkennung ueber Compose/Portainer Labels)
- Aktionen:
  - Deploy (Template -> Server -> Stackname)
  - Update (redeploy wenn neuerer commit)
  - Start/Stop/Restart (compose oder container-level je nach Datenlage)
  - Pull + Up (compose pull && compose up -d)
  - Inspect/Logs als eigene Main-Views (wie heute)

### Deploy (Wizard im Main-Bereich, keine Overlays)

1. Template waehlen
2. Server waehlen
3. Stackname + Remote-Pfad bestaetigen
4. Git-Ref waehlen (HEAD/Tag/Commit)
5. Preview der gerenderten Compose-Datei (readonly)
6. Deploy starten

## Ausfuehrungsmodell (nicht blockierend)

- Alle laengeren Operationen (ssh, git, deploy) laufen in Background-Tasks.
- UI zeigt:
  - Fortschritt/Fehler im Messages-View
  - In-flight Marker am betroffenen Stack (aehnlich wie heute bei Containeraktionen)

## Deploy-Algorithmus (Details)

1. Template bei gewaehltem Git-Ref exportieren (bevorzugt: `git archive <ref>` in temp dir; keine Aenderung am Working Tree).
2. compose.yaml lesen, Labels injizieren, ggf. Variablen aufloesen (spaeter).
3. Files nach Remote kopieren:
   - bevorzugt `tar` over ssh oder `rsync`, fallback `scp -r`
4. Remote: `compose up -d` (optional: vorher `compose pull`)
5. Remote `meta.json` schreiben (commit/template/time)
6. Lokal `state.json` aktualisieren

## Git-Integration

Git bleibt "as-is"; das Tool ruft nur Kommandos auf und zeigt Ausgaben an:

- `git status --porcelain`
- `git diff`
- `git log`
- `git commit -m ...`
- `git pull --rebase`
- `git push`

## Migration (spaeter, geplant)

Ziel: Container/Stacks zwischen Hosts migrieren (optional inkl. Daten).

Moegliche Bausteine:

- Images: pull auf Ziel (oder `docker save/load` als fallback)
- Volumes: tar-basierter Export/Import (oder rsync bei bind mounts, auf zfs ggf. zfs snapshot dann send recv)
- Stack: Template auf Ziel deployen, Daten migrieren, Cutover

Wichtig: Dry-run, klare Prompts und Sicherheitschecks.

## Milestones (Vorschlag)

- M1: Templates-View + Git-Aktionen + `$EDITOR` Integration
- M2: Stacks-View (Stacks erkennen, anzeigen, simple lifecycle actions)
- M3: Deploy (render + copy + compose up) inkl. state/history
- M4: Update/Rollback (select ref, redeploy, history UI)

## Offene Fragen

- Exakter Portainer-Labelsatz (Version/Setup abhaengig) und Erkennungslogik fuer Stacks.
- Umgang mit Secrets (`.env` opt-in? encrypted? out-of-scope?)
- Remote-Pfad Naming/Sanitizing fuer `stack_name` (erlaubte Zeichen, Kollisionen, rename).
- Podman/Compose Kompatibilitaet (compose command pro server konfigurierbar).
