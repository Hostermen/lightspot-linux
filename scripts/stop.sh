#!/usr/bin/env bash

# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Hostermen

# stop.sh — Stop lightspot-files (Electron UI + Rust daemon).
set -euo pipefail

# Kill the systemd units (graceful), ignoring failures if they aren't running.
systemctl --user stop lightspot-electron lightspot-daemon 2>/dev/null || true
# Also kill stray processes by name (covers non-systemd launches).
pgrep -x lightspot-files | xargs -r kill 2>/dev/null || true
pgrep -x electron | xargs -r kill 2>/dev/null || true
# Remove the IPC socket so the next start is clean.
rm -f /tmp/lightspot-files.sock
echo "lightspot-files stopped."
