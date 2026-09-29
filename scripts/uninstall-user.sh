#!/usr/bin/env bash
# Remove the user-level install of spotlight-files.
set -euo pipefail

BIN_NAME="spotlight-files"
INSTALL_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
AUTOSTART_DIR="$HOME/.config/autostart"

# Stop any running instances first (best-effort).
systemctl --user stop spotlight-electron spotlight-daemon 2>/dev/null || true
pgrep -x spotlight-files | xargs -r kill 2>/dev/null || true
pgrep -x electron | xargs -r kill 2>/dev/null || true
rm -f /tmp/spotlight-files.sock

# Remove installed files.
rm -f "$INSTALL_DIR/$BIN_NAME" \
      "$INSTALL_DIR/spotlight-start" \
      "$INSTALL_DIR/spotlight-stop" \
      "$DESKTOP_DIR/$BIN_NAME.desktop" \
      "$AUTOSTART_DIR/$BIN_NAME.desktop"

echo "Removed: $INSTALL_DIR/$BIN_NAME"
echo "Removed: $INSTALL_DIR/spotlight-start"
echo "Removed: $INSTALL_DIR/spotlight-stop"
echo "Removed: $DESKTOP_DIR/$BIN_NAME.desktop"
echo "Removed: $AUTOSTART_DIR/$BIN_NAME.desktop"
