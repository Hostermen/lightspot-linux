#!/usr/bin/env bash
# Launches the Spotlight Files Electron app + Rust daemon (hotkey listener).
set -euo pipefail

ELECTRON_DIR="__ELECTRON_DIR__"
BINARY="$HOME/.local/bin/spotlight-files"
ELECTRON="$ELECTRON_DIR/node_modules/electron/dist/electron"
SOCKET="/tmp/spotlight-files.sock"

# Kill existing instances (ignore failures — units may not exist yet)
systemctl --user stop spotlight-electron spotlight-daemon 2>/dev/null || true
systemctl --user reset-failed spotlight-electron spotlight-daemon 2>/dev/null || true
pgrep -x spotlight-files | xargs -r kill 2>/dev/null || true
pgrep -x electron | xargs -r kill 2>/dev/null || true
sleep 0.3

# Clean stale socket
rm -f "$SOCKET"

# Start Electron (UI) via systemd (fully detached)
export DISPLAY="${DISPLAY:-:0}"
systemd-run --user --unit=spotlight-electron \
    --working-directory="$ELECTRON_DIR" \
    --setenv=DISPLAY="${DISPLAY:-:0}" \
    --setenv=GDK_BACKEND=x11 \
    --setenv=CLUTTER_BACKEND=x11 \
    "$ELECTRON" --no-sandbox "$ELECTRON_DIR"

# Start Rust daemon (double-Shift hotkey)
systemd-run --user --unit=spotlight-daemon \
    "$BINARY"

sleep 1
echo "Spotlight Files launched."
echo "Press double-Shift to toggle Spotlight."
echo "Logs: journalctl --user -u spotlight-electron, journalctl --user -u spotlight-daemon"
echo "Stop: ~/.local/bin/spotlight-stop"
