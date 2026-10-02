#!/usr/bin/env bash

# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Hostermen

# Remove the user-level install of lightspot-files.
set -euo pipefail

BIN_NAME="lightspot-files"
INSTALL_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
AUTOSTART_DIR="$HOME/.config/autostart"

# Stop any running instances first (best-effort).
systemctl --user stop lightspot-electron lightspot-daemon 2>/dev/null || true
pgrep -x lightspot-files | xargs -r kill 2>/dev/null || true
pgrep -x electron | xargs -r kill 2>/dev/null || true
rm -f /tmp/lightspot-files.sock

# Remove installed files.
rm -f "$INSTALL_DIR/$BIN_NAME" \
      "$INSTALL_DIR/lightspot-start" \
      "$INSTALL_DIR/lightspot-stop" \
      "$DESKTOP_DIR/$BIN_NAME.desktop" \
      "$AUTOSTART_DIR/$BIN_NAME.desktop"

echo "Removed: $INSTALL_DIR/$BIN_NAME"
echo "Removed: $INSTALL_DIR/lightspot-start"
echo "Removed: $INSTALL_DIR/lightspot-stop"
echo "Removed: $DESKTOP_DIR/$BIN_NAME.desktop"
echo "Removed: $AUTOSTART_DIR/$BIN_NAME.desktop"
