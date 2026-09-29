#!/usr/bin/env bash
# Build and install spotlight-files (Electron UI + Rust search backend).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
ELECTRON_DIR="$PROJECT_DIR/electron"
BIN_NAME="spotlight-files"
INSTALL_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
AUTOSTART_DIR="$HOME/.config/autostart"

echo "=== Building Rust search backend ==="
cargo build --release --manifest-path "$PROJECT_DIR/Cargo.toml"

echo "=== Building React/Electron frontend ==="
cd "$ELECTRON_DIR"
npm install --silent 2>&1 | tail -3
npm run build 2>&1 | tail -3

# --- Binary ---
mkdir -p "$INSTALL_DIR"
cp -f "$PROJECT_DIR/target/release/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
chmod 0755 "$INSTALL_DIR/$BIN_NAME"

# --- Start script (launches both Electron + daemon) ---
cp -f "$SCRIPT_DIR/start.sh" "$INSTALL_DIR/spotlight-start"
sed -i "s#__ELECTRON_DIR__#$ELECTRON_DIR#" "$INSTALL_DIR/spotlight-start"
chmod 0755 "$INSTALL_DIR/spotlight-start"

# --- Stop script ---
cp -f "$SCRIPT_DIR/stop.sh" "$INSTALL_DIR/spotlight-stop"
chmod 0755 "$INSTALL_DIR/spotlight-stop"

# --- Desktop entry ---
mkdir -p "$DESKTOP_DIR"
cat > "$DESKTOP_DIR/$BIN_NAME.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Spotlight Files
GenericName=Application and File Launcher
Comment=Search apps and files instantly
Exec=$INSTALL_DIR/spotlight-start
Icon=system-search
Terminal=false
Categories=Utility;System;FileTools;
Keywords=spotlight;search;launcher;files;apps;
StartupNotify=true
EOF
chmod 0600 "$DESKTOP_DIR/$BIN_NAME.desktop"

# --- Autostart ---
mkdir -p "$AUTOSTART_DIR"
cat > "$AUTOSTART_DIR/$BIN_NAME.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Spotlight Files
Exec=$INSTALL_DIR/spotlight-start
Icon=system-search
Terminal=false
X-GNOME-Autostart-enabled=true
EOF

# --- Input group ---
if ! groups | grep -qw input; then
    echo ""
    echo "NOTICE: You are not in the 'input' group."
    echo "  The double-Shift hotkey needs read access to /dev/input/event*."
    echo "  Run this, then log out and back in:"
    echo "    sudo usermod -aG input \$USER"
else
    echo "Input group: OK"
fi

echo ""
echo "=== Installation complete ==="
echo "Binary:      $INSTALL_DIR/$BIN_NAME"
echo "Launcher:    $INSTALL_DIR/spotlight-start"
echo "Stopper:     $INSTALL_DIR/spotlight-stop"
echo "App menu:    $DESKTOP_DIR/$BIN_NAME.desktop"
echo "Autostart:   $AUTOSTART_DIR/$BIN_NAME.desktop"
echo ""
echo "Start now:   $INSTALL_DIR/spotlight-start"
echo "Stop:        $INSTALL_DIR/spotlight-stop"
echo ""
echo "Then press double-Shift to toggle Spotlight."
echo ""
echo "Note: file search needs 'plocate'. If missing:"
echo "  sudo apt install plocate && sudo updatedb.plocate"
