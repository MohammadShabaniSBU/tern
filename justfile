default:
    @just --list

fmt:
    cargo fmt --all

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

run:
    cargo run -p tern-app

# Linux release tarball in target/dist/
package:
    ./packaging/build-tarball.sh

# AppImage (needs appimagetool on PATH)
appimage:
    ./packaging/build-appimage.sh

# .deb (needs `cargo install cargo-deb`; run on Debian/Ubuntu so $auto deps resolve)
deb:
    cargo build --release -p tern-app
    cargo deb -p tern-app --no-build
