#!/usr/bin/env bash
# Build target/dist/tern-<arch>.AppImage. Needs `appimagetool` on PATH (or APPIMAGETOOL=…).
set -euo pipefail
cd "$(dirname "$0")/.."

tool="${APPIMAGETOOL:-appimagetool}"
command -v "$tool" >/dev/null || { echo "appimagetool not found (https://github.com/AppImage/appimagetool)"; exit 1; }

cargo build --release -p tern-app
appdir=target/dist/tern.AppDir
rm -rf "$appdir"
install -Dm 0755 target/release/tern "$appdir/usr/bin/tern"
install -Dm 0644 packaging/tern.desktop "$appdir/usr/share/applications/tern.desktop"
install -Dm 0644 packaging/tern.desktop "$appdir/tern.desktop"
install -Dm 0644 packaging/tern.svg "$appdir/usr/share/icons/hicolor/scalable/apps/tern.svg"
install -Dm 0644 packaging/tern.svg "$appdir/tern.svg"
ln -s usr/bin/tern "$appdir/AppRun"
ARCH="$(uname -m)" "$tool" "$appdir" "target/dist/tern-$(uname -m).AppImage"
