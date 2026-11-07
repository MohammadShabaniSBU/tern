//! Integration tests for the repositories against an in-memory database.
#![allow(clippy::unwrap_used)] // test code: panicking on failure is the point

use rusqlite::Connection;
use secrecy::{ExposeSecret, SecretString};
use tern_core::*;
use tern_store::*;
use tern_vault::{KdfParams, Vault};

fn db() -> Connection {
    open_in_memory().unwrap()
}

fn vault() -> Vault {
    let pw = SecretString::from("master");
    Vault::create_with_params(&pw, KdfParams::new(8, 1, 1).unwrap())
        .unwrap()
        .0
}

fn secret(s: &str) -> SecretString {
    SecretString::from(s)
}

fn group(name: &str) -> Group {
    Group {
        id: GroupId::new(),
        name: name.into(),
        parent_id: None,
        sort_order: 0,
    }
}

fn key_meta(label: &str) -> SshKeyMeta {
    SshKeyMeta {
        id: KeyId::new(),
        label: label.into(),
        key_type: "ssh-ed25519".into(),
        public_key: "ssh-ed25519 AAAA".into(),
        fingerprint: "SHA256:abc".into(),
    }
}

fn tunnel(host: HostId) -> Tunnel {
    Tunnel {
        id: TunnelId::new(),
        label: "db".into(),
        host_id: host,
        kind: TunnelKind::Local,
        bind_addr: "127.0.0.1".into(),
        bind_port: 5433,
        target_host: Some("localhost".into()),
        target_port: Some(5432),
        auto_start: true,
    }
}

// ---------- hosts ----------

#[test]
fn host_crud() {
    let conn = db();
    let repo = HostRepo::new(&conn);
    let mut host = Host::new("Web", "web.example.com");
    host.tags = vec!["prod".into(), "eu".into()];
    host.notes = Some("hello".into());
    host.keepalive_secs = Some(30);
    host.username = Some("deploy".into());

    assert!(repo.get(host.id).unwrap().is_none());
    repo.insert(&host).unwrap();
    assert_eq!(repo.get(host.id).unwrap().unwrap(), host);
    assert_eq!(repo.list().unwrap(), vec![host.clone()]);

    host.label = "Web 2".into();
    host.port = 2222;
    host.tags.clear();
    repo.update(&host).unwrap();
    assert_eq!(repo.get(host.id).unwrap().unwrap(), host);

    assert!(repo.delete(host.id).unwrap());
    assert!(!repo.delete(host.id).unwrap());
    assert!(repo.list().unwrap().is_empty());
}

#[test]
fn host_list_is_sorted_case_insensitively() {
    let conn = db();
    let repo = HostRepo::new(&conn);
    for label in ["bravo", "Alpha", "charlie"] {
        repo.insert(&Host::new(label, "h")).unwrap();
    }
    let labels: Vec<_> = repo.list().unwrap().into_iter().map(|h| h.label).collect();
    assert_eq!(labels, ["Alpha", "bravo", "charlie"]);
}

#[test]
fn host_validation_and_not_found() {
    let conn = db();
    let repo = HostRepo::new(&conn);
    let mut bad = Host::new("x", "h");
    bad.port = 0;
    assert!(matches!(repo.insert(&bad), Err(Error::Validation(_))));
    assert!(matches!(
        repo.update(&Host::new("x", "h")),
        Err(Error::NotFound { .. })
    ));
}

#[test]
fn host_last_connected() {
    let conn = db();
    let repo = HostRepo::new(&conn);
    let host = Host::new("x", "h");
    repo.insert(&host).unwrap();
    assert_eq!(repo.last_connected_at(host.id).unwrap(), None);
    repo.touch_last_connected(host.id).unwrap();
    let ts = repo.last_connected_at(host.id).unwrap().unwrap();
    assert!(ts.starts_with("20") && ts.ends_with('Z'), "{ts}");
}

#[test]
fn deleting_group_moves_hosts_and_subgroups_to_top_level() {
    let conn = db();
    let groups = GroupRepo::new(&conn);
    let hosts = HostRepo::new(&conn);
    let parent = group("parent");
    let mut child = group("child");
    child.parent_id = Some(parent.id);
    groups.insert(&parent).unwrap();
    groups.insert(&child).unwrap();
    let mut host = Host::new("h", "h");
    host.group_id = Some(parent.id);
    hosts.insert(&host).unwrap();

    assert!(groups.delete(parent.id).unwrap());
    assert_eq!(hosts.get(host.id).unwrap().unwrap().group_id, None);
    assert_eq!(groups.get(child.id).unwrap().unwrap().parent_id, None);
}

#[test]
fn deleting_jump_host_unsets_reference_but_keeps_host() {
    let conn = db();
    let hosts = HostRepo::new(&conn);
    let jump = Host::new("jump", "j");
    let mut target = Host::new("target", "t");
    target.jump_host_id = Some(jump.id);
    hosts.insert(&jump).unwrap();
    hosts.insert(&target).unwrap();

    hosts.delete(jump.id).unwrap();
    assert_eq!(hosts.get(target.id).unwrap().unwrap().jump_host_id, None);
}

#[test]
fn inserting_host_with_dangling_reference_fails() {
    let conn = db();
    let mut host = Host::new("h", "h");
    host.group_id = Some(GroupId::new());
    assert!(HostRepo::new(&conn).insert(&host).is_err());
}

// ---------- groups ----------

#[test]
fn group_crud() {
    let conn = db();
    let repo = GroupRepo::new(&conn);
    let mut g = group("one");
    repo.insert(&g).unwrap();
    assert_eq!(repo.get(g.id).unwrap().unwrap(), g);
    g.name = "renamed".into();
    g.sort_order = 3;
    repo.update(&g).unwrap();
    assert_eq!(repo.list().unwrap(), vec![g.clone()]);
    assert!(repo.delete(g.id).unwrap());
    assert!(repo.get(g.id).unwrap().is_none());
    assert!(matches!(repo.update(&g), Err(Error::NotFound { .. })));
}

// ---------- host auth & passwords ----------

fn host_with_password(hosts: &HostRepo<'_>, vault: &Vault, label: &str, pw: &str) -> Host {
    let mut host = Host::new(label, "h.example");
    host.username = Some("root".into());
    host.auth = HostAuth::Password;
    hosts.insert(&host).unwrap();
    hosts
        .set_password(host.id, Some(&secret(pw)), vault)
        .unwrap();
    host
}

#[test]
fn host_auth_round_trips() {
    let conn = db();
    let vault = vault();
    let keys = KeyRepo::new(&conn);
    let hosts = HostRepo::new(&conn);
    let meta = key_meta("k");
    keys.insert(&meta, &secret("PEM"), None, &vault).unwrap();
    for auth in [HostAuth::Ask, HostAuth::Password, HostAuth::Key(meta.id)] {
        let mut host = Host::new("h", "h");
        host.auth = auth;
        hosts.insert(&host).unwrap();
        assert_eq!(hosts.get(host.id).unwrap().unwrap(), host);
        host.auth = HostAuth::Ask;
        hosts.update(&host).unwrap();
        assert_eq!(hosts.get(host.id).unwrap().unwrap().auth, HostAuth::Ask);
    }
}

#[test]
fn host_password_round_trip() {
    let conn = db();
    let vault = vault();
    let hosts = HostRepo::new(&conn);
    let host = host_with_password(&hosts, &vault, "a", "hunter2");
    assert!(hosts.has_password(host.id).unwrap());
    let got = hosts.get_password(host.id, &vault).unwrap().unwrap();
    assert_eq!(got.expose_secret(), "hunter2");

    // Updating non-secret fields leaves the password alone.
    let mut edited = host.clone();
    edited.username = Some("admin".into());
    hosts.update(&edited).unwrap();
    let got = hosts.get_password(host.id, &vault).unwrap().unwrap();
    assert_eq!(got.expose_secret(), "hunter2");

    hosts.set_password(host.id, None, &vault).unwrap();
    assert!(!hosts.has_password(host.id).unwrap());
    assert!(hosts.get_password(host.id, &vault).unwrap().is_none());
}

#[test]
fn password_is_not_stored_in_plaintext() {
    let conn = db();
    let vault = vault();
    let password = "correct-horse-battery-staple";
    let hosts = HostRepo::new(&conn);
    host_with_password(&hosts, &vault, "a", password);

    let blob: Vec<u8> = conn
        .query_row("SELECT password_enc FROM hosts", [], |r| r.get(0))
        .unwrap();
    assert!(!blob.is_empty());
    assert!(
        !blob
            .windows(password.len())
            .any(|w| w == password.as_bytes()),
        "plaintext found in password_enc"
    );
}

#[test]
fn ciphertext_cannot_be_swapped_between_rows() {
    let conn = db();
    let vault = vault();
    let hosts = HostRepo::new(&conn);
    let a = host_with_password(&hosts, &vault, "a", "pw-a");
    let b = host_with_password(&hosts, &vault, "b", "pw-b");
    conn.execute(
        "UPDATE hosts SET password_enc = (SELECT password_enc FROM hosts WHERE id = ?1) \
         WHERE id = ?2",
        [a.id.to_string(), b.id.to_string()],
    )
    .unwrap();
    assert!(hosts.get_password(b.id, &vault).is_err());
}

#[test]
fn password_needs_the_right_vault() {
    let conn = db();
    let hosts = HostRepo::new(&conn);
    let host = host_with_password(&hosts, &vault(), "a", "pw");
    assert!(hosts.get_password(host.id, &vault()).is_err());
}

#[test]
fn deleting_key_unsets_host_key() {
    let conn = db();
    let vault = vault();
    let keys = KeyRepo::new(&conn);
    let hosts = HostRepo::new(&conn);
    let meta = key_meta("k");
    keys.insert(&meta, &secret("PEM"), None, &vault).unwrap();
    let mut host = Host::new("h", "h");
    host.auth = HostAuth::Key(meta.id);
    hosts.insert(&host).unwrap();
    assert_eq!(hosts.count_using_key(meta.id).unwrap(), 1);

    keys.delete(meta.id).unwrap();
    // The repo only unsets the reference; `Store::delete_key` refuses earlier.
    assert_eq!(hosts.get(host.id).unwrap().unwrap().auth, HostAuth::Ask);
}

// ---------- keys ----------

#[test]
fn key_crud_and_secrets() {
    let conn = db();
    let vault = vault();
    let repo = KeyRepo::new(&conn);
    let mut meta = key_meta("deploy key");
    let pem =
        "-----BEGIN OPENSSH PRIVATE KEY-----\nsecretsecret\n-----END OPENSSH PRIVATE KEY-----\n";

    repo.insert(&meta, &secret(pem), Some(&secret("pass")), &vault)
        .unwrap();
    assert_eq!(repo.get(meta.id).unwrap().unwrap(), meta);
    assert_eq!(repo.list().unwrap(), vec![meta.clone()]);
    assert_eq!(
        repo.get_private_key(meta.id, &vault)
            .unwrap()
            .unwrap()
            .expose_secret(),
        pem
    );
    assert_eq!(
        repo.get_passphrase(meta.id, &vault)
            .unwrap()
            .unwrap()
            .expose_secret(),
        "pass"
    );

    meta.label = "renamed".into();
    repo.update(&meta).unwrap();
    assert_eq!(repo.get(meta.id).unwrap().unwrap().label, "renamed");

    // Raw inspection: neither secret appears in the stored bytes.
    let (priv_enc, pass_enc): (Vec<u8>, Vec<u8>) = conn
        .query_row(
            "SELECT private_key_enc, passphrase_enc FROM ssh_keys",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(!priv_enc.windows(12).any(|w| w == b"secretsecret"));
    assert!(!pass_enc.windows(4).any(|w| w == b"pass"));

    assert!(repo.delete(meta.id).unwrap());
    assert!(repo.get_private_key(meta.id, &vault).unwrap().is_none());
}

#[test]
fn key_without_passphrase() {
    let conn = db();
    let vault = vault();
    let repo = KeyRepo::new(&conn);
    let meta = key_meta("k");
    repo.insert(&meta, &secret("PEM"), None, &vault).unwrap();
    assert!(repo.get_passphrase(meta.id, &vault).unwrap().is_none());
}

// ---------- tunnels ----------

#[test]
fn tunnel_crud() {
    let conn = db();
    let hosts = HostRepo::new(&conn);
    let repo = TunnelRepo::new(&conn);
    let host = Host::new("h", "h");
    hosts.insert(&host).unwrap();
    let mut t = tunnel(host.id);

    repo.insert(&t).unwrap();
    assert_eq!(repo.get(t.id).unwrap().unwrap(), t);
    assert_eq!(repo.list_for_host(host.id).unwrap(), vec![t.clone()]);

    t.kind = TunnelKind::Dynamic;
    t.target_host = None;
    t.target_port = None;
    t.auto_start = false;
    repo.update(&t).unwrap();
    assert_eq!(repo.get(t.id).unwrap().unwrap(), t);

    assert!(repo.delete(t.id).unwrap());
    assert!(repo.list().unwrap().is_empty());
}

#[test]
fn tunnel_validation_is_enforced() {
    let conn = db();
    let host = Host::new("h", "h");
    HostRepo::new(&conn).insert(&host).unwrap();
    let mut t = tunnel(host.id);
    t.target_host = None;
    assert!(matches!(
        TunnelRepo::new(&conn).insert(&t),
        Err(Error::Validation(_))
    ));
}

#[test]
fn deleting_host_cascades_to_tunnels() {
    let conn = db();
    let hosts = HostRepo::new(&conn);
    let tunnels = TunnelRepo::new(&conn);
    let host = Host::new("h", "h");
    hosts.insert(&host).unwrap();
    tunnels.insert(&tunnel(host.id)).unwrap();
    tunnels.insert(&tunnel(host.id)).unwrap();
    assert_eq!(tunnels.list().unwrap().len(), 2);

    hosts.delete(host.id).unwrap();
    assert!(tunnels.list().unwrap().is_empty());
}

#[test]
fn tunnel_for_missing_host_is_rejected() {
    let conn = db();
    assert!(
        TunnelRepo::new(&conn)
            .insert(&tunnel(HostId::new()))
            .is_err()
    );
}

// ---------- known hosts ----------

fn known(host: &str, key_type: &str) -> KnownHost {
    KnownHost {
        id: KnownHostId::new(),
        host: host.into(),
        port: 22,
        key_type: key_type.into(),
        fingerprint: "SHA256:xyz".into(),
        public_key: "AAAA".into(),
    }
}

#[test]
fn known_host_crud_and_uniqueness() {
    let conn = db();
    let repo = KnownHostRepo::new(&conn);
    let mut k = known("example.com", "ssh-ed25519");
    repo.insert(&k).unwrap();
    assert_eq!(repo.get(k.id).unwrap().unwrap(), k);
    assert_eq!(
        repo.find("example.com", 22, "ssh-ed25519")
            .unwrap()
            .unwrap(),
        k
    );
    assert!(repo.find("example.com", 22, "ssh-rsa").unwrap().is_none());
    assert!(
        repo.find("example.com", 2222, "ssh-ed25519")
            .unwrap()
            .is_none()
    );

    // UNIQUE(host, port, key_type)
    assert!(repo.insert(&known("example.com", "ssh-ed25519")).is_err());
    // A different key type for the same host is fine.
    repo.insert(&known("example.com", "ssh-rsa")).unwrap();
    assert_eq!(repo.list().unwrap().len(), 2);

    k.fingerprint = "SHA256:changed".into();
    repo.update(&k).unwrap();
    assert_eq!(
        repo.get(k.id).unwrap().unwrap().fingerprint,
        "SHA256:changed"
    );

    assert!(repo.delete(k.id).unwrap());
    assert_eq!(repo.list().unwrap().len(), 1);
}

// ---------- settings ----------

#[test]
fn settings_crud() {
    let conn = db();
    let repo = SettingsRepo::new(&conn);
    assert_eq!(repo.get("keymap").unwrap(), None);
    repo.set("keymap", r#"{"a":1}"#).unwrap();
    repo.set("theme", r#""dark""#).unwrap();
    repo.set("keymap", r#"{"a":2}"#).unwrap(); // upsert
    assert_eq!(repo.get("keymap").unwrap().as_deref(), Some(r#"{"a":2}"#));
    assert_eq!(repo.list().unwrap().len(), 2);
    assert!(repo.delete("theme").unwrap());
    assert!(!repo.delete("theme").unwrap());
}

// ---------- vault meta ----------

#[test]
fn vault_meta_round_trip_and_unlock() {
    let conn = db();
    let repo = VaultMetaRepo::new(&conn);
    assert!(!repo.exists().unwrap());
    assert!(repo.get().unwrap().is_none());

    let pw = secret("master");
    let (vault, meta) = Vault::create_with_params(&pw, KdfParams::new(8, 1, 1).unwrap()).unwrap();
    repo.insert(&meta).unwrap();
    assert!(repo.exists().unwrap());
    // Only one vault per database.
    assert!(repo.insert(&meta).is_err());

    let loaded = repo.get().unwrap().unwrap();
    assert_eq!(loaded, meta);
    let blob = vault.seal(b"x", b"a").unwrap();
    let unlocked = Vault::unlock(&pw, &loaded).unwrap();
    assert_eq!(
        unlocked
            .open(&blob, b"a")
            .unwrap()
            .expose_secret()
            .as_slice(),
        b"x"
    );

    let new_meta = vault.change_password(&pw, &secret("new"), &meta).unwrap();
    repo.update(&new_meta).unwrap();
    assert_eq!(repo.get().unwrap().unwrap(), new_meta);
}

#[test]
fn listing_never_touches_secrets() {
    // Listing works even with a corrupt secret column, proving it does not decrypt.
    let conn = db();
    let vault = vault();
    let hosts = HostRepo::new(&conn);
    let host = host_with_password(&hosts, &vault, "a", "pw");
    conn.execute("UPDATE hosts SET password_enc = x'00'", [])
        .unwrap();
    assert_eq!(hosts.list().unwrap(), vec![host]);
}
