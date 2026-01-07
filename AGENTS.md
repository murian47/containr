# DOS Shell (Simulation)

## Projektuebersicht
- Rust‑App, die eine DOS‑aehnliche Shell simuliert (keine Emulation).
- Fokus: DOS‑Prompt, interne Kommandos, Drive‑Mapping, DOS‑Pfadlogik.
- Hauptprojektpfad: `linux/dosshell`

## Abhaengigkeiten (Linux)
- Rust Toolchain (stable) inkl. `cargo`.
- Terminal mit TTY (Raw‑Mode fuer Line‑Editor).

## Projekt holen (Linux)
1) In das Projektverzeichnis wechseln:
   - `cd /pfad/zum/projekt/linux/dosshell`

## Build & Run
- Build: `cargo build`
- Run: `cargo run`
- Hinweis: Line‑Editor nutzt Raw‑Mode, daher in einem echten Terminal starten.

## Konfiguration
Datei: `linux/dosshell/dos_shell.ini`

Beispiel:
```
initial_drive=C

[drives]
C=.
D=/home/user/DOSROOT

[labels]
C=DOSSHELL
D=DATA

[serials]
C=0000-1234

[locale]
# locale=de-DE
# lang=de
# region=DE
```

## i18n
- Sprachdateien: `linux/dosshell/i18n/en.ini`, `linux/dosshell/i18n/de.ini`
- Struktur:
  - `[strings]` fuer Kurztexte
  - `[help]` fuer Hilfeausgaben (`\n` fuer Zeilenumbrueche)
- Sprache kommt aus OS‑Locale, optional via `[locale]` ueberschreibbar.

## Aktuelle Features (Auszug)
- Prompt: DOS‑Stil (`C:\...>`), Pfade in 8.3‑Kurzform.
- 8.3‑Kurzname‑Mapping pro Verzeichnis (eindeutig, Windows‑95‑Stil `~n`).
- Drive‑Mapping + Drive‑Wechsel (`C:`).
- Interne Kommandos: `DIR`, `CD/CHDIR`, `TYPE`, `DEL/ERASE`, `COPY`,
  `MD/MKDIR`, `RD/RMDIR`, `EXIT`.
- `DIR` Format im DOS‑Stil inkl. Volume‑Header, Datum/Zeit, Summary.
- `DIR /W` zeigt Verzeichnisse als `[DIRNAME]`.
- `/?` Hilfe in DOS‑Stil (aus i18n‑Dateien).

## Entwicklungsnotizen
- Dateinamen werden case‑insensitiv aufgeloest.
- 8.3‑Kurzformen duerfen als Parameter genutzt werden.
- Locale beeinflusst Datum/Zeit und Tausendertrennzeichen.
