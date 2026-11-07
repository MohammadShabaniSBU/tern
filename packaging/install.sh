#!/usr/bin/env bash
# Install tern for the current user (default) or system-wide: PREFIX=/usr/local sudo ./install.sh
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
prefix="${PREFIX:-$HOME/.local}"

if [ -f "$here/tern" ]; then
  bin="$here/tern"                        # inside the release tarball
elif [ -f "$here/../target/release/tern" ]; then
  bin="$here/../target/release/tern"      # repo checkout
else
  echo "tern binary not found; build first: cargo build --release -p tern-app" >&2
  exit 1
fi

install -Dm 0755 "$bin" "$prefix/bin/tern"
# Launchers often lack ~/.local/bin on PATH, so point Exec at the absolute path.
install -Dm 0644 /dev/null "$prefix/share/applications/tern.desktop"
sed "s|^Exec=tern|Exec=$prefix/bin/tern|" "$here/tern.desktop" > "$prefix/share/applications/tern.desktop"
install -Dm 0644 "$here/tern.svg" "$prefix/share/icons/hicolor/scalable/apps/tern.svg"
command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/share/applications" || true
echo "installed to $prefix (make sure $prefix/bin is on your PATH)"
