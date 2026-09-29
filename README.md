# Spotlight Linux

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![Electron](https://img.shields.io/badge/Electron-31-47848F.svg)](https://www.electronjs.org/)

A Spotlight-style launcher for Linux. Search applications and files, evaluate math expressions, and launch results instantly — summoned with a double-Shift, just like macOS.

Spotlight Linux is built as a small, persistent Rust daemon paired with an Electron + React frontend. The daemon captures the hotkey at the kernel level via `evdev` and serves search results as JSON; the Electron app renders the frosted-glass UI and forwards activations to `gtk-launch` / `xdg-open`.

## Features

- **Double-Shift hotkey** — kernel-level `evdev` capture works on both Wayland and X11 without interfering with normal typing. Super+Space is registered as a fallback.
- **Unified search** — fuzzy-matched applications (from `.desktop` entries) and instant file lookup via the `plocate` index, plus an inline calculator. Results stream in after a 150 ms debounce.
- **Native look** — a transparent, centered card with backdrop blur, rounded corners, and blue selection highlight, positioned ~12% above screen center to match macOS Spotlight.
- **Keyboard-first** — type to filter, arrow keys to navigate, Enter to activate, Esc to dismiss.
- **Lightweight backend** — the Rust binary is under 600 KB. File search reads a compressed `plocate` index in a single scan, with no background indexer or daemon of its own.

## Requirements

- An X11 or Wayland desktop session
- `plocate` for file search (optional but recommended)
- Membership in the `input` group for the double-Shift hotkey (reads `/dev/input/event*`)

```bash
sudo apt install plocate
sudo updatedb.plocate
sudo usermod -aG input "$USER"   # then log out and back in
```

## Installation

### Debian package (recommended)

Download the latest `.deb` from the [releases page](https://github.com/Hostermen/spotlight-linux/releases) and install it:

```bash
sudo apt install ./spotlight-linux_<version>_amd64.deb
```

The package installs the backend binary, the Electron frontend (bundled), desktop menu and autostart entries, and the start/stop scripts. After installing, log out and back in (for the `input` group), then start it:

```bash
spotlight-start
```

Press double-Shift to summon Spotlight.

### Release tarball

```bash
tar xf spotlight-linux-<version>.tar.gz
cd spotlight-linux-<version>/
./scripts/install-user.sh
```

### Build from source

```bash
git clone https://github.com/Hostermen/spotlight-linux.git
cd spotlight-linux

# Backend
cargo build --release

# Frontend
cd electron && npm install && npm run build

# Install (user-level)
cd .. && ./scripts/install-user.sh
```

## Usage

| Key | Action |
|-----|--------|
| Shift, Shift | Summon / dismiss Spotlight |
| Super+Space | Fallback hotkey |
| Type | Filter apps, files, or calculate |
| Arrow Down / Tab | Move down in results |
| Arrow Up | Move up (back to the search field from the first result) |
| Enter | Open the selected result (or copy a calculator result) |
| Esc | Dismiss Spotlight |

Start and stop the launcher manually:

```bash
spotlight-start    # launches the Electron UI + Rust daemon
spotlight-stop     # stops both
```

Logs are available via systemd user units:

```bash
journalctl --user -u spotlight-electron -f   # Electron UI
journalctl --user -u spotlight-daemon -f      # Rust hotkey daemon
```

Uninstall:

```bash
# Debian package
sudo apt remove spotlight-linux

# User-level install
./scripts/uninstall-user.sh
```

## How it works

The Rust daemon reads keyboard events directly from `/dev/input/event*` via the Linux `evdev` interface, so the hotkey works regardless of which application has focus. It detects two Shift presses within 280 ms (auto-repeat ignored) and writes `toggle` to `/tmp/spotlight-files.sock`, causing the Electron app to show or hide its window.

When the user types, the Electron renderer sends the query to the backend over IPC. The backend spawns `spotlight-files --search <query>`, which loads `.desktop` entries, runs `plocate` (for queries of two or more characters), evaluates the input as math when applicable, and returns a single JSON document. The renderer displays the results and, on Enter, asks the main process to launch the app with `gtk-launch`, open the file with `xdg-open`, or copy a calculator result to the clipboard.

## Verification

```bash
cargo test --release                       # 18 tests
cargo clippy --release -- -D warnings
cd electron && npm run build
```

## Project structure

```
.
├── Cargo.toml                      # Rust backend manifest
├── src/                            # Rust backend
│   ├── main.rs                     # Entry: daemon mode + --search JSON mode
│   ├── search.rs                   # Aggregates and serializes search results
│   ├── app_search.rs               # Parses .desktop entries
│   ├── file_search.rs              # plocate wrapper
│   ├── calculator.rs               # Recursive-descent math evaluator
│   ├── keywatch.rs                 # evdev double-Shift detector
│   └── model.rs                     # Shared data types
├── electron/                       # Electron + React frontend
│   ├── electron/
│   │   ├── main.cjs                # Main process: window, IPC, socket, shortcuts
│   │   └── preload.cjs             # contextBridge API for the renderer
│   ├── src/
│   │   ├── main.jsx                # React entry
│   │   ├── App.jsx                 # Spotlight UI component
│   │   └── App.css                 # Spotlight styling
│   ├── package.json
│   ├── vite.config.js
│   └── index.html
└── scripts/
    ├── install-user.sh             # Build + install (user-level)
    ├── uninstall-user.sh           # Remove the user-level install
    ├── start.sh                    # Launch Electron UI + Rust daemon
    ├── stop.sh                     # Stop both
    ├── package.sh                  # Build the release tarball
    └── package-deb.sh              # Build the .deb package
```

## License

GPL-3.0.
