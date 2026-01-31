# Plan: Stack Update MVP

Stand: 2026-01-31

Ziel: Ein Kommando `:stack update`, das fuer den aktiven Stack `compose pull` und `compose up -d --force-recreate` ausfuehrt.
Kein Digest-Check, keine selektiven Services (Phase 2 folgt spaeter).

## Schritte (MVP)

1) Command + Hilfe
   - `:stack update [--pull] [--dry]`
   - Command registrieren und in Help aufnehmen

2) Compose-Kontext ermitteln
   - Helper: `stack_compose_path(stack)`
   - Pfad: `$HOME/.config/containr/apps/<stack>/compose.rendered.yaml`
   - Wenn nicht vorhanden: Warnung + Abbruch

3) Task/Runner
   - Action `UpdateStack`
   - Ablauf:
     - optional `compose pull` (default an)
     - `compose up -d --force-recreate`

4) UI Feedback
   - Inflight Marker am Stack
   - Messages: Start + Ende (ok/fehler)

5) Cleanup + Refresh
   - Marker entfernen
   - Container/Stacks refresh anstossen

## Optional (MVP+)

- `--dry` -> loggt nur die geplanten Kommandos, fuehrt nichts aus
- `--pull=false` -> Skip pull

## Phase 2 (nach MVP)

- Digest-Vergleich je Service
- Selektives Recreate
- `--all` fuer komplettes Recreate
- `--services <csv>` fuer explizite Auswahl
