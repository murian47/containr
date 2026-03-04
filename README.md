# containr

`containr` is a terminal UI for managing Docker and Podman workloads locally or on remote hosts via `ssh`.

The current `0.5.0` target platforms are:

- Linux: primary platform for local and remote Docker/Podman workflows
- macOS: supported development and client platform, including local Docker setups

Not part of the `0.5.0` support matrix:

- `*BSD`
- Windows

Other platforms may work on a best-effort basis, but are not currently validated.

## Highlights

- Single-shell TUI with sidebar, dashboard, lists, details, overlays, and docked messages
- Remote execution through system `ssh`
- Local Docker/Podman support through `target = "local"`
- Containers, stacks, images, volumes, networks, templates, registries, themes, and messages
- Template-based deploy/redeploy workflows with Git integration
- Inspect and logs views with search, scrolling, and clipboard copy
- Theme selector with preview plus built-in and user override themes

## Requirements

- Local Linux/macOS machine with `ssh` in `PATH`
- Remote host with Docker or Podman installed
- SSH user must use a passwordless SSH key or agent-based key login
- SSH user must be able to run `docker` / `podman` commands used by containr
- Optional:
  - `git` for template versioning
  - configured editor via `editor_cmd` or `$EDITOR`

## Quick start

Run directly against a target:

```bash
cargo run -- --target user@server
```

Run against a named server from the config:

```bash
cargo run -- --server rpi5
```

Useful options:

```bash
cargo run -- --target user@server --refresh-secs 2
cargo run -- --target user@server --docker-cmd "sudo docker"
cargo run -- --target user@server --identity ~/.ssh/id_ed25519 --port 2222
cargo run -- --target user@server --mouse
cargo run -- --target user@server --ascii-only
```

Release build:

```bash
cargo build --release
```

## Installation

### macOS

System-wide install:

```bash
./packaging/macos/install.sh
```

User-local install without `sudo`:

```bash
./packaging/macos/install.sh --prefix "$HOME/.local"
export PATH="$HOME/.local/bin:$PATH"
```

Default paths:

- binary payload: `/usr/local/libexec/containr/containr`
- wrapper: `/usr/local/bin/containr`
- themes: `/usr/local/share/containr/themes`

Uninstall:

```bash
./packaging/macos/uninstall.sh
./packaging/macos/uninstall.sh --prefix "$HOME/.local"
./packaging/macos/uninstall.sh --keep-themes
```

### Linux

System-wide install:

```bash
./packaging/linux/install.sh
```

User-local install without `sudo`:

```bash
./packaging/linux/install.sh --prefix "$HOME/.local"
export PATH="$HOME/.local/bin:$PATH"
```

Default paths:

- binary: `/usr/local/bin/containr`
- themes: `/usr/local/share/containr/themes`

Uninstall:

```bash
./packaging/linux/uninstall.sh
./packaging/linux/uninstall.sh --prefix "$HOME/.local"
./packaging/linux/uninstall.sh --keep-themes
```

## Configuration

Config path:

- `$XDG_CONFIG_HOME/containr/config.json`
- fallback: `$HOME/.config/containr/config.json`

Minimal example:

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

No passwords or registry secrets are stored in the server config.

Local Docker/Podman is configured as:

```json
{
  "name": "local",
  "target": "local",
  "docker_cmd": "docker"
}
```

## Themes

Bundled themes are based on [Ghostty](https://ghostty.org) themes and adapted for containr.

Theme lookup order:

1. user overrides in the config directory
2. bundled `themes/` near the workspace / installation
3. system themes in `/usr/local/share/containr/themes` and `/usr/share/containr/themes`

Useful commands:

- `:theme list`
- `:theme use <name>`
- `:theme edit [name]`
- `:theme new <name>`
- `:theme rm <name>`

## Common commands

Help and messages:

- `F1` or `:help`
- `:messages`
- `:messages save <file>`
- `:log dock [3..12]`

Servers:

- `:server add <name> ssh <target> [-p <port>] [-i <identity>] [--cmd <docker|podman>]`
- `:server add <name> local [--cmd <docker|podman>]`
- `:server select <name>`

Templates:

- `:template add <name>`
- `:template edit [name]`
- `:template deploy [--pull] [--recreate] [name]`
- `:template rm [name]`
- `:templates toggle`

Git in file-backed workspaces:

- `:git templates status`
- `:git templates diff`
- `:git templates log`
- `:git templates commit -m "..."`
- `:git themes status`
- `:git themes diff`
- `:git themes log`
- `:git themes commit -m "..."`

## Key defaults

The exact keymap is configurable. The important defaults are:

- `F1`: help
- `Tab` / `Shift-Tab`: cycle focus
- `:`: command line
- `^b`: toggle sidebar
- `^g`: open messages
- `^p`: toggle split layout where supported
- `^u`: stack update
- `^U`: stack update `--all`
- `^y`: template deploy
- `^Y`: template deploy with recreate and pull

For the full current command and keybinding reference, use the built-in help view and `:map list`.

## Documentation

- release checklist: `docs/testing-checklist.md`
- roadmap: `docs/roadmap-priorities.md`
- release prep: `docs/release-prep.md`
- AI agents in project work: `docs/ai-agents.md`
- user guide (DE): `docs/user_guide_de.md`
- code map: `docs/code-map-ui.md`
