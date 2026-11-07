//! Sprint 7 flows driven by [`IoForm`]: backup export/import, plaintext export, ssh_config
//! import/export, key scan and scheduled backups. Child module of `app` so it can reach `State`.

use std::collections::HashMap;
use std::path::PathBuf;

use iced::Task;
use secrecy::SecretString;
use tern_core::{HostAuth, HostId, KeyId, SshKeyMeta};
use tern_io::bundle::Payload;
use tern_io::merge::{self, Scope, Strategy};
use tern_store::Store;

use super::{Message, Mode, State, ToastKind, expand_home, write_private_file};
use crate::io_form::{self, IoForm, IoKind, Preview};

fn to_store(e: tern_io::Error) -> tern_store::Error {
    match e {
        tern_io::Error::Store(s) => s,
        other => tern_store::Error::Corrupt(other.to_string()),
    }
}

fn write_out(path: &str, bytes: Vec<u8>) -> Result<PathBuf, String> {
    let p = expand_home(path.trim());
    let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    write_private_file(&p, &text).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            format!("{} already exists; choose another path", p.display())
        } else {
            format!("cannot write {}: {e}", p.display())
        }
    })?;
    Ok(p)
}

async fn export_payload(store: &Store, group: String) -> Result<Payload, String> {
    store
        .query(move |db| {
            let group = group.trim().to_owned();
            let scope = if group.is_empty() {
                Scope::default()
            } else {
                let ids: Vec<_> = db
                    .groups()
                    .list()?
                    .into_iter()
                    .filter(|g| g.name.eq_ignore_ascii_case(&group))
                    .map(|g| g.id)
                    .collect();
                if ids.is_empty() {
                    return Err(tern_store::Error::Corrupt(format!(
                        "no group named '{group}'"
                    )));
                }
                Scope { groups: Some(ids) }
            };
            merge::snapshot(db, &scope).map_err(to_store)
        })
        .await
        .map_err(|e| match e {
            tern_store::Error::Corrupt(m) => m,
            other => other.to_string(),
        })
}

async fn encrypt_to(payload: Payload, password: SecretString) -> Result<Vec<u8>, String> {
    tokio::task::spawn_blocking(move || tern_io::encrypt(&payload, &password))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

fn counts_text(p: &Payload) -> String {
    format!(
        "{} hosts, {} keys, {} tunnels",
        p.hosts.len(),
        p.ssh_keys.len(),
        p.tunnels.len()
    )
}

async fn setting<T: serde::de::DeserializeOwned>(store: &Store, key: &'static str) -> Option<T> {
    let v = store
        .query(move |db| db.settings().get(key))
        .await
        .ok()
        .flatten()?;
    serde_json::from_str(&v).ok()
}

/// True when scheduled backups are enabled and a folder is configured.
pub(super) async fn auto_backup_due(store: &Store) -> bool {
    setting::<bool>(store, "backup.enabled").await == Some(true)
        && setting::<String>(store, "backup.dir")
            .await
            .is_some_and(|d| !d.trim().is_empty())
}

async fn write_scheduled(store: &Store, password: SecretString) -> Result<String, String> {
    let dir: String = setting(store, "backup.dir")
        .await
        .ok_or("no backup folder configured")?;
    let keep: u32 = setting(store, "backup.keep").await.unwrap_or(5).max(1);
    let payload = export_payload(store, String::new()).await?;
    let bytes = encrypt_to(payload, password).await?;
    let dir = expand_home(dir.trim());
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create folder: {e}"))?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let stamp: String = tern_io::bundle::rfc3339(secs)
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T')
        .collect();
    let path = dir.join(format!("tern-backup-{stamp}.tern"));
    let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    write_private_file(&path, &text).map_err(|e| format!("cannot write backup: {e}"))?;
    // Prune: newest `keep` survive (names sort chronologically).
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("tern-backup-") && n.ends_with(".tern"))
        })
        .collect();
    files.sort();
    let excess = files.len().saturating_sub(keep as usize);
    for old in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(old);
    }
    Ok(format!("Scheduled backup written to {}", path.display()))
}

impl State {
    pub(super) fn open_io(&mut self, kind: IoKind) -> Task<Message> {
        let form = IoForm::new(kind);
        let first = form.first_field();
        self.io_form = Some(form);
        self.mode = Mode::Insert;
        self.matcher.reset();
        if kind == IoKind::BackupSchedule {
            let store = self.store.clone();
            return Task::batch([
                iced::widget::operation::focus(first),
                Task::perform(
                    async move {
                        let dir: String = setting(&store, "backup.dir").await.unwrap_or_default();
                        let on: bool = setting(&store, "backup.enabled").await.unwrap_or(false);
                        let keep: u32 = setting(&store, "backup.keep").await.unwrap_or(5);
                        (dir, on, keep)
                    },
                    |(d, o, k)| Message::IoScheduleLoaded(d, o, k),
                ),
            ]);
        }
        iced::widget::operation::focus(first)
    }

    pub(super) fn edit_io(&mut self, f: impl FnOnce(&mut IoForm)) -> Task<Message> {
        if let Some(form) = &mut self.io_form {
            f(form);
            form.error = None;
        }
        Task::none()
    }

    pub(super) fn io_previewed(&mut self, r: Result<Preview, String>) -> Task<Message> {
        let Some(form) = &mut self.io_form else {
            return Task::none();
        };
        form.busy = false;
        match r {
            Ok(p) => {
                form.extra = match p {
                    Preview::Backup { .. } => "skip".into(),
                    _ => "all".into(),
                };
                form.preview = Some(p);
                form.password.clear();
                iced::widget::operation::focus(io_form::PATH_ID)
            }
            Err(e) => {
                form.error = Some(e);
                Task::none()
            }
        }
    }

    pub(super) fn io_done(&mut self, r: Result<String, String>) -> Task<Message> {
        match r {
            Ok(msg) => {
                self.io_form = None;
                self.mode = Mode::Normal;
                self.toast(ToastKind::Success, msg);
                Task::batch([
                    self.reload_hosts(),
                    self.reload_keys(),
                    self.reload_tunnels(),
                ])
            }
            Err(e) => {
                if let Some(f) = &mut self.io_form {
                    f.busy = false;
                    f.error = Some(e);
                }
                Task::none()
            }
        }
    }

    pub(super) fn submit_io(&mut self) -> Task<Message> {
        let Some(form) = &mut self.io_form else {
            return Task::none();
        };
        if form.busy {
            return Task::none();
        }
        let kind = form.kind;
        let path = form.path.trim().to_owned();
        let scope = form.scope.trim().to_owned();
        let store = self.store.clone();

        // Second stage (preview confirmed).
        if let Some(preview) = form.preview.clone() {
            let extra = form.extra.clone();
            let passphrase =
                (!form.password.is_empty()).then(|| form.password.clone().take_secret());
            return match preview {
                Preview::Backup { payload, .. } => {
                    let strategy = match extra.trim().to_ascii_lowercase().as_str() {
                        "" | "skip" | "s" => Strategy::Skip,
                        "overwrite" | "o" => Strategy::Overwrite,
                        "both" | "keep both" | "b" => Strategy::KeepBoth,
                        _ => {
                            form.error = Some("choose skip, overwrite or both".into());
                            return Task::none();
                        }
                    };
                    form.busy = true;
                    Task::perform(
                        async move {
                            let a = store
                                .query(move |db| {
                                    merge::apply(db, *payload, strategy).map_err(to_store)
                                })
                                .await
                                .map_err(|e| match e {
                                    tern_store::Error::Corrupt(m) => m,
                                    o => o.to_string(),
                                })?;
                            Ok(format!(
                                "Imported: {} added, {} overwritten, {} unchanged",
                                a.added, a.overwritten, a.skipped
                            ))
                        },
                        Message::IoDone,
                    )
                }
                Preview::SshConfig(hosts) => {
                    let sel = match io_form::parse_selection(&extra, hosts.len()) {
                        Ok(s) => s,
                        Err(e) => {
                            form.error = Some(e);
                            return Task::none();
                        }
                    };
                    form.busy = true;
                    let chosen: Vec<_> = sel.into_iter().map(|i| hosts[i].clone()).collect();
                    Task::perform(import_ssh_hosts(store, chosen, passphrase), Message::IoDone)
                }
                Preview::Keys(keys) => {
                    let sel = match io_form::parse_selection(&extra, keys.len()) {
                        Ok(s) => s,
                        Err(e) => {
                            form.error = Some(e);
                            return Task::none();
                        }
                    };
                    form.busy = true;
                    let chosen: Vec<_> = sel.into_iter().map(|i| keys[i].clone()).collect();
                    Task::perform(import_key_files(store, chosen, passphrase), Message::IoDone)
                }
            };
        }

        match kind {
            IoKind::ExportBackup => {
                if path.is_empty() {
                    form.error = Some("enter a file path".into());
                    return Task::none();
                }
                if form.password.is_empty() {
                    form.error = Some("enter a backup password".into());
                    return Task::none();
                }
                if form.password.as_str() != form.confirm.as_str() {
                    form.error = Some("passwords do not match".into());
                    return Task::none();
                }
                let pw = form.password.clone().take_secret();
                form.busy = true;
                Task::perform(
                    async move {
                        let payload = export_payload(&store, scope).await?;
                        let summary = counts_text(&payload);
                        let bytes = encrypt_to(payload, pw).await?;
                        let p = write_out(&path, bytes)?;
                        Ok(format!("Exported {summary} to {}", p.display()))
                    },
                    Message::IoDone,
                )
            }
            IoKind::ImportBackup => {
                if path.is_empty() || form.password.is_empty() {
                    form.error = Some("enter the file path and its password".into());
                    return Task::none();
                }
                let pw = form.password.clone().take_secret();
                form.busy = true;
                Task::perform(
                    async move {
                        let file = expand_home(&path);
                        let payload = tokio::task::spawn_blocking(move || {
                            let bytes = std::fs::read(&file)
                                .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
                            tern_io::decrypt(&bytes, &pw).map_err(|e| e.to_string())
                        })
                        .await
                        .map_err(|e| e.to_string())??;
                        let incoming = payload.clone();
                        let counts = store
                            .query(move |db| {
                                let local =
                                    merge::snapshot(db, &Scope::default()).map_err(to_store)?;
                                Ok(merge::plan(&incoming, &local))
                            })
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(Preview::Backup {
                            payload: Box::new(payload),
                            counts,
                        })
                    },
                    Message::IoPreviewed,
                )
            }
            IoKind::ExportPlain => {
                if path.is_empty() {
                    form.error = Some("enter a file path".into());
                    return Task::none();
                }
                let typed = form.extra.trim();
                let include = typed == "INCLUDE SECRETS";
                if !typed.is_empty() && !include {
                    form.error = Some("type exactly INCLUDE SECRETS, or leave blank".into());
                    return Task::none();
                }
                form.busy = true;
                Task::perform(
                    async move {
                        let payload = export_payload(&store, scope).await?;
                        let summary = counts_text(&payload);
                        let bytes =
                            tern_io::plain::to_json(payload, include).map_err(|e| e.to_string())?;
                        let p = write_out(&path, bytes)?;
                        Ok(format!(
                            "Exported {summary} to {} ({})",
                            p.display(),
                            if include {
                                "WITH secrets"
                            } else {
                                "no secrets"
                            }
                        ))
                    },
                    Message::IoDone,
                )
            }
            IoKind::ExportSshConfig => {
                if path.is_empty() {
                    form.error = Some("enter a file path".into());
                    return Task::none();
                }
                form.busy = true;
                Task::perform(
                    async move {
                        let p = export_payload(&store, scope).await?;
                        let all = store.list_hosts().await.map_err(|e| e.to_string())?;
                        let text = tern_io::ssh_config::export(&p.hosts, &all, &p.tunnels);
                        let out = write_out(&path, text.into_bytes())?;
                        Ok(format!(
                            "Wrote {} hosts to {}",
                            p.hosts.len(),
                            out.display()
                        ))
                    },
                    Message::IoDone,
                )
            }
            IoKind::ImportSshConfig => {
                form.busy = true;
                Task::perform(
                    async move {
                        let file = expand_home(&path);
                        let home = directories::UserDirs::new()
                            .map(|d| d.home_dir().to_path_buf())
                            .unwrap_or_default();
                        let hosts = tokio::task::spawn_blocking(move || {
                            tern_io::ssh_config::parse_file(&file, &home)
                        })
                        .await
                        .map_err(|e| e.to_string())?;
                        if hosts.is_empty() {
                            return Err("no importable hosts found (wildcards are skipped)".into());
                        }
                        Ok(Preview::SshConfig(hosts))
                    },
                    Message::IoPreviewed,
                )
            }
            IoKind::ImportSshKeys => {
                form.busy = true;
                Task::perform(
                    async move {
                        let dir = expand_home(&path);
                        let keys =
                            tokio::task::spawn_blocking(move || tern_io::keyscan::scan(&dir))
                                .await
                                .map_err(|e| e.to_string())?;
                        if keys.is_empty() {
                            return Err("no private keys found there".into());
                        }
                        Ok(Preview::Keys(keys))
                    },
                    Message::IoPreviewed,
                )
            }
            IoKind::BackupSchedule => {
                let on = matches!(scope.to_ascii_lowercase().as_str(), "on" | "yes" | "true");
                let keep: u32 = match form.extra.trim() {
                    "" => 5,
                    s => match s.parse() {
                        Ok(n) if n >= 1 => n,
                        _ => {
                            form.error = Some("'keep' must be a number ≥ 1".into());
                            return Task::none();
                        }
                    },
                };
                if on && path.is_empty() {
                    form.error = Some("choose a folder".into());
                    return Task::none();
                }
                form.busy = true;
                Task::perform(
                    async move {
                        let dir = serde_json::to_string(&path).map_err(|e| e.to_string())?;
                        store
                            .query(move |db| {
                                let s = db.settings();
                                s.set("backup.dir", &dir)?;
                                s.set("backup.keep", &keep.to_string())?;
                                s.set("backup.enabled", if on { "true" } else { "false" })
                            })
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(if on {
                            "Scheduled backups on".to_string()
                        } else {
                            "Scheduled backups off".to_string()
                        })
                    },
                    Message::IoDone,
                )
            }
            IoKind::AutoBackup => {
                if form.password.is_empty() {
                    form.error = Some("enter the backup password (Esc skips)".into());
                    return Task::none();
                }
                let pw = form.password.clone().take_secret();
                form.busy = true;
                Task::perform(
                    async move { write_scheduled(&store, pw).await },
                    Message::IoDone,
                )
            }
        }
    }
}

/// Import selected ssh_config entries (hosts, tunnels, jump hosts, IdentityFiles as keys) atomically.
async fn import_ssh_hosts(
    store: Store,
    chosen: Vec<tern_io::ssh_config::ParsedHost>,
    passphrase: Option<SecretString>,
) -> Result<String, String> {
    let home = directories::UserDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_default();
    let existing: HashMap<String, HostId> = store
        .list_hosts()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|h| (h.label, h.id))
        .collect();
    let built = tern_io::ssh_config::build(&chosen, &existing, &home);
    let mut warnings = built.warnings.clone();

    // Import each distinct IdentityFile once.
    let mut by_path: HashMap<PathBuf, (SshKeyMeta, tern_ssh::keys::KeyMaterial)> = HashMap::new();
    for (_, path) in &built.key_files {
        if by_path.contains_key(path) {
            continue;
        }
        let (p, pass) = (path.clone(), passphrase.clone());
        let res = tokio::task::spawn_blocking(move || {
            let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            tern_ssh::keys::import(&text, None, "", pass.as_ref()).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?;
        match res {
            Ok(m) => {
                let label = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "key".into());
                let meta = SshKeyMeta {
                    id: KeyId::new(),
                    label,
                    key_type: m.key_type.clone(),
                    public_key: m.public_key.clone(),
                    fingerprint: m.fingerprint.clone(),
                };
                by_path.insert(path.clone(), (meta, m));
            }
            Err(e) => warnings.push(format!("{}: {e}", path.display())),
        }
    }
    let host_key: HashMap<HostId, PathBuf> = built.key_files.iter().cloned().collect();
    let n_hosts = built.hosts.len();
    let n_tunnels = built.tunnels.len();
    let n_keys = by_path.len();

    store
        .query(move |db| {
            let vault = db.vault()?;
            let tx = db.conn().unchecked_transaction()?;
            let mut key_for: HashMap<PathBuf, KeyId> = HashMap::new();
            let mut hosts = built.hosts;
            for (path, (meta, material)) in by_path {
                db.keys().insert(
                    &meta,
                    &material.private_pem,
                    material.passphrase.as_ref(),
                    vault,
                )?;
                key_for.insert(path, meta.id);
            }
            let mut jumps = Vec::new();
            for h in &mut hosts {
                if let Some(k) = host_key.get(&h.id).and_then(|p| key_for.get(p)) {
                    h.auth = HostAuth::Key(*k);
                    h.username.get_or_insert_with(|| "root".into());
                }
                if let Some(j) = h.jump_host_id.take() {
                    jumps.push((h.id, j));
                }
                db.hosts().insert(h)?;
            }
            for (id, j) in jumps {
                if let Some(h) = hosts.iter_mut().find(|h| h.id == id) {
                    h.jump_host_id = Some(j);
                    db.hosts().update(h)?;
                }
            }
            for t in &built.tunnels {
                db.tunnels().insert(t)?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
        .map_err(|e| e.to_string())?;

    let mut msg = format!("Imported {n_hosts} hosts, {n_tunnels} tunnels, {n_keys} keys");
    if !warnings.is_empty() {
        msg.push_str(&format!(" ({} warnings: {})", warnings.len(), warnings[0]));
    }
    Ok(msg)
}

/// Import selected private-key files as stored keys.
async fn import_key_files(
    store: Store,
    chosen: Vec<tern_io::keyscan::KeyCandidate>,
    passphrase: Option<SecretString>,
) -> Result<String, String> {
    let known: Vec<String> = store
        .list_keys()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|k| k.fingerprint)
        .collect();
    let (mut ok, mut dup) = (0, 0);
    let mut failed: Vec<String> = Vec::new();
    for c in chosen {
        let (p, pass) = (c.path.clone(), passphrase.clone());
        let res = tokio::task::spawn_blocking(move || {
            let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            tern_ssh::keys::import(&text, None, "", pass.as_ref()).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?;
        match res {
            Ok(m) if known.contains(&m.fingerprint) => {
                dup += 1;
                drop(m);
            }
            Ok(m) => {
                let meta = SshKeyMeta {
                    id: KeyId::new(),
                    label: c.name.clone(),
                    key_type: m.key_type,
                    public_key: m.public_key,
                    fingerprint: m.fingerprint,
                };
                match store.insert_key(meta, m.private_pem, m.passphrase).await {
                    Ok(()) => ok += 1,
                    Err(e) => failed.push(format!("{}: {e}", c.name)),
                }
            }
            Err(e) => failed.push(format!("{}: {e}", c.name)),
        }
    }
    let mut msg = format!("Imported {ok} keys");
    if dup > 0 {
        msg.push_str(&format!(", {dup} already present"));
    }
    if !failed.is_empty() {
        msg.push_str(&format!(", {} failed ({})", failed.len(), failed[0]));
    }
    if ok == 0 && !failed.is_empty() {
        return Err(msg);
    }
    Ok(msg)
}
