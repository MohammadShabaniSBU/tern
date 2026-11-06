# Performance notes (S8-T06)

Measured on the author's desktop (release build, 2026-11-06).

| Target | Result | How to re-measure |
|---|---|---|
| Start → first screen < 1 s | ~0.5 s from process start to the unlock screen | `TERN_DATA_DIR=$(mktemp -d) target/release/tern-app`, then read `first screen ready elapsed_ms=…` in `logs/` |
| Host list with 1,000 hosts | build items 0.5 ms, load list ~5 µs, filter ~50 µs | `cargo test -p tern-app --release perf_ -- --ignored --nocapture` |
| Terminal throughput | ~90 MiB/s through the emulator (50 MiB of colored lines); one snapshot ~140 µs | `cargo test -p tern-term --release perf_ -- --ignored --nocapture` |

The unlock itself (Argon2id, default cost) adds ~0.4 s after the password is entered; Settings →
Security can lower or raise the cost. The host list is virtualised (only visible rows are built),
and remote output is coalesced to ~16 ms / 64 KiB batches before it reaches the UI.

No code changes were needed: all targets were met on the first measurement.
