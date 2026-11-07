# tern backup format (`tern-backup`, version 1)

A backup is a UTF-8 JSON file (conventionally `*.tern`) with an unencrypted envelope and an encrypted payload.

```json
{
  "format": "tern-backup",
  "version": 1,
  "created_at": "2026-11-06T12:00:00Z",
  "kdf": { "alg": "argon2id", "m_cost": 65536, "t_cost": 3, "p_cost": 1, "salt": "<b64, 16 bytes>" },
  "cipher": "xchacha20poly1305",
  "payload": "<b64 of seal output>"
}
```

- Key: Argon2id(export password, salt, kdf params) → 32 bytes. The export password is independent of the master password.
- `payload` = base64 of `version(1) || nonce(24) || ciphertext || tag` (XChaCha20-Poly1305). The nonce is embedded, so there is no separate `nonce` field. AAD is the ASCII string `tern-backup:v1`.
- Readers must reject unknown `format`/`version`, and KDF costs beyond m≤1 GiB, t≤64, p≤16.
- A wrong password and a tampered file both fail authentication and are reported as "wrong password, or the backup is corrupted".

## Decrypted payload (JSON)
`groups`, `hosts` (each with `username` and `auth`: `{"kind":"ask"|"password"}` or `{"kind":"key","key_id":…}`), `host_passwords` (`{host_id, password}`), `ssh_keys` (flattened key metadata + `private_key`, optional `passphrase`), `tunnels`, `known_hosts`, `settings` (array of `[key, json-text]`). Entity ids are UUIDs and are preserved, so backups can be merged across machines. Secrets are re-encrypted with the importing vault's data key; ciphertext is never copied between vaults.

## Versioning
Additive changes keep `version: 1` (unknown payload fields are ignored; missing arrays default to empty). Breaking changes bump `version`.
