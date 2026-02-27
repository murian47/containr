## Plan: Aufspaltung von `render.inc.rs` und Entkopplung UI/Logik

Status: historical reference (major parts completed; `render.inc.rs` no longer exists).
Aktueller Arbeitsstand: `docs/readability-refactor-pr-plan.md` und `docs/code-map-ui.md`.

Ziel: Wartbarkeit und Testbarkeit erhöhen, ohne die App längerfristig unbenutzbar zu machen. Schritte sind so geschnitten, dass nach jedem Meilenstein `cargo test` läuft und die TUI startet.

### Phase 1 – Struktur trennen, keine Logik ändern
1. **Dateien pro View**: render_dashboard.rs, render_stacks.rs, render_containers.rs, … (nur Kopien/Exports, Aufrufe bleiben in `render.inc.rs`), sodass späteres Umschalten nur Requires/uses benötigt.
2. **Common UI-Helfer**: Footer-Hints, Sidebar-Builder, Status-Badges in `ui/render_helpers.rs`.
3. **Styling/Theme Utilities**: Farb-/Style-Funktionen in `ui/style.rs`.

### Phase 2 – Call-Sites umstellen
1. `render.inc.rs` ruft die neuen Modul-Funktionen auf; alter Code wird entfernt.
2. Pro View einzeln umschalten (Dashboard → Stacks → Containers → Images → Volumes → Networks → Templates → Registries → Inspect/Logs/Help).
3. Nach jeder Umschaltung: `cargo fmt && cargo test --lib ui::tests::smoke` (Mock-Tests).

### Phase 3 – Logik aus UI ziehen
1. **Action-Services**: Start/Stop/Restart/Delete (Container/Stack), Pull/Recreate, Template-Deploy in `services/*.rs`.
2. UI ruft nur noch Service-Fassade, State-Updates bleiben im UI-State.
3. Fehler- und Success-Meldungen einheitlich über ein Message-API.

### Phase 4 – Test & Cleanup
1. Clippy auf UI/Services (`cargo clippy --all-targets -- -D warnings`).
2. Entfernte Dead-Code-Pfade? → weg.
3. Dokumentation: Kurze Entwickler-Notiz, wie neue Views/Actions ergänzt werden.

### Guardrails
- Nach jedem Teil-Schritt lauffähig halten (smoke-test + kurzer manueller Start).
- Keine neuen Monolithen: Module < ~500 Zeilen anstreben, gemeinsame Helfer in kleinen, fokussierten Dateien halten.
