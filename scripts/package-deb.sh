#!/usr/bin/env bash
# package-deb.sh — Build a self-contained .deb package for spotlight-linux.
#
# The package bundles:
#   - The prebuilt Rust backend binary (→ /usr/bin/spotlight-files)
#   - The Electron runtime + built frontend (→ /usr/lib/spotlight-linux/electron/)
#   - start/stop wrapper scripts (→ /usr/bin/spotlight-start, spotlight-stop)
#   - A desktop menu entry (→ /usr/share/applications/)
#   - An autostart entry (→ /etc/xdg/autostart/)
#
# Output: dist/spotlight-linux_<version>_amd64.deb
#
# Requires: cargo, npm, dpkg-deb. Run on the target architecture (e.g. amd64).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
DIST_DIR="$PROJECT_DIR/dist"

# Read the version from Cargo.toml.
VERSION="$(grep -m1 '^version' "$PROJECT_DIR/Cargo.toml" | sed -E 's/.*"([^"]+)".*/\1/')"
PKG_NAME="spotlight-linux"
ARCH="$(dpkg --print-architecture)"
DEB_NAME="${PKG_NAME}_${VERSION}_${ARCH}"
STAGE="$DIST_DIR/$DEB_NAME"

echo "=== Building Rust backend (release) ==="
cargo build --release --manifest-path "$PROJECT_DIR/Cargo.toml"

echo "=== Building Electron frontend ==="
cd "$PROJECT_DIR/electron"
npm install --silent 2>&1 | tail -3
npm run build 2>&1 | tail -3

echo "=== Staging .deb layout at $STAGE ==="
rm -rf "$STAGE"
mkdir -p "$STAGE/DEBIAN" \
         "$STAGE/usr/bin" \
         "$STAGE/usr/lib/$PKG_NAME/electron" \
         "$STAGE/usr/share/applications" \
         "$STAGE/etc/xdg/autostart"

# ── Rust backend ──────────────────────────────────────────────────────
cp -f "$PROJECT_DIR/target/release/spotlight-files" "$STAGE/usr/bin/spotlight-files"
chmod 0755 "$STAGE/usr/bin/spotlight-files"

# ── Electron app (runtime + built frontend) ────────────────────────────
# Bundle the Electron runtime so no npm/node is needed on the target.
cp -rf "$PROJECT_DIR/electron/node_modules/electron/dist/." \
      "$STAGE/usr/lib/$PKG_NAME/electron/runtime/"
# The Electron runtime needs to be at electron/runtime/, and main.cjs
# references it relative to its own location. We place the app code at
# electron/ and the runtime at electron/runtime/.
cp -f "$PROJECT_DIR/electron/electron/main.cjs"   "$STAGE/usr/lib/$PKG_NAME/electron/main.cjs"
cp -f "$PROJECT_DIR/electron/electron/preload.cjs" "$STAGE/usr/lib/$PKG_NAME/electron/preload.cjs"
cp -f "$PROJECT_DIR/electron/package.json"         "$STAGE/usr/lib/$PKG_NAME/electron/package.json"
cp -rf "$PROJECT_DIR/electron/dist/."              "$STAGE/usr/lib/$PKG_NAME/electron/dist/"

# Patch main.cjs to find the Electron binary at ./runtime/electron.
# The original looks for node_modules/electron/dist/electron; we rewrite
# that to a runtime-relative path.
sed -i 's#node_modules/electron/dist/electron#runtime/electron#' \
    "$STAGE/usr/lib/$PKG_NAME/electron/main.cjs"

# The chrome-sandbox helper needs SUID to function, but we run with
# --no-sandbox, so it's unused. Make sure the binary is executable.
chmod 0755 "$STAGE/usr/lib/$PKG_NAME/electron/runtime/electron" 2>/dev/null || true

# ── Start / stop wrapper scripts ───────────────────────────────────────
# These are generated here (not copied from scripts/) so the paths are
# absolute and architecture-independent.

cat > "$STAGE/usr/bin/spotlight-start" <<'EOF'
#!/usr/bin/env bash
# Launch Spotlight Linux (Electron UI + Rust daemon) as systemd user units.
set -euo pipefail

APP_DIR="/usr/lib/spotlight-linux/electron"
ELECTRON="$APP_DIR/runtime/electron"
BINARY="/usr/bin/spotlight-files"
SOCKET="/tmp/spotlight-files.sock"

systemctl --user stop spotlight-electron spotlight-daemon 2>/dev/null || true
systemctl --user reset-failed spotlight-electron spotlight-daemon 2>/dev/null || true
pgrep -x spotlight-files | xargs -r kill 2>/dev/null || true
pgrep -x electron | xargs -r kill 2>/dev/null || true
sleep 0.3
rm -f "$SOCKET"

export DISPLAY="${DISPLAY:-:0}"
systemd-run --user --unit=spotlight-electron \
    --working-directory="$APP_DIR" \
    --setenv=DISPLAY="${DISPLAY:-:0}" \
    --setenv=GDK_BACKEND=x11 \
    --setenv=CLUTTER_BACKEND=x11 \
    "$ELECTRON" --no-sandbox "$APP_DIR"

systemd-run --user --unit=spotlight-daemon "$BINARY"

sleep 1
echo "Spotlight Linux launched. Press double-Shift to toggle."
echo "Logs: journalctl --user -u spotlight-electron, journalctl --user -u spotlight-daemon"
echo "Stop: spotlight-stop"
EOF
chmod 0755 "$STAGE/usr/bin/spotlight-start"

cat > "$STAGE/usr/bin/spotlight-stop" <<'EOF'
#!/usr/bin/env bash
# Stop Spotlight Linux.
set -euo pipefail
systemctl --user stop spotlight-electron spotlight-daemon 2>/dev/null || true
pgrep -x spotlight-files | xargs -r kill 2>/dev/null || true
pgrep -x electron | xargs -r kill 2>/dev/null || true
rm -f /tmp/spotlight-files.sock
echo "Spotlight Linux stopped."
EOF
chmod 0755 "$STAGE/usr/bin/spotlight-stop"

# ── Desktop menu entry ─────────────────────────────────────────────────
cat > "$STAGE/usr/share/applications/spotlight-linux.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Spotlight Linux
GenericName=Application and File Launcher
Comment=Search apps and files instantly
Exec=spotlight-start
Icon=system-search
Terminal=false
Categories=Utility;System;FileTools;
Keywords=spotlight;search;launcher;files;apps;
StartupNotify=true
EOF

# ── Autostart entry ────────────────────────────────────────────────────
cat > "$STAGE/etc/xdg/autostart/spotlight-linux.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Spotlight Linux
Exec=spotlight-start
Icon=system-search
Terminal=false
X-GNOME-Autostart-enabled=true
EOF

# ── DEBIAN/control ─────────────────────────────────────────────────────
INSTALLED_SIZE="$(du -sk "$STAGE" | cut -f1)"
cat > "$STAGE/DEBIAN/control" <<EOF
Package: spotlight-linux
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Installed-Size: ${INSTALLED_SIZE}
Depends: plocate
Recommends: 
Maintainer: Hostermen <hostermen@users.noreply.github.com>
Description: Spotlight-style launcher for Linux
 Spotlight Linux provides instant application and file search, an inline
 calculator, and a double-Shift hotkey to summon a macOS-style translucent
 launcher window. Built with a Rust backend (evdev hotkey + plocate search)
 and an Electron + React frontend.
 .
 The hotkey reads /dev/input/event* via evdev and requires membership in the
 "input" group: sudo usermod -aG input \$USER
EOF
chmod 0644 "$STAGE/DEBIAN/control"

# ── DEBIAN/postinst ────────────────────────────────────────────────────
# Remind the user about the input group on first install.
cat > "$STAGE/DEBIAN/postinst" <<'EOF'
#!/usr/bin/env bash
set -e
if [ "$1" = "configure" ]; then
    if ! groups | grep -qw input 2>/dev/null; then
        echo ""
        echo "spotlight-linux: NOTE"
        echo "  The double-Shift hotkey needs read access to /dev/input/event*."
        echo "  Run:  sudo usermod -aG input \$USER"
        echo "  Then log out and back in for it to take effect."
        echo ""
    fi
fi
exit 0
EOF
chmod 0755 "$STAGE/DEBIAN/postinst"

# ── DEBIAN/prerm ───────────────────────────────────────────────────────
# Stop running instances before removal.
cat > "$STAGE/DEBIAN/prerm" <<'EOF'
#!/usr/bin/env bash
set -e
if [ "$1" = "remove" ] || [ "$1" = "upgrade" ]; then
    systemctl --user stop spotlight-electron spotlight-daemon 2>/dev/null || true
    pgrep -x spotlight-files | xargs -r kill 2>/dev/null || true
    pgrep -x electron | xargs -r kill 2>/dev/null || true
    rm -f /tmp/spotlight-files.sock
fi
exit 0
EOF
chmod 0755 "$STAGE/DEBIAN/prerm"

# ── Build the .deb ─────────────────────────────────────────────────────
echo "=== Building .deb ==="
mkdir -p "$DIST_DIR"
dpkg-deb --root-owner-group --build "$STAGE" "$DIST_DIR/${DEB_NAME}.deb"

DEB_PATH="$DIST_DIR/${DEB_NAME}.deb"
SIZE="$(du -h "$DEB_PATH" | cut -f1)"
echo ""
echo "=== .deb package ready ==="
echo "  $DEB_PATH  ($SIZE)"
echo ""
echo "Install with:"
echo "  sudo apt install ./$DEB_NAME.deb"
echo "  spotlight-start"
