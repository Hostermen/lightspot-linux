#!/usr/bin/env bash
# Remove the user-level install of spotlight-files.
set -euo pipefail

BIN_NAME="spotlight-files"
INSTALL_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
EXTENSION_UUID="spotlight-center@spotlight-files"
EXTENSION_DIR="$HOME/.local/share/gnome-shell/extensions/$EXTENSION_UUID"
AUTOSTART_DIR="$HOME/.config/autostart"

if command -v gnome-extensions &>/dev/null; then
    gnome-extensions disable "$EXTENSION_UUID" 2>/dev/null || true
fi

rm -rf "$EXTENSION_DIR"
rm -f "$INSTALL_DIR/$BIN_NAME" \
      "$DESKTOP_DIR/$BIN_NAME.desktop" \
      "$AUTOSTART_DIR/$BIN_NAME.desktop"

echo "Removed: $INSTALL_DIR/$BIN_NAME"
echo "Removed: $DESKTOP_DIR/$BIN_NAME.desktop"
echo "Removed: $AUTOSTART_DIR/$BIN_NAME.desktop"
echo "Removed: $EXTENSION_DIR"
