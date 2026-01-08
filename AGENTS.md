# Agent Instructions – containr

## Project Scope
- TUI‑basierter Docker/Stack/Template Manager in Rust.
- Hauptpfad: `linux/containr`.
- Netzwerkzugriff: laut Vorgaben der CLI (hier: restricted).

## Build & Test
- Build/Test lokal: `cargo test`
- Run (debug): `cargo run`
- Keine destruktiven Git-Kommandos ohne Aufforderung; Versionsbump nur bei Code/Theme-Änderungen.

## Strukturhinweise
- UI liegt unter `src/ui/` (`render.inc.rs` + Modul-Splits `render/`).
- Domain/Runner/SSH/Docker-Logik ist mit dem UI verzahnt; Refactor-Plan siehe `docs/ui-logic-separation-plan.md`.
- Themes unter `themes/`; Templates unter `~/.config/containr/templates`.

## Arbeitsregeln
- Antworten an den User auf Deutsch; Code-Kommentare/Doku in Englisch.
- Im Zweifel nachfragen, wenn Destruktives nötig wäre.
- Bei refactors schrittweise vorgehen, App lauffähig halten.
- Bei Codeänderungen vor dem Commit den Patch-Level der Version um 1 erhöhen, außer die Version wurde vorher explizit gesetzt.
- Nach größeren Änderungen Tests ausführen (`cargo test`), bevor weitergearbeitet oder committed wird.
- Refactors schrittweise, App lauffähig halten; für utils-Aufteilung (text/scroll/fs) später nachschärfen, aktuell bleiben die Helfer in `render/utils.rs`.
