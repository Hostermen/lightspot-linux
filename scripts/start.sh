#!/usr/bin/env bash

# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Hostermen

# start.sh — Launch the Spotlight Files Electron UI + Rust daemon.
#
# This file is a TEMPLATE: the `__ELECTRON_DIR__` placeholder is replaced
# with the absolute path to the electron/ directory at install time by
# scripts/install-user.sh (so the installed copy works regardless of where
# the project lives).
set -euo pipefail

ELECTRON_DIR="__ELECTRON_DIR__"                       # patched at install
BINARY="$HOME/.local/bin/spotlight-files"             # the Rust backend
ELECTRON="$ELECTRON_DIR/node_modules/electron/dist/electron"  # electron binary
SOCKET="/tmp/spotlight-files.sock"                    # IPC socket between them

# Kill existing instances.
# Guard every command with `|| true` so `set -e` doesn't abort the script
# when a unit/process doesn't exist yet (common on first run).
systemctl --user stop spotlight-electron spotlight-daemon 2>/dev/null || true
# Clear any leftover failed state so systemd-run can reuse the unit names.
systemctl --user reset-failed spotlight-electron spotlight-daemon 2>/dev/null || true
pgrep -x spotlight-files | xargs -r kill 2>/dev/null || true
pgrep -x electron | xargs -r kill 2>/dev/null || true
sleep 0.3

# Clean stale socket from a previous crash.
rm -f "$SOCKET"

# Start Electron (UI) as a detached systemd user unit.
# Detect display session: on Wayland, let Electron use ozone-platform=wayland
# (set in main.cjs) natively — forcing GDK_BACKEND=x11 causes XWayland connection
# drops that crash Electron every ~30 min. On X11, set the X11 backends.
SESSION_TYPE="${XDG_SESSION_TYPE:-x11}"
SYSTEMD_FLAGS=()
SYSTEMD_FLAGS+=("--setenv=DISPLAY=${DISPLAY:-:0}")
if [ "$SESSION_TYPE" = "wayland" ]; then
    SYSTEMD_FLAGS+=("--setenv=WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-wayland-0}")
    SYSTEMD_FLAGS+=("--setenv=XDG_SESSION_TYPE=wayland")
else
    SYSTEMD_FLAGS+=("--setenv=GDK_BACKEND=x11")
    SYSTEMD_FLAGS+=("--setenv=CLUTTER_BACKEND=x11")
fi
systemd-run --user --unit=spotlight-electron \
    --working-directory="$ELECTRON_DIR" \
    "${SYSTEMD_FLAGS[@]}" \
    "$ELECTRON" --no-sandbox "$ELECTRON_DIR"

# Start the Rust daemon (double-Shift hotkey listener).
systemd-run --user --unit=spotlight-daemon \
    "$BINARY"

sleep 1
echo "Spotlight Files launched."
echo "Press double-Shift to toggle Spotlight."
echo "Logs: journalctl --user -u spotlight-electron, journalctl --user -u spotlight-daemon"
echo "Stop: ~/.local/bin/spotlight-stop"
