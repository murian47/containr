# containr

Lokales TUI-Dashboard im Midnight-Commander-Stil fuer Docker-Container auf Remote-Hosts via `ssh` (oder lokal).

## Offiziell unterstuetzte Plattformen

- Linux: Hauptzielplattform fuer lokale und entfernte Docker/Podman-Workflows
- macOS: unterstuetzte Entwicklungs- und Client-Plattform, inklusive lokalem Docker-Setup

Nicht Teil des `0.5.0`-Supports:

- `*BSD`
- Windows

Andere Plattformen koennen im Einzelfall funktionieren, gelten derzeit aber nur als best effort.

## Voraussetzungen

- macOS/Linux lokal: `ssh` im PATH (Keys/Agent/`~/.ssh/config` wie ueblich)
- Remote: Docker installiert; dein SSH-User darf `docker ps` und `docker stats` ausfuehren
  - falls noetig: `--docker-cmd "sudo docker"` (und sudo ohne Passwort)

## Start

```bash
cd linux/containr
cargo run -- --target user@server
```

Optional:

```bash
cargo run -- --target user@server --refresh-secs 2
cargo run -- --target user@server --docker-cmd "sudo docker"
cargo run -- --target user@server --identity ~/.ssh/id_ed25519 --port 2222
cargo run -- --target user@server --mouse
cargo run -- --target user@server --ascii-only
```

## Release Build

```bash
cd linux/containr
cargo build --release
```

## macOS Installation Script

Fuer eine lokale macOS-Installation aus dem aktuellen Source-Tree:

```bash
cd linux/containr
./packaging/macos/install.sh
```

Default-Installationspfade:

- Binary-Payload: `/usr/local/libexec/containr/containr`
- Wrapper: `/usr/local/bin/containr`
- Themes: `/usr/local/share/containr/themes`

Optional:

```bash
./packaging/macos/install.sh --prefix "$HOME/.local"
./packaging/macos/install.sh --skip-build --source-binary target/release/containr
```

Deinstallation:

```bash
./packaging/macos/uninstall.sh
./packaging/macos/uninstall.sh --prefix "$HOME/.local"
./packaging/macos/uninstall.sh --keep-themes
```

## Linux Installation Script

Fuer eine lokale Linux-Installation aus dem aktuellen Source-Tree:

```bash
cd linux/containr
./packaging/linux/install.sh
```

Default-Installationspfade:

- Binary: `/usr/local/bin/containr`
- Themes: `/usr/local/share/containr/themes`

Optional:

```bash
./packaging/linux/install.sh --prefix "$HOME/.local"
./packaging/linux/install.sh --skip-build --source-binary target/release/containr
```

Deinstallation:

```bash
./packaging/linux/uninstall.sh
./packaging/linux/uninstall.sh --prefix "$HOME/.local"
./packaging/linux/uninstall.sh --keep-themes
```

## Serverliste (lokal)

Containr kann eine lokale Konfiguration laden/speichern:

- Pfad: `$XDG_CONFIG_HOME/containr/config.json`
- Fallback: `$HOME/.config/containr/config.json`

Legacy: falls die neue Datei nicht existiert, werden auch `servers.json`/`serverlist.json` (containr/mcdoc/dockdash) eingelesen.

Beispiel:

```json
{
  "version": 1,
  "last_server": "rpi5",
  "servers": [
    {
      "name": "rpi5",
      "target": "mag@rpi5",
      "port": 22,
      "identity": "~/.ssh/id_ed25519",
      "docker_cmd": "docker"
    }
  ]
}
```

Wichtig: Es werden keine Passwoerter/Secrets gespeichert (nur SSH-Ziel/Port/Identity-Pfad und optional `docker_cmd` wie `"sudo docker"`).

Verwendung:

- `cargo run -- --server rpi5` nutzt einen Eintrag aus der Serverliste.
- `--target ...` verbindet direkt; das Target wird (wenn noetig) in die Liste geschrieben/aktualisiert.
- In der TUI: `F1` oeffnet die Serverauswahl. Die Auswahl wird als `last_server` gespeichert.

### Local Docker (ohne SSH)

Als Target kann auch `"local"` verwendet werden (dann wird lokal `docker ...` ausgefuehrt):

```json
{
  "name": "local",
  "target": "local",
  "docker_cmd": "docker"
}
```

## Features

- MC-UI: Function-Key-Leiste (`F1..F10`), Dialoge mit OK/Cancel, Maus optional
- Remote-Ausfuehrung ueber System-`ssh` (keine SSH-Library)
- Containerliste inkl. `docker stats` (CPU/Mem) + Ports
- IP-Anzeige fuer laufende Container (aus `docker inspect`, batched, gecached)
- Health-Highlighting aus `STATUS` (healthy/unhealthy/starting)
- Action-Status: Start/Stop/Restart zeigt einen Spinner am Container, bis sich der Status konsistent aktualisiert
- Dual-Pane: links Liste, rechts Details (Toggle via `Ctrl-D`)
- Inspect-Viewer (JSON Tree) mit Folding, Suche, Copy/Path
- Logs-Viewer mit Suche + Highlight, Home/End, Scrollbar
- Server-Shell als echte interaktive Shell ausserhalb der TUI (Container-Console ist vorbereitet, wird spaeter ins Menu integriert)
- Multi-Select/Bulk Actions (Markierungen ueber Refresh hinweg)
- Stack/Tree-View: Compose/Swarm Stacks als Header + Expand/Collapse

## UI-Modi

### Flat vs Tree (Stacks)

Tree-View gruppiert Container nach Stack-Namen aus Labels:

- Compose: `com.docker.compose.project`
- Swarm: `com.docker.stack.namespace`

In Tree-View erscheinen Stacks als uebergeordnete Zeilen (Header) und Container darunter eingerueckt.

### Split Details (angedockt)

`Ctrl-D` schaltet zwischen:

- Dialog-Details (per Enter auf Container) und
- angedockten Details (rechts neben der Liste) um.

In Split-Mode werden in der Liste einige Spalten ausgeblendet (um Platz fuer Details zu schaffen).

### Markierungen (Bulk Select)

- Markierte Container bleiben ueber Refresh-Zyklen erhalten.
- Serverwechsel verwirft alle Markierungen.

## Keybindings

### Global / Main Screen

Menu:

- `F9` Menu (wie Midnight Commander), Navigation mit `Left/Right/Up/Down`, `Enter` waehlt, `Esc` schliesst
- `Left`/`Right` im Menu waehlt das Ziel-Pane, `Down` oeffnet das Submenu; dort: `Details/Containers/Images/Volumes/Networks`
- `Tab` wechselt den aktiven Pane (Fokus)
- `Ctrl-W` swaps Left/Right panes (Fallback: `Ctrl-,` / `Ctrl-<` je nach Terminal)

- `F1` Servers
- `F2` Inspect (auf Container)
- `F3` Logs (auf Container)
- `F4` Actions (auf Auswahl / Marks / Stack)
- `F5` Refresh
- `F7` Console (Container exec)
- `F8` SSH (Server Shell)
- `F10` Exit (funktioniert immer, auch in Overlays)

Fallback fuer Terminals ohne F-Tasten:

- `Esc-1..Esc-9` => `F1..F9` (nur Hauptscreen)
- `Esc-0` => `F10` (immer)

Falls `Alt+<Buchstabe>` vom Terminal nicht als Modifier gesendet wird, funktionieren die Menue-Mnemonics auch als `Esc` dann Buchstabe (innerhalb ~2s): `Esc-v`, `Esc-c`, `Esc-i`, `Esc-o`, `Esc-n`.

Navigation:

- `Up/Down` oder `j/k` Auswahl
- `PageUp/PageDown` scrollt
- `Home/End` (wo unterstuetzt) springt

### Split/Tree/Marking

- `Ctrl-D` Toggle Dual-Pane (Details rechts)
- `Ctrl-T` oder `Ctrl-S` Toggle Tree/Stack-View
- `Ctrl-E` Expand/Collapse all Stacks (synchron)
- `Space`
  - auf Stack-Header: expand/collapse
  - auf Container: mark/unmark
- `Ctrl-A` Mark all containers
- `Ctrl-N` Clear all marks

### Actions Dialog

- `Up/Down` Aktion waehlen
- `Left/Right` OK/Cancel waehlen
- `Enter` OK fuehrt aus, Cancel schliesst
- `Esc` schliesst

Actions Scope:

- Wenn ein Stack-Header selektiert ist: Action geht auf alle Container des Stacks.
- Sonst, wenn Markierungen existieren: Action geht auf alle markierten Container.
- Sonst: Action geht auf den selektierten Container.

### Servers Dialog

- `Up/Down` Server waehlen
- `Left/Right` OK/Cancel waehlen
- `Enter` OK aktiviert Server, Cancel schliesst
- `Esc` Cancel

### Inspect Dialog (JSON Tree)

- Navigation: `Up/Down` (oder `j/k`), `PageUp/PageDown`
- Folding: `Enter`/`Space` toggle, `Left/Right` fold/unfold
- Search: `/` (Enter to commit), Treffer: `n` / `N`
- Commands: `:` (Enter to run)
- Copy: `y` (pretty), `c` (compact), `p` (path)
- `Esc` schliesst

### Logs Dialog

- Scroll: `Up/Down`, `PageUp/PageDown`, `Home/End`
- Search: `/` (Enter to commit), Treffer: `n` / `N`
- Reload: `r`
- `Esc` schliesst
