#!/usr/bin/env bash
# Build target/dist/tern-<version>-linux-<arch>.tar.gz (binary, desktop entry, icon, install.sh).
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="tern-app"))')
arch=$(uname -m)
name="tern-${version}-linux-${arch}"
stage="target/dist/${name}"

cargo build --release -p tern-app
rm -rf "target/dist/${name}" "target/dist/${name}.tar.gz"
mkdir -p "${stage}"
install -m 0755 target/release/tern "${stage}/tern"
install -m 0644 packaging/tern.desktop packaging/tern.svg README.md LICENSE* "${stage}/" 2>/dev/null || \
  install -m 0644 packaging/tern.desktop packaging/tern.svg README.md "${stage}/"
install -m 0755 packaging/install.sh "${stage}/install.sh"
tar -C target/dist -czf "target/dist/${name}.tar.gz" "${name}"
echo "built target/dist/${name}.tar.gz"
