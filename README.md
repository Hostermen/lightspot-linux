# lightspot-linux

[![License: GPL v3](https://img.shields.io/badge/License-GPL%20v3-blue.svg)](https://www.gnu.org/licenses/gpl-3.0)
[![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)](https://www.rust-lang.org/)
[![Electron](https://img.shields.io/badge/Electron-31-47848F.svg)](https://www.electronjs.org/)

A keyboard-driven launcher for Linux. Search applications and files, evaluate math expressions, and launch results instantly — summoned with a double-Shift.

lightspot-linux is built as a small, persistent Rust daemon paired with an Electron + React frontend. The daemon captures the hotkey at the kernel level via `evdev` and serves search results over a Unix socket; the Electron app renders the frosted-glass UI and forwards activations to `gtk-launch` / `xdg-open`.

## Features

- **Double-Shift hotkey** — kernel-level `evdev` capture works on both Wayland and X11 without interfering with normal typing. Super+Space is registered as a fallback.
- **Unified search** — fuzzy-matched applications (from `.desktop` entries), instant file lookup via the `plocate` index, full-text content search via a persistent [Tantivy](https://github.com/quickwit-oss/tantivy) index, plus an inline calculator. Results stream in after an 80 ms debounce.
- **Full-text content search** — the daemon builds and maintains a Tantivy full-text index of file *contents* under your home directory and watches for changes incrementally, so you can search by what's inside a file, not just its name. Each content hit shows a snippet of the matching text.
- **Native look** — a transparent, centered card with backdrop blur, rounded corners, and blue selection highlight, positioned ~12% above screen center.
- **Keyboard-first** — type to filter, arrow keys to navigate, Enter to activate, Esc to dismiss.
- **Lightweight backend** — file-name search reads a compressed `plocate` index in a single scan; content search queries a persistent mmap-backed Tantivy index updated by a background watcher thread.

## Requirements

- An X11 or Wayland desktop session
- `plocate` for file search (optional but recommended)
- `python3` + `python3-gi` (PyGObject) + GTK 3 for icon resolution
- `xdg-utils` (provides `xdg-open`) for opening files
- `gtk3` (provides `gtk-launch`) for launching applications
- Membership in the `input` group for the double-Shift hotkey (reads `/dev/input/event*`)

```bash
sudo apt install plocate python3-gi libgtk-3-0 xdg-utils
sudo updatedb.plocate
sudo usermod -aG input "$USER"   # then log out and back in
```

## Installation

### Debian package

Download the latest `.deb` from the [releases page](https://github.com/Hostermen/lightspot-linux/releases) (if available) and install it:

```bash
sudo apt install ./lightspot-linux_<version>_amd64.deb
```

The package installs the backend binary, the Electron frontend (bundled), desktop menu and autostart entries, and the start/stop scripts. After installing, log out and back in (for the `input` group), then start it:

```bash
lightspot-start
```

Press double-Shift to summon the launcher.

### Release tarball

```bash
tar xf lightspot-linux-<version>.tar.gz
cd lightspot-linux-<version>/
./scripts/install-user.sh
```

### Build from source (recommended)

```bash
git clone https://github.com/Hostermen/lightspot-linux.git
cd lightspot-linux

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
| Shift, Shift | Summon / dismiss the launcher |
| Super+Space | Fallback hotkey |
| Type | Filter apps, files, or calculate |
| Arrow Down / Tab | Move down in results |
| Arrow Up | Move up (back to the search field from the first result) |
| Enter | Open the selected result (or copy a calculator result) |
| Esc | Dismiss the launcher |

Start and stop the launcher manually:

```bash
lightspot-start    # launches the Electron UI + Rust daemon
lightspot-stop     # stops both
```

Logs are available via systemd user units:

```bash
journalctl --user -u lightspot-electron -f   # Electron UI
journalctl --user -u lightspot-daemon -f      # Rust hotkey daemon
```

Uninstall:

```bash
# Debian package
sudo apt remove lightspot-linux

# User-level install
./scripts/uninstall-user.sh
```

## How it works

The Rust daemon reads keyboard events directly from `/dev/input/event*` via the Linux `evdev` interface, so the hotkey works regardless of which application has focus. It detects two Shift presses within 280 ms (auto-repeat ignored) and writes `toggle` to `/tmp/lightspot-files.sock`, causing the Electron app to show or hide its window.

When the user types, the Electron renderer sends the query to the daemon over a Unix socket (`/tmp/lightspot-search.sock`). The daemon queries its in-memory app list (cached with a 30 s TTL), runs `plocate` (for queries of two or more characters), queries the warm Tantivy content index, evaluates the input as math when applicable, and returns a single JSON document. This avoids spawning a new process on every keystroke, keeping the app list and index warm in memory. The renderer displays the results and, on Enter, asks the main process to launch the app with `gtk-launch`, open the file with `xdg-open`, or copy a calculator result to the clipboard.

On startup the daemon spawns a background indexer thread that performs an initial full build of the Tantivy content index (under `$XDG_CACHE_HOME/lightspot-linux/index/`, or `~/.cache/...` by default) and then uses the `notify` crate to watch for filesystem changes, debouncing events and committing incrementally. The index scope defaults to `$HOME` and can be overridden with the `LIGHTSPOT_INDEX_DIRS` environment variable (colon-separated paths). Binary files (detected via NUL-byte sniffing), files over 2 MiB, hidden files, git-ignored entries, and common build/dependency directories (`node_modules`, `target`, `dist`, `__pycache__`, …) are skipped. A manual full reindex is available with `lightspot-files --index`.

## Verification

```bash
cargo test --release                       # 22 tests
cargo clippy --release -- -D warnings
cd electron && npm run build
```

## Project structure

```
.
├── Cargo.toml                      # Rust backend manifest
├── LICENSE                          # GPL-3.0
├── src/                            # Rust backend
│   ├── main.rs                     # Entry: daemon mode + --search/--index modes
│   ├── search.rs                   # Aggregates and serializes search results
│   ├── server.rs                   # Unix-socket search server (daemon)
│   ├── app_search.rs               # Parses .desktop entries
│   ├── file_search.rs              # plocate wrapper
│   ├── content_index.rs            # Tantivy full-text index + notify watcher
│   ├── calculator.rs               # Recursive-descent math evaluator
│   ├── keywatch.rs                 # evdev double-Shift detector
│   └── model.rs                    # Shared data types
├── electron/                       # Electron + React frontend
│   ├── electron/
│   │   ├── main.cjs                # Main process: window, IPC, socket, shortcuts
│   │   └── preload.cjs             # contextBridge API for the renderer
│   ├── src/
│   │   ├── main.jsx                # React entry
│   │   ├── App.jsx                 # Launcher UI component
│   │   └── App.css                 # Launcher styling
│   ├── package.json
│   ├── eslint.config.js            # Flat ESLint config
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

## Privacy

lightspot-linux is **fully local and offline**. It makes no network requests, collects no telemetry, and sends no data anywhere. All file indexing, search, and application launching happens on your machine.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
