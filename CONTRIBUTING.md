# Contributing

Run `just fmt`, `just lint` and `just test` before every commit. See `docs/README.md` for the Definition of Done.

## Never log secrets

Passwords, private keys, passphrases, decrypted data keys, the master password and decrypted vault contents must **never** appear in logs, error messages or `Debug` output.

- Wrap secret values in `secrecy::SecretString` / `SecretBox` (and `zeroize` where needed) as soon as they enter the program. Their `Debug` impl prints `[REDACTED]`.
- Do not derive `Debug` on a struct holding a raw secret `String`/`Vec<u8>`; wrap the field or implement `Debug` by hand.
- Do not put secrets in `tracing` fields or in `thiserror` messages. Log identifiers (host id, label, key fingerprint) instead.
- Logging `?host` is fine only if `Host` contains no secret material (`tern-core` types intentionally don't).
- Before logging any new type, ask: could a field of this ever hold a secret?

## Logging

`RUST_LOG` selects the filter (default `info`), e.g. `RUST_LOG=tern_app=debug,russh=warn`.
Debug builds log to stdout; release builds log to `<data dir>/logs/tern.log.<date>`.
Override the data dir with `TERN_DATA_DIR`.

## Errors

Library crates define their own `Error` with `thiserror` (`tern-core` uses `ValidationError`). The `tern-app` binary uses `anyhow` at the edges.
