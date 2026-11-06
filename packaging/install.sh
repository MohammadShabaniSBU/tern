#!/usr/bin/env bash
# Install tern for the current user (default) or system-wide: PREFIX=/usr/local sudo ./install.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
prefix="${PREFIX:-$HOME/.local}"

install -Dm 0755 "$here/tern" "$prefix/bin/tern"
install -Dm 0644 "$here/tern.desktop" "$prefix/share/applications/tern.desktop"
install -Dm 0644 "$here/tern.svg" "$prefix/share/icons/hicolor/scalable/apps/tern.svg"
command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/share/applications" || true
echo "installed to $prefix (make sure $prefix/bin is on your PATH)"
