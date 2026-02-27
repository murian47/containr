# Konzept: Platzhalter in Kommandos (Keybindings / Commandline)

Stand: 2025-12-16
Status: depriorisiert (aktuell out of scope).

Dieses Dokument beschreibt ein Konzept, um Kommandos (Commandline und Keybindings) mit Platzhaltern zu versehen.
Ziel: Keybindings sollen kontextsensitiv sein, ohne dass der Nutzer fuer jeden Server/Container eigene fixe Strings pflegen muss.

## Motivation

Beispiel: Eine SSH-Shell auf den aktuellen Server oeffnen.

- Als Kommando: `:ssh mag@rpi5`
- Als Keybinding soll das generisch sein, z.B. `:ssh ${server.target}`

Aehnlich fuer Container-Konsole, Logs, Inspect, Bulk-Selection, etc.

## Grundidee

- Kommandos (aus der Commandline oder aus Keybindings via `:map`) sind Strings.
- Vor der Ausfuehrung werden Platzhalter im String expandiert.
- Platzhalter greifen auf einen Read-only "Context" zu (aktueller Server, aktive View, Selektion, Markierungen).
- Expansion ist deterministisch und ohne Seiteneffekte.

## Syntax

### Platzhalterform

- `${...}` fuer Platzhalterausdruecke
- `$` ohne `{}` wird nicht als Platzhalter interpretiert (vermeidet Konflikte mit Shell-Syntax)
- Escape: `\${` soll wortwoertlich bleiben

### Ausdruckssprache (minimal)

- `${server.name}`
- `${server.target}`
- `${server.runner}` -> `ssh` oder `local`
- `${view}` -> `containers|images|volumes|networks|logs|inspect|messages|help`

Selektion (kontextabhaengig):

- `${selection.kind}` -> `container|image|volume|network|none`
- `${selection.id}` / `${selection.name}` / `${selection.ref}` (je nach Kind)

Markierungen:

- `${marks.count}`
- `${marks.ids}` -> kommaseparierte Liste (oder JSON, siehe unten)
- `${marks.names}` -> fuer volumes
- `${marks.keys}` -> fuer images (ref:... oder id:...)

Compose/Stack:

- `${stack.name}` -> wenn in Tree/Stack-Kontext, sonst leer

Zeit/Meta:

- `${now.iso}` -> aktuelle Zeit (ISO-8601)
- `${app.server_label}` -> Anzeige-Label (z.B. wie im Header)

## Typen und Serialisierung

Platzhalter liefern Strings. Fuer Listen gibt es zwei Varianten:

- CSV: `${marks.ids}` -> `id1,id2,id3`
- JSON: `${marks.ids.json}` -> `["id1","id2","id3"]`

Vorschlag: Standard ist CSV, `.json` liefert JSON.

## Scopes / Fehlerverhalten

Scopes bestimmen, welche Platzhalter sinnvoll sind:

- Global: `server.*`, `view`, `now.*`
- Containers view: `selection.*` fuer Container, `marks.ids`, `stack.name`
- Images view: `selection.*` fuer Image, `marks.keys`
- Volumes view: `selection.*` fuer Volume, `marks.names`
- Networks view: `selection.*` fuer Network, `marks.ids`

Fehlerverhalten (konservativ):

- Unbekannter Platzhalter: Expansion bricht ab, es wird eine Message geloggt (Warn/Error), Kommando wird nicht ausgefuehrt.
- Platzhalter bekannt, aber Wert leer (z.B. keine Selektion): Kommando wird nicht ausgefuehrt, Message "missing selection" (Warn).

Optional spaeter:

- `${...:-fallback}` (fallback wenn leer)

## Ausfuehrungszeitpunkt

- Expansion passiert "late", direkt vor dem Ausfuehren.
- Damit funktionieren Keybindings auch nach Serverwechsel/Selektion/Markierung.

## Beispiele

### SSH Shell auf aktuellen Server

- Keybinding: `:map F8 :ssh ${server.target}`

### Container-Konsole (bash) auf aktuellem Container

- Keybinding: `:map ctrl-c :c bash` (intern nutzt es dann `${selection.id}`)
- Oder explizit: `:map ctrl-c :exec docker exec -it ${selection.id} bash`

### Bulk Action auf Markierungen

- `:stop ${marks.ids}` -> Tool expandiert IDs und fuehrt Stop fuer alle aus

### Messages View toggeln

- `:map ctrl-, :messages`

## Implementation Notes (praktisch)

- Parser: Scan String, finde Sequenzen `${...}` (mit Escape Handling).
- Resolver: Map von "keys" -> Fn(&App) -> Option<String>.
- Fuer `.json` Suffix: Resolver liefert Vec<String> und serialisiert via serde_json.
- Keine Shell-Expansion: Platzhalterexpansion ist rein intern; anschliessend wird das Kommando wie heute geparst/ausgefuehrt.

## Offene Punkte

- Einheitliche Identifier: `selection.id` soll bei Images evtl. `sha256:...` sein; bei Images ist `selection.ref` oft besser.
- Quote/Escaping: Wenn Werte Leerzeichen enthalten, braucht es definierte Quote-Regeln (z.B. immer shell-escapen fuer `:exec`).
- Mehrfachselektion pro View sauber modellieren (IDs vs Names vs Keys).
