#!/usr/bin/env bash

# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Hostermen

# start.sh — Launch the lightspot-files Electron UI + Rust daemon.
#
# This file is a TEMPLATE: the `__ELECTRON_DIR__` placeholder is replaced
# with the absolute path to the electron/ directory at install time by
# scripts/install-user.sh (so the installed copy works regardless of where
# the project lives).
set -euo pipefail

ELECTRON_DIR="__ELECTRON_DIR__"                       # patched at install
BINARY="$HOME/.local/bin/lightspot-files"             # the Rust backend
ELECTRON="$ELECTRON_DIR/node_modules/electron/dist/electron"  # electron binary
SOCKET="/tmp/lightspot-files.sock"                    # IPC socket between them

# Kill existing instances.
# Guard every command with `|| true` so `set -e` doesn't abort the script
# when a unit/process doesn't exist yet (common on first run).
systemctl --user stop lightspot-electron lightspot-daemon 2>/dev/null || true
# Clear any leftover failed state so systemd-run can reuse the unit names.
systemctl --user reset-failed lightspot-electron lightspot-daemon 2>/dev/null || true
pgrep -x lightspot-files | xargs -r kill 2>/dev/null || true
pgrep -x electron | xargs -r kill 2>/dev/null || true
sleep 0.3

# Clean stale socket from a previous crash.
rm -f "$SOCKET"

# Start Electron (UI) as a detached systemd user unit.
# Electron runs under XWayland by default, where win.setBounds({x,y})
# positioning works. On native Wayland the compositor ignores client
# positioning, so we don't pass --ozone-platform=wayland.
SESSION_TYPE="${XDG_SESSION_TYPE:-x11}"
SYSTEMD_FLAGS=()
SYSTEMD_FLAGS+=("--setenv=DISPLAY=${DISPLAY:-:0}")
if [ "$SESSION_TYPE" = "wayland" ]; then
    SYSTEMD_FLAGS+=("--setenv=WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-wayland-0}")
    SYSTEMD_FLAGS+=("--setenv=XDG_SESSION_TYPE=wayland")
    SYSTEMD_FLAGS+=("--setenv=GDK_BACKEND=x11")
    SYSTEMD_FLAGS+=("--setenv=CLUTTER_BACKEND=x11")
else
    SYSTEMD_FLAGS+=("--setenv=GDK_BACKEND=x11")
    SYSTEMD_FLAGS+=("--setenv=CLUTTER_BACKEND=x11")
fi
systemd-run --user --unit=lightspot-electron \
    --property=Restart=on-failure \
    --property=RestartSec=2s \
    --working-directory="$ELECTRON_DIR" \
    "${SYSTEMD_FLAGS[@]}" \
    "$ELECTRON" --no-sandbox "$ELECTRON_DIR"

# Start the Rust daemon (double-Shift hotkey listener).
systemd-run --user --unit=lightspot-daemon \
    --property=Restart=on-failure \
    --property=RestartSec=2s \
    "$BINARY"

sleep 1
echo "lightspot-files launched."
echo "Press double-Shift to toggle the launcher."
echo "Logs: journalctl --user -u lightspot-electron, journalctl --user -u lightspot-daemon"
echo "Stop: ~/.local/bin/lightspot-stop"
