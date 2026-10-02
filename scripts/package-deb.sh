#!/usr/bin/env bash

# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 Hostermen

# package-deb.sh — Build a self-contained .deb package for lightspot-linux.
#
# The package bundles:
#   - The prebuilt Rust backend binary (→ /usr/bin/lightspot-files)
#   - The Electron runtime + built frontend (→ /usr/lib/lightspot-linux/electron/)
#   - start/stop wrapper scripts (→ /usr/bin/lightspot-start, lightspot-stop)
#   - A desktop menu entry (→ /usr/share/applications/)
#   - An autostart entry (→ /etc/xdg/autostart/)
#
# Output: dist/lightspot-linux_<version>_amd64.deb
#
# Requires: cargo, npm, dpkg-deb. Run on the target architecture (e.g. amd64).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
DIST_DIR="$PROJECT_DIR/dist"

# Read the version from Cargo.toml.
VERSION="$(grep -m1 '^version' "$PROJECT_DIR/Cargo.toml" | sed -E 's/.*"([^"]+)".*/\1/')"
PKG_NAME="lightspot-linux"
ARCH="$(dpkg --print-architecture)"
DEB_NAME="${PKG_NAME}_${VERSION}_${ARCH}"
STAGE="$DIST_DIR/$DEB_NAME"

echo "=== Building Rust backend (release) ==="
# Scrub the build user's home path from the release binary so the public
# artifact doesn't leak /home/<user>. --remap-path-prefix rewrites the paths
# embedded in debug info and panic location strings (dependency source paths
# under $HOME/.cargo/registry/...). Without this, `strings` on the binary
# exposes the builder's username 200+ times.
# (Only $HOME is remapped: it contains no spaces, and the project dir — which
#  has a space and would break RUSTFLAGS word-splitting — does not leak into
#  the binary anyway.)
RUSTFLAGS="--remap-path-prefix=$HOME=/home/builder" \
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
cp -f "$PROJECT_DIR/target/release/lightspot-files" "$STAGE/usr/bin/lightspot-files"
chmod 0755 "$STAGE/usr/bin/lightspot-files"

# ── Electron app (runtime + built frontend) ────────────────────────────
# Bundle the Electron runtime so no npm/node is needed on the target.
cp -rf "$PROJECT_DIR/electron/node_modules/electron/dist/." \
      "$STAGE/usr/lib/$PKG_NAME/electron/runtime/"

# Strip the FDK AAC codec license paragraph from the bundled Chromium
# license file so the release artifact carries no third-party codec name
# that could implicitly identify the maintainer's employer. This removes
# required third-party license attribution at the maintainer's explicit
# request. Guarded with '|| true' so a future Electron version whose license
# file differs in wording doesn't break packaging.
sed -i '/FDK AAC and OpenSSL/,/compatible with the LGPL\./d' \
      "$STAGE/usr/lib/$PKG_NAME/electron/runtime/LICENSES.chromium.html" 2>/dev/null || true
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

cat > "$STAGE/usr/bin/lightspot-start" <<'EOF'
#!/usr/bin/env bash
# Launch lightspot-linux (Electron UI + Rust daemon) as systemd user units.
set -euo pipefail

APP_DIR="/usr/lib/lightspot-linux/electron"
ELECTRON="$APP_DIR/runtime/electron"
BINARY="/usr/bin/lightspot-files"
SOCKET="/tmp/lightspot-files.sock"

systemctl --user stop lightspot-electron lightspot-daemon 2>/dev/null || true
systemctl --user reset-failed lightspot-electron lightspot-daemon 2>/dev/null || true
pgrep -x lightspot-files | xargs -r kill 2>/dev/null || true
pgrep -x electron | xargs -r kill 2>/dev/null || true
sleep 0.3
rm -f "$SOCKET"

export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
SESSION_TYPE="${XDG_SESSION_TYPE:-x11}"
SYSTEMD_FLAGS=()
SYSTEMD_FLAGS+=("--setenv=XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR}")
if [ "$SESSION_TYPE" = "wayland" ] || [ -n "${WAYLAND_DISPLAY:-}" ]; then
    SYSTEMD_FLAGS+=("--setenv=WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-wayland-0}")
    SYSTEMD_FLAGS+=("--setenv=XDG_SESSION_TYPE=wayland")
    SYSTEMD_FLAGS+=("--setenv=GDK_BACKEND=x11")
    SYSTEMD_FLAGS+=("--setenv=CLUTTER_BACKEND=x11")
else
    SYSTEMD_FLAGS+=("--setenv=GDK_BACKEND=x11")
    SYSTEMD_FLAGS+=("--setenv=CLUTTER_BACKEND=x11")
fi
[ -n "${DISPLAY:-}" ] && SYSTEMD_FLAGS+=("--setenv=DISPLAY=${DISPLAY}")
systemd-run --user --unit=lightspot-electron \
    --property=Restart=on-failure \
    --property=RestartSec=2s \
    --working-directory="$APP_DIR" \
    "${SYSTEMD_FLAGS[@]}" \
    "$ELECTRON" --no-sandbox "$APP_DIR"

systemd-run --user --unit=lightspot-daemon \
    --property=Restart=on-failure \
    --property=RestartSec=2s \
    "$BINARY"

sleep 1
echo "lightspot-linux launched. Press double-Shift to toggle."
echo "Logs: journalctl --user -u lightspot-electron, journalctl --user -u lightspot-daemon"
echo "Stop: lightspot-stop"
EOF
chmod 0755 "$STAGE/usr/bin/lightspot-start"

cat > "$STAGE/usr/bin/lightspot-stop" <<'EOF'
#!/usr/bin/env bash
# Stop lightspot-linux.
set -euo pipefail
systemctl --user stop lightspot-electron lightspot-daemon 2>/dev/null || true
pgrep -x lightspot-files | xargs -r kill 2>/dev/null || true
pgrep -x electron | xargs -r kill 2>/dev/null || true
rm -f /tmp/lightspot-files.sock
echo "lightspot-linux stopped."
EOF
chmod 0755 "$STAGE/usr/bin/lightspot-stop"

# ── Desktop menu entry ─────────────────────────────────────────────────
cat > "$STAGE/usr/share/applications/lightspot-linux.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=lightspot-linux
GenericName=Application and File Launcher
Comment=Search apps and files instantly
Exec=lightspot-start
Icon=system-search
Terminal=false
Categories=Utility;System;FileTools;
Keywords=spotlight;search;launcher;files;apps;
StartupNotify=true
EOF

# ── Autostart entry ────────────────────────────────────────────────────
cat > "$STAGE/etc/xdg/autostart/lightspot-linux.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=lightspot-linux
Exec=lightspot-start
Icon=system-search
Terminal=false
X-GNOME-Autostart-enabled=true
EOF

# ── DEBIAN/control ─────────────────────────────────────────────────────
INSTALLED_SIZE="$(du -sk "$STAGE" | cut -f1)"
cat > "$STAGE/DEBIAN/control" <<EOF
Package: lightspot-linux
Version: ${VERSION}
Section: utils
Priority: optional
Architecture: ${ARCH}
Installed-Size: ${INSTALLED_SIZE}
Depends: plocate
Recommends:
Maintainer: Hostermen <lukas.kuemmerle@gmail.com>
Description: keyboard-driven launcher for Linux
 lightspot-linux provides instant application and file search, an inline
  calculator, and a double-Shift hotkey to summon a translucent
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
        echo "lightspot-linux: NOTE"
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
    systemctl --user stop lightspot-electron lightspot-daemon 2>/dev/null || true
    pgrep -x lightspot-files | xargs -r kill 2>/dev/null || true
    pgrep -x electron | xargs -r kill 2>/dev/null || true
    rm -f /tmp/lightspot-files.sock
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
echo "  lightspot-start"
