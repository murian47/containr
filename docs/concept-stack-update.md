# Konzept: Stack Update (Image-basierter Recreate)

Stand: 2026-01-31

Ziel: Ein einzelnes Kommando, das alle Images eines Stacks aktualisiert und nur die betroffenen Services neu erstellt (recreate).

## Kommando

- `:stack update`
- Optionen:
  - `--pull` (default: true) - `compose pull` fuer Stack-Images
  - `--all` - Recreate aller Services, auch ohne Digest-Aenderung
  - `--dry` - nur anzeigen, keine Aenderung
  - `--services <csv>` - eingeschraenkte Service-Auswahl

## Flow (Soll)

1) Stack -> Compose-Kontext finden
   - containr-deployt: `$HOME/.config/containr/apps/<stack>/compose.rendered.yaml`
   - Fremde Stacks: best-effort (compose labels / compose ls)

2) Images + Services ermitteln
   - `docker compose -f <compose> config --images`
   - `docker compose -f <compose> config --services`

3) Pull
   - `docker compose -f <compose> pull` (oder selektiv)

4) Digest-Vergleich (selektiv)
   - `docker inspect --format '{{.Image}}' <container>` -> aktueller Digest
   - `docker image inspect --format '{{.Id}}' <image>` -> neuer Digest
   - Falls unterschiedlich -> Service markieren
   - Fallback bei fehlendem Digest: parse Pull-Output

5) Recreate nur betroffener Services
   - `docker compose -f <compose> up -d --no-deps --force-recreate <svc...>`

6) UI-Feedback
   - Inflight Marker am Stack
   - Messages: Summary + Service-Details

## MVP (Phase 1)

- `:stack update` fuehrt immer aus:
  - `compose pull`
  - `compose up -d --force-recreate` (alle Services)
- Kein Digest-Check, keine Selektivitaet

## Phase 2 (Selektiv)

- Digest-Vergleich + Recreate nur fuer betroffene Services
- `--all` erzwingt Recreate
- `--dry` zeigt geplante Aktionen

## Edge Cases

- Kein Compose-Pfad: Warnung, Aktion abbrechen
- Nicht alle Services laufend: Recreate nur vorhandener Services, Rest warnen
- Podman: compose_cmd pro Server nutzen

## Offene Punkte

- Zuverlaessige Ermittlung von Compose-Services bei fremden Stacks
- Umgang mit mehrfachen Compose-Dateien (override)
