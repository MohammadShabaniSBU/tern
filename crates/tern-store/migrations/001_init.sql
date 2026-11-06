-- Initial schema. Secret columns (*_enc) hold `version || nonce || ciphertext` blobs sealed with
-- the vault's data key; everything else is plaintext (see ADR-1).

-- One row. Holds what is needed to unlock the vault. The nonce for wrapped_data_key is embedded
-- in the blob itself, so there is no separate wrap_nonce column.
CREATE TABLE vault_meta (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    kdf_params TEXT NOT NULL,        -- JSON: { m_cost, t_cost, p_cost }
    kdf_salt BLOB NOT NULL,
    wrapped_data_key BLOB NOT NULL,  -- data key sealed under the KEK
    created_at TEXT NOT NULL
);

CREATE TABLE groups (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    parent_id TEXT NULL REFERENCES groups(id) ON DELETE SET NULL,
    sort_order INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE ssh_keys (
    id TEXT PRIMARY KEY,
    label TEXT NOT NULL,
    key_type TEXT NOT NULL,
    public_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    private_key_enc BLOB NOT NULL,
    passphrase_enc BLOB NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE identities (
    id TEXT PRIMARY KEY,
    label TEXT NOT NULL,
    username TEXT NOT NULL,
    password_enc BLOB NULL,
    ssh_key_id TEXT NULL REFERENCES ssh_keys(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE hosts (
    id TEXT PRIMARY KEY,
    label TEXT NOT NULL,
    hostname TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 22 CHECK (port BETWEEN 1 AND 65535),
    username TEXT NULL,              -- overrides identity username
    identity_id TEXT NULL REFERENCES identities(id) ON DELETE SET NULL,
    group_id TEXT NULL REFERENCES groups(id) ON DELETE SET NULL,
    jump_host_id TEXT NULL REFERENCES hosts(id) ON DELETE SET NULL,
    tags TEXT NOT NULL DEFAULT '[]', -- JSON array
    notes TEXT NULL,
    keepalive_secs INTEGER NULL,
    last_connected_at TEXT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX hosts_group_id ON hosts(group_id);

CREATE TABLE known_hosts (
    id TEXT PRIMARY KEY,
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    key_type TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    public_key TEXT NOT NULL,
    first_seen_at TEXT NOT NULL,
    UNIQUE (host, port, key_type)
);

CREATE TABLE tunnels (
    id TEXT PRIMARY KEY,
    label TEXT NOT NULL,
    host_id TEXT NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('local', 'remote', 'dynamic')),
    bind_addr TEXT NOT NULL DEFAULT '127.0.0.1',
    bind_port INTEGER NOT NULL CHECK (bind_port BETWEEN 1 AND 65535),
    target_host TEXT NULL,           -- NULL for dynamic
    target_port INTEGER NULL,        -- NULL for dynamic
    auto_start INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX tunnels_host_id ON tunnels(host_id);

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL              -- JSON; keymap overrides live here
);
