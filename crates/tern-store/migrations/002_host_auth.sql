-- Identities are gone: a host carries its own auth. Existing identity data is folded into the
-- hosts that used it.
--
-- The legacy `identities` table and `hosts.identity_id` column stay (emptied / nulled) because
-- SQLite cannot drop a column that carries a foreign key without rebuilding `hosts`, which would
-- cascade-delete tunnels. They are unused.
--
-- A password sealed for an identity is bound to AAD `identities:password_enc:<id>`; copying the
-- blob keeps that AAD in `password_aad` until the password is next saved (re-sealed for the host).
ALTER TABLE hosts ADD COLUMN auth_kind TEXT NOT NULL DEFAULT 'ask'
    CHECK (auth_kind IN ('ask', 'password', 'key'));
ALTER TABLE hosts ADD COLUMN ssh_key_id TEXT NULL REFERENCES ssh_keys(id) ON DELETE SET NULL;
ALTER TABLE hosts ADD COLUMN password_enc BLOB NULL;
ALTER TABLE hosts ADD COLUMN password_aad TEXT NULL;

UPDATE hosts SET
    username = COALESCE(NULLIF(username, ''),
                        (SELECT username FROM identities i WHERE i.id = hosts.identity_id)),
    ssh_key_id = (SELECT ssh_key_id FROM identities i WHERE i.id = hosts.identity_id),
    password_enc = (SELECT password_enc FROM identities i WHERE i.id = hosts.identity_id),
    password_aad = (SELECT 'identities:password_enc:' || i.id FROM identities i
                    WHERE i.id = hosts.identity_id AND i.password_enc IS NOT NULL)
WHERE identity_id IS NOT NULL;

UPDATE hosts SET auth_kind = CASE
    WHEN ssh_key_id IS NOT NULL THEN 'key'
    WHEN password_enc IS NOT NULL THEN 'password'
    ELSE 'ask' END;

UPDATE hosts SET identity_id = NULL;
DELETE FROM identities;
