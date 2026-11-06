//! Import planning and application: snapshot the local DB, diff against a decrypted bundle, and
//! apply it atomically with a conflict strategy.

use std::collections::HashMap;

use secrecy::{ExposeSecret, SecretString};
use tern_core::{GroupId, HostId, IdentityId, KeyId, TunnelId};
use tern_store::Db;

use crate::Result;
use crate::bundle::{IdentityEntry, KeyEntry, Payload};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Skip,
    Overwrite,
    KeepBoth,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub new: usize,
    pub conflicting: usize,
    pub identical: usize,
}

impl Counts {
    fn add(&mut self, o: Counts) {
        self.new += o.new;
        self.conflicting += o.conflicting;
        self.identical += o.identical;
    }
}

/// Export scope: everything, or only hosts in these groups (plus what they need).
#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub groups: Option<Vec<GroupId>>,
}

/// Read the whole database (decrypting secrets) into a [`Payload`].
pub fn snapshot(db: &Db, scope: &Scope) -> Result<Payload> {
    let vault = db.vault()?;
    let mut p = Payload {
        groups: db.groups().list()?,
        hosts: db.hosts().list()?,
        ..Payload::default()
    };
    let identities = db.identities().list()?;
    let keys = db.keys().list()?;
    p.tunnels = db.tunnels().list()?;
    p.known_hosts = db.known_hosts().list()?;
    p.settings = db.settings().list()?;

    if let Some(sel) = &scope.groups {
        // Selected groups and their descendants.
        let mut keep: Vec<GroupId> = sel.clone();
        loop {
            let before = keep.len();
            for g in &p.groups {
                if let Some(parent) = g.parent_id
                    && keep.contains(&parent)
                    && !keep.contains(&g.id)
                {
                    keep.push(g.id);
                }
            }
            if keep.len() == before {
                break;
            }
        }
        p.hosts
            .retain(|h| h.group_id.is_some_and(|g| keep.contains(&g)));
        // Keep ancestors so the tree stays valid.
        let mut need = keep.clone();
        for g in keep.clone() {
            let mut cur = p
                .groups
                .iter()
                .find(|x| x.id == g)
                .and_then(|x| x.parent_id);
            while let Some(id) = cur {
                if !need.contains(&id) {
                    need.push(id);
                }
                cur = p
                    .groups
                    .iter()
                    .find(|x| x.id == id)
                    .and_then(|x| x.parent_id);
            }
        }
        p.groups.retain(|g| need.contains(&g.id));
        // Jump hosts of kept hosts must come along.
        loop {
            let ids: Vec<HostId> = p.hosts.iter().map(|h| h.id).collect();
            let all = db.hosts().list()?;
            let missing: Vec<_> = p
                .hosts
                .iter()
                .filter_map(|h| h.jump_host_id)
                .filter(|j| !ids.contains(j))
                .collect();
            if missing.is_empty() {
                break;
            }
            p.hosts
                .extend(all.into_iter().filter(|h| missing.contains(&h.id)));
        }
        let host_ids: Vec<HostId> = p.hosts.iter().map(|h| h.id).collect();
        p.tunnels.retain(|t| host_ids.contains(&t.host_id));
        p.known_hosts.clear();
        p.settings.clear();
        let used_ident: Vec<IdentityId> = p.hosts.iter().filter_map(|h| h.identity_id).collect();
        let identities: Vec<_> = identities
            .into_iter()
            .filter(|i| used_ident.contains(&i.id))
            .collect();
        let used_keys: Vec<KeyId> = identities.iter().filter_map(|i| i.ssh_key_id).collect();
        let keys: Vec<_> = keys
            .into_iter()
            .filter(|k| used_keys.contains(&k.id))
            .collect();
        fill_secrets(db, vault, &mut p, identities, keys)?;
        return Ok(p);
    }
    fill_secrets(db, vault, &mut p, identities, keys)?;
    Ok(p)
}

fn fill_secrets(
    db: &Db,
    vault: &tern_vault::Vault,
    p: &mut Payload,
    identities: Vec<tern_core::Identity>,
    keys: Vec<tern_core::SshKeyMeta>,
) -> Result<()> {
    for identity in identities {
        let password = db
            .identities()
            .get_password(identity.id, vault)?
            .map(|s| s.expose_secret().to_string());
        p.identities.push(IdentityEntry { identity, password });
    }
    for meta in keys {
        let private_key = db
            .keys()
            .get_private_key(meta.id, vault)?
            .map(|s| s.expose_secret().to_string());
        let passphrase = db
            .keys()
            .get_passphrase(meta.id, vault)?
            .map(|s| s.expose_secret().to_string());
        p.ssh_keys.push(KeyEntry {
            meta,
            private_key,
            passphrase,
        });
    }
    Ok(())
}

/// Remove all secrets (for the plaintext export without secrets).
pub fn strip_secrets(p: &mut Payload) {
    for i in &mut p.identities {
        i.password = None;
    }
    for k in &mut p.ssh_keys {
        k.private_key = None;
        k.passphrase = None;
    }
}

fn classify<T: PartialEq>(incoming: &T, local: Option<&T>) -> Counts {
    match local {
        None => Counts {
            new: 1,
            ..Counts::default()
        },
        Some(l) if l == incoming => Counts {
            identical: 1,
            ..Counts::default()
        },
        Some(_) => Counts {
            conflicting: 1,
            ..Counts::default()
        },
    }
}

/// Preview of what importing `incoming` would do against `local`.
pub fn plan(incoming: &Payload, local: &Payload) -> Counts {
    fn by<T, K: std::hash::Hash + Eq>(v: &[T], f: impl Fn(&T) -> K) -> HashMap<K, &T> {
        v.iter().map(|x| (f(x), x)).collect()
    }
    let mut c = Counts::default();
    let g = by(&local.groups, |x| x.id);
    incoming
        .groups
        .iter()
        .for_each(|x| c.add(classify(x, g.get(&x.id).copied())));
    let h = by(&local.hosts, |x| x.id);
    incoming
        .hosts
        .iter()
        .for_each(|x| c.add(classify(x, h.get(&x.id).copied())));
    let i = by(&local.identities, |x| x.identity.id);
    incoming
        .identities
        .iter()
        .for_each(|x| c.add(classify(x, i.get(&x.identity.id).copied())));
    let k = by(&local.ssh_keys, |x| x.meta.id);
    incoming
        .ssh_keys
        .iter()
        .for_each(|x| c.add(classify(x, k.get(&x.meta.id).copied())));
    let t = by(&local.tunnels, |x| x.id);
    incoming
        .tunnels
        .iter()
        .for_each(|x| c.add(classify(x, t.get(&x.id).copied())));
    let kh = by(&local.known_hosts, |x| {
        (x.host.clone(), x.port, x.key_type.clone())
    });
    incoming.known_hosts.iter().for_each(|x| {
        let key = (x.host.clone(), x.port, x.key_type.clone());
        let l = kh.get(&key).copied();
        // Compare content except the surrogate id.
        let same =
            l.is_some_and(|l| l.fingerprint == x.fingerprint && l.public_key == x.public_key);
        c.add(match l {
            None => Counts {
                new: 1,
                ..Counts::default()
            },
            Some(_) if same => Counts {
                identical: 1,
                ..Counts::default()
            },
            Some(_) => Counts {
                conflicting: 1,
                ..Counts::default()
            },
        });
    });
    let s = by(&local.settings, |x| x.0.clone());
    incoming.settings.iter().for_each(|x| {
        c.add(match s.get(&x.0) {
            None => Counts {
                new: 1,
                ..Counts::default()
            },
            Some(l) if l.1 == x.1 => Counts {
                identical: 1,
                ..Counts::default()
            },
            Some(_) => Counts {
                conflicting: 1,
                ..Counts::default()
            },
        })
    });
    c
}

/// Counts of rows actually written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Applied {
    pub added: usize,
    pub overwritten: usize,
    pub skipped: usize,
}

fn secret(s: &Option<String>) -> Option<SecretString> {
    s.as_ref().map(|s| SecretString::from(s.clone()))
}

/// Apply `incoming` to the DB in a single transaction (all or nothing).
pub fn apply(db: &Db, incoming: Payload, strategy: Strategy) -> Result<Applied> {
    let tx = db
        .conn()
        .unchecked_transaction()
        .map_err(tern_store::Error::from)?;
    let r = apply_inner(db, incoming, strategy);
    match r {
        Ok(a) => {
            tx.commit().map_err(tern_store::Error::from)?;
            Ok(a)
        }
        Err(e) => Err(e), // dropping `tx` rolls back
    }
}

fn apply_inner(db: &Db, mut inc: Payload, strategy: Strategy) -> Result<Applied> {
    let vault = db.vault()?;
    let local = snapshot(db, &Scope::default())?;
    let mut out = Applied::default();
    let suffix = |l: &str| format!("{l} (imported)");

    // --- id remapping for KeepBoth ---
    let mut gmap: HashMap<GroupId, GroupId> = HashMap::new();
    let mut kmap: HashMap<KeyId, KeyId> = HashMap::new();
    let mut imap: HashMap<IdentityId, IdentityId> = HashMap::new();
    let mut hmap: HashMap<HostId, HostId> = HashMap::new();

    // Groups, parent-first.
    let mut pending = std::mem::take(&mut inc.groups);
    let mut done: Vec<GroupId> = Vec::new();
    let mut guard = 0;
    while !pending.is_empty() {
        guard += 1;
        if guard > 10_000 {
            return Err(crate::Error::Invalid("group cycle".into()));
        }
        let mut rest = Vec::new();
        for mut g in pending {
            if let Some(p) = g.parent_id
                && !done.contains(&p)
                && !local.groups.iter().any(|x| x.id == p)
                && !gmap.contains_key(&p)
            {
                rest.push(g);
                continue;
            }
            let orig = g.id;
            if let Some(p) = g.parent_id {
                g.parent_id = Some(*gmap.get(&p).unwrap_or(&p));
            }
            let existing = local.groups.iter().find(|x| x.id == orig);
            match existing {
                None => {
                    db.groups().insert(&g)?;
                    out.added += 1;
                }
                Some(l) if *l == g => out.skipped += 1,
                Some(_) => match strategy {
                    Strategy::Skip => out.skipped += 1,
                    Strategy::Overwrite => {
                        db.groups().update(&g)?;
                        out.overwritten += 1;
                    }
                    Strategy::KeepBoth => {
                        g.id = GroupId::new();
                        g.name = suffix(&g.name);
                        gmap.insert(orig, g.id);
                        db.groups().insert(&g)?;
                        out.added += 1;
                    }
                },
            }
            done.push(orig);
        }
        pending = rest;
    }

    // Keys.
    for mut k in inc.ssh_keys {
        let orig = k.meta.id;
        match local.ssh_keys.iter().find(|x| x.meta.id == orig) {
            Some(l) if *l == k => {
                out.skipped += 1;
                continue;
            }
            Some(_) if strategy == Strategy::Skip => {
                out.skipped += 1;
                continue;
            }
            Some(_) if strategy == Strategy::Overwrite => {
                db.keys().delete(orig)?;
                out.overwritten += 1;
            }
            Some(_) => {
                k.meta.id = KeyId::new();
                k.meta.label = suffix(&k.meta.label);
                kmap.insert(orig, k.meta.id);
                out.added += 1;
            }
            None => out.added += 1,
        }
        let Some(private) = secret(&k.private_key) else {
            return Err(crate::Error::Invalid(format!(
                "key '{}' has no private key in this backup",
                k.meta.label
            )));
        };
        db.keys()
            .insert(&k.meta, &private, secret(&k.passphrase).as_ref(), vault)?;
    }

    // Identities.
    for mut i in inc.identities {
        let orig = i.identity.id;
        if let Some(k) = i.identity.ssh_key_id {
            i.identity.ssh_key_id = Some(*kmap.get(&k).unwrap_or(&k));
        }
        match local.identities.iter().find(|x| x.identity.id == orig) {
            Some(l) if *l == i => out.skipped += 1,
            Some(_) if strategy == Strategy::Skip => out.skipped += 1,
            Some(_) if strategy == Strategy::Overwrite => {
                db.identities().update(&i.identity)?;
                if let Some(pw) = secret(&i.password) {
                    db.identities().set_password(orig, Some(&pw), vault)?;
                }
                out.overwritten += 1;
            }
            Some(_) => {
                i.identity.id = IdentityId::new();
                i.identity.label = suffix(&i.identity.label);
                imap.insert(orig, i.identity.id);
                db.identities()
                    .insert(&i.identity, secret(&i.password).as_ref(), vault)?;
                out.added += 1;
            }
            None => {
                db.identities()
                    .insert(&i.identity, secret(&i.password).as_ref(), vault)?;
                out.added += 1;
            }
        }
    }

    // Hosts: first without jump host, then link.
    let mut jumps: Vec<(HostId, HostId)> = Vec::new();
    for mut h in inc.hosts {
        let orig = h.id;
        if let Some(g) = h.group_id {
            h.group_id = Some(*gmap.get(&g).unwrap_or(&g));
        }
        if let Some(i) = h.identity_id {
            h.identity_id = Some(*imap.get(&i).unwrap_or(&i));
        }
        let jump = h.jump_host_id.take();
        let existing = local.hosts.iter().find(|x| x.id == orig);
        let mut link = true;
        match existing {
            Some(l)
                if {
                    let mut a = l.clone();
                    a.jump_host_id = None;
                    a == h
                } && l.jump_host_id == jump =>
            {
                out.skipped += 1;
                link = false;
            }
            Some(_) if strategy == Strategy::Skip => {
                out.skipped += 1;
                link = false;
            }
            Some(_) if strategy == Strategy::Overwrite => {
                db.hosts().update(&h)?;
                out.overwritten += 1;
            }
            Some(_) => {
                h.id = HostId::new();
                h.label = suffix(&h.label);
                hmap.insert(orig, h.id);
                db.hosts().insert(&h)?;
                out.added += 1;
            }
            None => {
                db.hosts().insert(&h)?;
                out.added += 1;
            }
        }
        if link && let Some(j) = jump {
            jumps.push((h.id, j));
        }
    }
    for (id, j) in jumps {
        let j = *hmap.get(&j).unwrap_or(&j);
        if let Some(mut h) = db.hosts().get(id)? {
            h.jump_host_id = Some(j);
            db.hosts().update(&h)?;
        }
    }

    // Tunnels.
    for mut t in inc.tunnels {
        let orig = t.id;
        t.host_id = *hmap.get(&t.host_id).unwrap_or(&t.host_id);
        match local.tunnels.iter().find(|x| x.id == orig) {
            Some(l) if *l == t => out.skipped += 1,
            Some(_) if strategy == Strategy::Skip => out.skipped += 1,
            Some(_) if strategy == Strategy::Overwrite => {
                db.tunnels().update(&t)?;
                out.overwritten += 1;
            }
            Some(_) => {
                t.id = TunnelId::new();
                t.label = suffix(&t.label);
                db.tunnels().insert(&t)?;
                out.added += 1;
            }
            None => {
                db.tunnels().insert(&t)?;
                out.added += 1;
            }
        }
    }

    // Known hosts (unique by host/port/type; KeepBoth cannot duplicate, so it acts as Skip).
    for mut k in inc.known_hosts {
        match db.known_hosts().find(&k.host, k.port, &k.key_type)? {
            None => {
                db.known_hosts().insert(&k)?;
                out.added += 1;
            }
            Some(l) if l.fingerprint == k.fingerprint && l.public_key == k.public_key => {
                out.skipped += 1
            }
            Some(l) if strategy == Strategy::Overwrite => {
                k.id = l.id;
                db.known_hosts().update(&k)?;
                out.overwritten += 1;
            }
            Some(_) => out.skipped += 1,
        }
    }

    // Settings.
    for (k, v) in inc.settings {
        match local.settings.iter().find(|x| x.0 == k) {
            None => {
                db.settings().set(&k, &v)?;
                out.added += 1;
            }
            Some(l) if l.1 == v => out.skipped += 1,
            Some(_) if strategy == Strategy::Overwrite => {
                db.settings().set(&k, &v)?;
                out.overwritten += 1;
            }
            Some(_) => out.skipped += 1,
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tern_core::{Group, Host, Identity, SshKeyMeta, Tunnel, TunnelKind};
    use tern_store::Store;
    use tern_vault::KdfParams;

    async fn store() -> Store {
        let s = Store::open_in_memory().unwrap();
        s.create_vault_with_params(
            SecretString::from("m".to_string()),
            KdfParams::new(8, 1, 1).unwrap(),
        )
        .await
        .unwrap();
        s
    }

    async fn populate(s: &Store) -> (HostId, HostId) {
        let g = Group {
            id: GroupId::new(),
            name: "prod".into(),
            parent_id: None,
            sort_order: 0,
        };
        s.insert_group(g.clone()).await.unwrap();
        let key = SshKeyMeta {
            id: KeyId::new(),
            label: "k".into(),
            key_type: "ssh-ed25519".into(),
            public_key: "ssh-ed25519 AAAA".into(),
            fingerprint: "SHA256:x".into(),
        };
        s.insert_key(key.clone(), SecretString::from("PRIV".to_string()), None)
            .await
            .unwrap();
        let ident = Identity {
            id: IdentityId::new(),
            label: "me".into(),
            username: "root".into(),
            ssh_key_id: Some(key.id),
        };
        s.insert_identity(ident.clone(), Some(SecretString::from("pw".to_string())))
            .await
            .unwrap();
        let mut jump = Host::new("jump", "j.example.com");
        jump.group_id = Some(g.id);
        let mut web = Host::new("web", "w.example.com");
        web.identity_id = Some(ident.id);
        web.jump_host_id = Some(jump.id);
        s.insert_host(jump.clone()).await.unwrap();
        s.insert_host(web.clone()).await.unwrap();
        let t = Tunnel {
            id: TunnelId::new(),
            label: "db".into(),
            host_id: web.id,
            kind: TunnelKind::Local,
            bind_addr: "127.0.0.1".into(),
            bind_port: 5432,
            target_host: Some("db".into()),
            target_port: Some(5432),
            auto_start: false,
        };
        s.insert_tunnel(t).await.unwrap();
        (jump.id, web.id)
    }

    #[tokio::test]
    async fn backup_restore_into_fresh_vault() {
        let a = store().await;
        let (_, web) = populate(&a).await;
        let payload = a
            .query(|db| snapshot(db, &Scope::default()).map_err(to_store))
            .await
            .unwrap();
        let bytes = crate::encrypt(&payload, &SecretString::from("x".to_string())).unwrap();
        let back = crate::decrypt(&bytes, &SecretString::from("x".to_string())).unwrap();

        let b = store().await;
        let applied = b
            .query(move |db| apply(db, back, Strategy::Skip).map_err(to_store))
            .await
            .unwrap();
        assert!(applied.added > 0);
        let again = b
            .query(|db| snapshot(db, &Scope::default()).map_err(to_store))
            .await
            .unwrap();
        assert_eq!(again, payload);
        assert!(
            again
                .hosts
                .iter()
                .any(|h| h.id == web && h.jump_host_id.is_some())
        );
        assert_eq!(plan(&payload, &again).new, 0);
    }

    #[tokio::test]
    async fn skip_twice_is_noop_and_keep_both_duplicates() {
        let a = store().await;
        populate(&a).await;
        let payload = a
            .query(|db| snapshot(db, &Scope::default()).map_err(to_store))
            .await
            .unwrap();
        let p2 = payload.clone();
        let r = a
            .query(move |db| apply(db, p2, Strategy::Skip).map_err(to_store))
            .await
            .unwrap();
        assert_eq!((r.added, r.overwritten), (0, 0));

        // Make one host conflict, then KeepBoth.
        let mut changed = payload.clone();
        changed.hosts[0].hostname = "other.example.com".into();
        let r = a
            .query(move |db| apply(db, changed, Strategy::KeepBoth).map_err(to_store))
            .await
            .unwrap();
        assert_eq!(r.added, 1);
        assert_eq!(a.list_hosts().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn failure_rolls_back() {
        let a = store().await;
        let mut p = Payload::default();
        p.hosts.push(Host::new("ok", "ok.example.com"));
        p.ssh_keys.push(KeyEntry {
            meta: SshKeyMeta {
                id: KeyId::new(),
                label: "nokey".into(),
                key_type: "t".into(),
                public_key: "p".into(),
                fingerprint: "f".into(),
            },
            private_key: None,
            passphrase: None,
        });
        // Keys are applied before hosts, so make the failure come after a write.
        p.groups.push(Group {
            id: GroupId::new(),
            name: "g".into(),
            parent_id: None,
            sort_order: 0,
        });
        let r = a
            .query(move |db| apply(db, p, Strategy::Skip).map_err(to_store))
            .await;
        assert!(r.is_err());
        assert!(a.list_hosts().await.unwrap().is_empty());
        assert!(a.list_groups().await.unwrap().is_empty());
    }

    fn to_store(e: crate::Error) -> tern_store::Error {
        match e {
            crate::Error::Store(s) => s,
            other => tern_store::Error::Corrupt(other.to_string()),
        }
    }
}
