#!/usr/bin/env bash
# Stop Spotlight Files (Electron + daemon).
set -euo pipefail
pgrep -x spotlight-files | xargs -r kill 2>/dev/null
pgrep -x electron | xargs -r kill 2>/dev/null
rm -f /tmp/spotlight-files.sock
echo "Spotlight Files stopped."
