# Spotlight Linux

A lightweight **macOS Spotlight-style launcher for Linux**: instant **app search**, **file search** (via `plocate`), and a built-in **calculator**. Summons with **double-Shift** (just like macOS), appears **centered and translucent** on screen.

Built as two cooperating pieces:

- **Rust binary** (`spotlight-files`) — the search backend and the double-Shift hotkey daemon. Listens for the hotkey at the kernel level via `evdev` and signals the UI over a Unix socket.
- **Electron + React** app — the frosted-glass UI. Talks to the Rust backend over IPC for search results, and over `/tmp/spotlight-files.sock` for show/hide/toggle.

```
┌──────────────────────────────────────────────────────────────────┐
│  double-Shift  ──evdev──►  Rust daemon  ──"toggle"──►  Unix socket  │
│                                                                    │
│  Electron app ◄──socket──┘  shows/hides the window                │
│                                                                    │
│  User types ──IPC──►  Electron  ──`spotlight-files --search Q`──► │
│                       Rust (apps + plocate + calc) ──JSON──►       │
│  Electron renders results  ──Enter──►  gtk-launch / xdg-open     │
└──────────────────────────────────────────────────────────────────┘
```

## Features

- **Double-Shift hotkey** — press Shift twice within 280 ms to summon/dismiss. Uses `evdev` kernel-level input, so it works on both Wayland and X11 and doesn't interfere with normal typing. Auto-repeat is ignored.
- **macOS Spotlight look** — translucent dark card (78% opacity), rounded corners, magnifier icon, blue selection highlight. Centered ~12% above the vertical middle of the screen.
- **App search** — fuzzy-matches `.desktop` entries across all XDG application directories; launches via `gtk-launch`.
- **File search** — instant results from the `plocate` index (a compressed full-filename index, refreshed daily). `plocate` typically answers in <10 ms with near-zero idle cost.
- **Calculator** — inline math (`2+2`, `(1+2)*3`, `2^10`, `20/8`); Enter copies the result to the clipboard.
- **Keyboard-first** — type to filter, Arrow keys to navigate, Enter to open, Esc to dismiss.
- **Persistent window** — the Electron app stays running in the background; summoning is instant.
- **Click-outside / blur to dismiss**.
- **Fallback hotkey** — Super+Space is also registered by the Electron app.

## Requirements

- Rust ≥ 1.74 (edition 2021) — to build the search/hotkey backend
- Node.js ≥ 18 and npm — to build the Electron/React frontend
- `plocate` for file search (optional but recommended)
- Membership in the `input` group — the double-Shift hotkey reads `/dev/input/event*` via evdev
- A running X or Wayland session

Install on Ubuntu/Debian:

```bash
sudo apt install plocate
sudo updatedb.plocate          # build/refresh the filename index
sudo usermod -aG input "$USER"  # hotkey access — log out and back in after
```

## Build from source

```bash
# Backend (Rust)
cargo build --release

# Frontend (Electron + React)
cd electron
npm install
npm run build
```

## Install (user-level)

```bash
./scripts/install-user.sh
```

This:
- builds the Rust backend (`cargo build --release`)
- builds the React/Electron frontend (`npm run build` → `electron/dist/`)
- copies the binary to `~/.local/bin/spotlight-files`
- installs `spotlight-start` / `spotlight-stop` launcher scripts (the start script's electron path is patched in at install time, so it works from any project location)
- adds a GNOME application menu entry ("Spotlight Files")
- adds an autostart entry (launches on login)

After installing:
1. If you just added yourself to the `input` group, **log out and back in**.
2. Start it: `~/.local/bin/spotlight-start`
3. Press **double-Shift** to summon Spotlight.

Uninstall:

```bash
./scripts/uninstall-user.sh
```

## Usage / keyboard shortcuts

| Key | Action |
|-----|--------|
| Shift, Shift | Summon / dismiss Spotlight |
| Super+Space | Fallback hotkey (registered by the Electron app) |
| Type | Filter apps, files, or calculate |
| Arrow Down / Tab | Move down in results |
| Arrow Up | Move up (back to the search field from the first result) |
| Enter | Open the selected result (or copy a calculator result) |
| Esc | Dismiss Spotlight |
| Ctrl+Q | Quit the Electron background process |

Start / stop manually:

```bash
~/.local/bin/spotlight-start   # launches Electron UI + Rust daemon (as systemd user units)
~/.local/bin/spotlight-stop    # stops both
```

Logs:

```bash
journalctl --user -u spotlight-electron -f   # Electron UI
journalctl --user -u spotlight-daemon -f     # Rust hotkey daemon
```

## How the hotkey works

The Rust daemon reads keyboard events directly from `/dev/input/event*` via the Linux `evdev` interface. This is kernel-level, so it works on both Wayland and X11 regardless of which app has focus. It listens for two Shift presses (left or right) within 280 ms. Auto-repeat events (holding Shift) are ignored, so normal typing is unaffected. When it detects a double-Shift, it writes `toggle` to `/tmp/spotlight-files.sock`, and the Electron app shows/hides the window.

This requires membership in the `input` group. If you prefer not to use evdev, set `SPOTLIGHT_USE_EVDEV=0` and bind a GNOME custom shortcut to `spotlight-files` instead.

## How window centering works

The Electron app positions its window via `setBounds()` to the horizontal center and ~12% above the vertical center of the primary display, matching macOS Spotlight placement. On X11 this works directly. On GNOME/Wayland, application-requested window positions are honored for override-redirect/transparent windows like this one.

## How file search stays light

`plocate` stores a compressed `mlocate`-style index (built once daily by `updatedb.plocate`, usually via a system cron/timer). A query is a single read-only scan of that index — no filesystem walk, no running daemon, no per-keystroke I/O. The backend spawns `plocate` only after a 150 ms typing debounce and caps results at 100, so typing never hammers the CPU.

## Verification

```bash
cargo test --release        # 18 tests (calculator, fuzzy match, plocate, app parsing, double-tap logic)
cargo clippy --release -- -D warnings
cd electron && npm run build
```

## Release package

A self-contained release tarball can be built with:

```bash
./scripts/package.sh
# → dist/spotlight-linux-<version>.tar.gz
```

It bundles the prebuilt Rust backend, the built Electron frontend, the GNOME extension, and the install/uninstall scripts. To install from a release tarball:

```bash
tar xf spotlight-linux-*.tar.gz
cd spotlight-linux-*/
./scripts/install-user.sh
```

Prebuilt releases are published on the [GitHub Releases page](https://github.com/Hostermen/spotlight-linux/releases).

## Project structure

```
.
├── Cargo.toml                 # Rust backend manifest
├── Cargo.lock
├── src/                       # Rust backend source
│   ├── main.rs                # Entry: daemon mode, --search JSON mode, --gtk fallback
│   ├── search.rs              # Builds + serializes search results (apps + files + calc)
│   ├── app_search.rs          # Parses .desktop entries into app list
│   ├── file_search.rs         # plocate wrapper for file search
│   ├── calculator.rs          # Recursive-descent math evaluator
│   ├── keywatch.rs            # evdev double-Shift detector
│   └── model.rs              # Shared data types
├── electron/                  # Electron + React frontend
│   ├── package.json
│   ├── vite.config.js
│   ├── index.html
│   ├── electron/
│   │   ├── main.cjs           # Main process: window, IPC, Unix socket, shortcuts
│   │   └── preload.cjs        # contextBridge API exposed to the renderer
│   └── src/
│       ├── main.jsx           # React entry
│       ├── App.jsx            # Spotlight UI component
│       └── App.css            # Spotlight styling
└── scripts/
    ├── install-user.sh        # Build + install everything (user-level)
    ├── uninstall-user.sh      # Remove the user-level install
    ├── start.sh               # Launch Electron UI + Rust daemon (template; patched at install)
    ├── stop.sh                # Stop both
    └── package.sh             # Build the release tarball
```

## Limitations

- File search sees only paths present when `updatedb.plocate` last ran (usually <24 h old). Run `sudo updatedb.plocate` for a fresh index.
- Results are filename-based (not full-text contents), matching the "Everything" model on Windows.
- The Rust hotkey daemon checks ATR-like keyboard presence only; it is a convenience feature, not a security boundary.

## License

GPL-3.0.
