#!/usr/bin/env bash
# package.sh — Build a self-contained release tarball for spotlight-files.
#
# The tarball bundles:
#   - The prebuilt Rust backend binary (target/release/spotlight-files)
#   - The built Electron frontend (electron/dist/)
#   - The Electron main/preload scripts and package.json (so electron can run)
#   - The install/uninstall/start/stop scripts
#   - This README
#
# Output: dist/spotlight-linux-<version>.tar.gz
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
DIST_DIR="$PROJECT_DIR/dist"

# Read the version from Cargo.toml ([package] version = "...").
VERSION="$(grep -m1 '^version' "$PROJECT_DIR/Cargo.toml" | sed -E 's/.*"([^"]+)".*/\1/')"
PKG_NAME="spotlight-linux-$VERSION"
STAGE="$DIST_DIR/$PKG_NAME"

echo "=== Building Rust backend (release) ==="
cargo build --release --manifest-path "$PROJECT_DIR/Cargo.toml"

echo "=== Building Electron frontend ==="
cd "$PROJECT_DIR/electron"
npm install --silent 2>&1 | tail -3
npm run build 2>&1 | tail -3

echo "=== Staging release into $STAGE ==="
rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/electron/dist" "$STAGE/scripts"

# Rust backend binary.
cp -f "$PROJECT_DIR/target/release/spotlight-files" "$STAGE/bin/"

# Electron app: main/preload, built frontend, package.json, index.html.
cp -f "$PROJECT_DIR/electron/electron/main.cjs"  "$STAGE/electron/main.cjs"
cp -f "$PROJECT_DIR/electron/electron/preload.cjs" "$STAGE/electron/preload.cjs"
cp -f "$PROJECT_DIR/electron/package.json"       "$STAGE/electron/package.json"
cp -rf "$PROJECT_DIR/electron/dist/."            "$STAGE/electron/dist/"

# Scripts (install path uses the project-relative electron/ dir).
cp -f "$SCRIPT_DIR/install-user.sh"   "$STAGE/scripts/"
cp -f "$SCRIPT_DIR/uninstall-user.sh" "$STAGE/scripts/"
cp -f "$SCRIPT_DIR/start.sh"          "$STAGE/scripts/"
cp -f "$SCRIPT_DIR/stop.sh"            "$STAGE/scripts/"

# Docs.
cp -f "$PROJECT_DIR/README.md" "$STAGE/"

# Patch the staged start.sh so it points at the bundled electron/ dir.
# (The bundled layout has the electron app at ./electron relative to the stage.)
sed -i "s#__ELECTRON_DIR__#$(pwd)/electron#" "$STAGE/scripts/start.sh"

# Patch install-user.sh to reference the bundled binary path instead of
# rebuilding from source (release tarballs ship prebuilt binaries).
# We rewrite the binary-copy step to use ./bin/spotlight-files.
python3 - "$STAGE/scripts/install-user.sh" <<'PY'
import sys, re
p = sys.argv[1]
s = open(p).read()
# Replace the cargo build step with a notice that binaries are prebuilt.
s = s.replace(
    'echo "=== Building Rust search backend ==="\n'
    '# Build the optimized backend binary.\n'
    'cargo build --release --manifest-path "$PROJECT_DIR/Cargo.toml"',
    'echo "=== Using prebuilt Rust backend ==="\n'
    '# Release tarballs ship a prebuilt binary — no cargo build needed.'
)
# Copy the prebuilt binary from ./bin/ instead of target/release/.
s = s.replace(
    'cp -f "$PROJECT_DIR/target/release/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"',
    'cp -f "$PROJECT_DIR/bin/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"'
)
# Replace the npm build steps with a notice that the frontend is prebuilt.
s = s.replace(
    'echo "=== Building React/Electron frontend ==="\n'
    'cd "$ELECTRON_DIR"\n'
    '# Install npm dependencies (quietly) then build the production bundle.\n'
    'npm install --silent 2>&1 | tail -3\n'
    'npm run build 2>&1 | tail -3',
    'echo "=== Using prebuilt Electron frontend ==="\n'
    '# Release tarballs ship the built frontend in electron/dist/.'
)
open(p, 'w').write(s)
PY

echo "=== Creating tarball ==="
mkdir -p "$DIST_DIR"
TARBALL="$DIST_DIR/$PKG_NAME.tar.gz"
tar -C "$DIST_DIR" -czf "$TARBALL" "$PKG_NAME"

# Show the resulting tarball size.
SIZE="$(du -h "$TARBALL" | cut -f1)"
echo ""
echo "=== Release package ready ==="
echo "  $TARBALL  ($SIZE)"
echo ""
echo "To install from this tarball:"
echo "  tar xf $PKG_NAME.tar.gz"
echo "  cd $PKG_NAME"
echo "  ./scripts/install-user.sh"
