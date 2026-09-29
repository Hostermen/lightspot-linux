# Spotlight Files

A lightweight Spotlight-style launcher for Linux: instant **app search**, **file search**, and a built-in **calculator**. Built with Rust + GTK 4. No background daemon, no telemetry, no heavy indexer of its own — file search is powered by `plocate`, the fastest, lowest-resource filename index on Linux.

Summons with **double-Shift** (just like macOS Spotlight), appears **centered and translucent** on screen.

This is a fresh, self-contained reimplementation inspired by [SHADOWOKX/Spotlight-linux](https://github.com/SHADOWOKX/Spotlight-linux), extended with Everything-like file search.

## Features

- **Double-Shift hotkey** — press Shift twice within 280ms to summon/dismiss, just like macOS. Uses `evdev` kernel-level input (works on both Wayland and X11, doesn't interfere with normal typing).
- **macOS Spotlight look** — translucent dark card (78% opacity), rounded corners, magnifier icon, blue selection highlight, modern typography. Centered on screen.
- **App search** — fuzzy-matches `.desktop` entries across XDG dirs; launches via `gtk-launch`.
- **File search** — instant results from the `plocate` index (a full-filename index, updated daily by cron). `plocate` typically answers in <10 ms with near-zero idle cost — far lighter than Tracker/Baloo.
- **Calculator** — inline math (`2+2`, `(1+2)*3`, `2^10`, `20/8`); Enter copies the result.
- **Keyboard-first** — type to filter, Arrow keys to navigate, Enter to open, Esc to dismiss.
- **Persistent window** — stays running in the background (613 KB binary), shows/hides on demand. No startup delay on summon.
- **Click-outside to dismiss** — loses focus, hides automatically.

## Requirements

- Rust ≥ 1.74 (edition 2021)
- GTK 4 development files (`libgtk-4-dev`)
- `plocate` for file search (optional but recommended)
- `input` group membership for the double-Shift hotkey (evdev needs read access to `/dev/input/event*`)
- GNOME Shell 45+ for the centering extension

Install on Ubuntu/Debian:

```bash
sudo apt install libgtk-4-dev plocate
sudo updatedb.plocate      # build/refresh the filename index
sudo usermod -aG input $USER  # hotkey access — log out and back in after
```

## Build from source

```bash
cargo build --release
./target/release/spotlight-files
```

## Install (user-level)

```bash
./scripts/install-user.sh
```

This installs:
- Binary to `~/.local/bin/spotlight-files`
- App menu entry (appears in GNOME Overview as "Spotlight Files")
- Autostart entry (launches on login so the hotkey is always available)
- GNOME Shell extension for window centering

After installing:
1. **Restart GNOME Shell** to load the centering extension:
   - Wayland: log out and back in
   - X11: Alt+F2, type `r`, press Enter
2. If you just added yourself to the `input` group, **log out and back in** for it to take effect.
3. Press **double-Shift** to summon Spotlight.

Uninstall:

```bash
./scripts/uninstall-user.sh
```

## How the hotkey works

The launcher reads keyboard events directly from `/dev/input/event*` via the Linux `evdev` interface. This is kernel-level, so it works on both Wayland and X11 regardless of which app has focus. It listens for two Shift presses (left or right) within 280ms. Auto-repeat events (holding Shift) are ignored, so normal typing is unaffected.

This requires membership in the `input` group. If you prefer not to use evdev, set `SPOTLIGHT_USE_EVDEV=0` and bind a GNOME custom shortcut to `spotlight-files` instead.

## How window centering works

GNOME/Mutter on Wayland doesn't allow applications to position their own windows, and doesn't support the `wlr-layer-shell` protocol. A small GNOME Shell extension (`gnome-extension/extension.js`) watches for windows titled "Spotlight" and calls `move_frame()` to center them each time they appear. The window sits about 12% above the vertical center, matching macOS Spotlight placement.

## How file search stays light

`plocate` stores a compressed `mlocate`-style index (built once daily by `updatedb.plocate`, usually via a system cron/timer). A query is a single read-only scan of that index — no filesystem walk, no running daemon, no per-keystroke I/O. The launcher itself spawns `plocate` only after a 150 ms typing debounce and caps results at 100, so typing never hammers the CPU.

## Verification

```bash
cargo test --release        # 14 tests (calculator, fuzzy match, plocate, app parsing, double-tap logic)
cargo clippy --release -- -D warnings
```

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| Shift, Shift | Summon / dismiss Spotlight |
| Type | Filter apps, files, or calculate |
| Arrow Down / Tab | Move to results |
| Arrow Up | Back to search field (from first result) |
| Enter | Open selected result (or copy calculator result) |
| Esc | Dismiss Spotlight |
| Ctrl+Q | Quit the background process |

## Limitations

- File search sees only paths present when `updatedb.plocate` last ran (usually <24 h old). Run `sudo updatedb.plocate` for a fresh index.
- Results are filename-based (not full-text contents), matching the "Everything" model on Windows.
- The centering extension requires GNOME Shell. Other desktop environments (KDE, sway, etc.) may need different approaches.

## License

GPL-3.0.
