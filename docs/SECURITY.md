# Security model

tern is a local, single-user app. This document says what its encryption protects and what it
does not.

## Protects against

- **A stolen disk, laptop or copied `tern.db`.** Secrets (host passwords, private keys,
  key passphrases) are stored only as XChaCha20-Poly1305 ciphertext. The data key is wrapped by a
  key derived from the master password with Argon2id (default 128 MiB / 8 passes; adjustable in
  Settings → Security). Without the master password (or the recovery key) the file reveals only
  non-secret metadata: host names, labels, tags, tunnel definitions.
- **Tampering/swapping of secrets between rows.** Each secret is bound to its table, column and
  row id as AEAD associated data.
- **A stolen backup.** `.tern` bundles are encrypted with their own password and KDF salt
  (see `EXPORT_FORMAT.md`). Plaintext exports are an explicit, re-authenticated action.
- **Shoulder-surfing / walking away.** Idle auto-lock (default 15 min, also after the machine
  sleeps) zeroizes the data key, drops decrypted UI state and clears the clipboard if tern put a
  password on it. Setting `lock.keep_sessions` keeps open terminals and tunnels running while
  locked; the default closes them.
- **Network attackers.** Host keys are verified for every hop (including jump hosts); changed keys
  are flagged and refused unless the user explicitly replaces them.

## Does not protect against

- **Malware or another process running as your user while the vault is unlocked.** It can read
  tern's memory, the clipboard, your terminal output, or your SSH agent. No desktop app can
  defend against this.
- **A compromised unlocked session.** Anything on screen or in a terminal scrollback is plain text
  in memory.
- **Swapping to disk.** The data key lives in a zeroize-on-drop box but is not `mlock`ed: the
  workspace forbids `unsafe`, and locking pages needs it. Use encrypted swap.
- **Weak master passwords.** Argon2id slows guessing; it does not stop it.
- **Metadata exposure.** Host names, usernames, labels and known-host fingerprints are not
  encrypted in `tern.db`.
- **Loss of both the master password and the recovery key.** There is no reset.
- **The private-key editor.** While a key is being pasted in the add-key form, the text lives in
  a UI text buffer that cannot be zeroized; it is dropped when the form closes or on lock.

## Rules the code follows

- Secrets are `SecretString`/`Zeroizing` (`secrecy`, `zeroize`); UI password fields use the
  `Sensitive` wrapper.
- No secret type implements `Debug`/`Display` with its contents (tests in `tern-ssh`, `tern-io`,
  `tern-vault`, `tern-app` assert this). Logs never include passwords, keys, passphrases or
  decrypted data.
- The backup password is never stored; scheduled backups ask for it after each unlock.
