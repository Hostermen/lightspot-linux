#!/usr/bin/env bash

# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Hostermen

# stop.sh — Stop Spotlight Files (Electron UI + Rust daemon).
set -euo pipefail

# Kill the systemd units (graceful), ignoring failures if they aren't running.
systemctl --user stop spotlight-electron spotlight-daemon 2>/dev/null || true
# Also kill stray processes by name (covers non-systemd launches).
pgrep -x spotlight-files | xargs -r kill 2>/dev/null || true
pgrep -x electron | xargs -r kill 2>/dev/null || true
# Remove the IPC socket so the next start is clean.
rm -f /tmp/spotlight-files.sock
echo "Spotlight Files stopped."
