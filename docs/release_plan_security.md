## Release-Plan (Sicherheit & Stabilität)

Ziel: Vor der ersten Veröffentlichung kritische Risiken minimieren und Nutzer vor Datenverlust oder Abstürzen schützen.

### 1) Geheimnisse und Credentials
- Registry-Credentials nicht im Klartext speichern: OS-Keyring bevorzugen; mindestens Maskierung und keine Ausgabe in Logs oder State.
- Optional: pro Registry klare UI-Hinweise, wenn Anmeldedaten fehlen oder unsicher gespeichert werden.

### 2) Absturzsicherheit
- Dashboard-Parsing robuster machen (keine `expect` auf ungeprüfte Daten).
- Hintergrund-Worker für Docker-Overview: Rennen in `tokio::select!` vermeiden (kein `expect` auf bereits konsumierte Child-Prozesse).
- Crash-Monitoring: Fehlerpfade sammeln, in Messages anzeigen, nicht paniken.

### 3) Netz- & Registry-Zugriffe
- Rate-Limits früh erkennen, Warnbanner behalten; vor dem Limit warnen.
- Optional: Auth-Unterstützung pro Registry, damit Updates nicht an 429 scheitern.

### 4) Tests & CI
- Lokale Smoke-Tests ohne SSH/Remote (Mock Runner) ergänzen.
- Clippy + rustfmt + ausgewählte Integrationstests in CI (Matrix: Linux x64, arm64 Cross-Check).
- Schnelltest-Kommando dokumentieren (z. B. `cargo test --lib ui::integration_tests::smoke`).

### 5) Wartbarkeit vor Release
- Monolith `render.inc.rs` in view-spezifische Module aufteilen.
- Services für Aktionen (Start/Stop/Deploy) von UI entkoppeln.
- Dokumentation: kurze „How to Contribute“ + Coding-Guidelines (Fehlerbehandlung, Logging, Tests).

### 6) Deploy/Template-Nachvollziehbarkeit
- Template-ID/Deploy-Historie konsistent (fehlende IDs erkennen und reparieren).
- Logging: Jede Aktion (deploy/recreate/pull) mit Server, Commit und Ergebnis protokollieren.

### 7) Build & Release
- Reproduzierbarer Release-Build (Release-Profile, Strip, ggf. LTO).
- Binary-Größe prüfen; Feature-Gates für optionale Teile (z. B. Kitty-Graphics) erwägen.

### 8) Pre-Release Checkliste
- Tests/Clippy/Format durchlaufen.
- Keine offenen Panics/`expect` in Runtime-Pfaden.
- Secrets nicht im Repo/Logs/State.
- Templates-Verzeichnis validiert (ID, Labels, Deploy-Status).
