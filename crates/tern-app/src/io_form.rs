//! One dialog for all Sprint 7 import/export flows (backup, plaintext, ssh_config, key scan,
//! scheduled-backup settings). Keyboard-first: text fields only, Enter / Ctrl+S to continue.

use iced::widget::{column, container, row, text, text_input};
use iced::{Color, Element, Fill, Theme};
use tern_io::bundle::Payload;
use tern_io::keyscan::KeyCandidate;
use tern_io::merge::Counts;
use tern_io::ssh_config::ParsedHost;

use crate::app::Message;
use crate::auth::Sensitive;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoKind {
    ExportBackup,
    ImportBackup,
    ExportPlain,
    ExportSshConfig,
    ImportSshConfig,
    ImportSshKeys,
    BackupSchedule,
    /// Asks only for the password, then writes the scheduled backup.
    AutoBackup,
}

#[derive(Debug, Clone)]
pub enum Preview {
    Backup {
        payload: Box<Payload>,
        counts: Counts,
    },
    SshConfig(Vec<ParsedHost>),
    Keys(Vec<KeyCandidate>),
}

pub const PATH_ID: &str = "io-path";
pub const PASSWORD_ID: &str = "io-password";

#[derive(Debug)]
pub struct IoForm {
    pub kind: IoKind,
    pub path: String,
    /// Optional group name limiting the export scope; for BackupSchedule: "on"/"off".
    pub scope: String,
    pub password: Sensitive,
    pub confirm: Sensitive,
    /// Plaintext export: must read `INCLUDE SECRETS` to include secrets.
    /// Preview stage: strategy (skip/overwrite/both) or selection (all / none / 1,3,5).
    /// BackupSchedule: number of backups to keep.
    pub extra: String,
    pub preview: Option<Preview>,
    pub error: Option<String>,
    pub busy: bool,
}

impl IoForm {
    pub fn new(kind: IoKind) -> Self {
        let path = match kind {
            IoKind::ExportBackup => "~/tern-backup.tern",
            IoKind::ImportBackup => "~/tern-backup.tern",
            IoKind::ExportPlain => "~/tern-export.json",
            IoKind::ExportSshConfig => "~/tern-ssh-config",
            IoKind::ImportSshConfig => "~/.ssh/config",
            IoKind::ImportSshKeys => "~/.ssh",
            IoKind::BackupSchedule | IoKind::AutoBackup => "",
        };
        Self {
            kind,
            path: path.into(),
            scope: String::new(),
            password: Sensitive::default(),
            confirm: Sensitive::default(),
            extra: String::new(),
            preview: None,
            error: None,
            busy: false,
        }
    }

    pub fn first_field(&self) -> &'static str {
        match self.kind {
            IoKind::AutoBackup => PASSWORD_ID,
            _ => PATH_ID,
        }
    }

    fn title(&self) -> &'static str {
        match self.kind {
            IoKind::ExportBackup => "Export encrypted backup",
            IoKind::ImportBackup => "Import backup",
            IoKind::ExportPlain => "Export plaintext JSON",
            IoKind::ExportSshConfig => "Export ssh_config snippet",
            IoKind::ImportSshConfig => "Import ~/.ssh/config",
            IoKind::ImportSshKeys => "Import keys from a folder",
            IoKind::BackupSchedule => "Scheduled backups",
            IoKind::AutoBackup => "Scheduled backup",
        }
    }
}

/// Parse a selection like `all`, `none` or `1,3,5-7` into zero-based indexes below `len`.
pub fn parse_selection(s: &str, len: usize) -> Result<Vec<usize>, String> {
    let s = s.trim().to_ascii_lowercase();
    if s.is_empty() || s == "all" {
        return Ok((0..len).collect());
    }
    if s == "none" {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (part, part),
        };
        let (a, b): (usize, usize) = match (a.parse(), b.parse()) {
            (Ok(a), Ok(b)) => (a, b),
            _ => return Err(format!("bad selection '{part}'")),
        };
        if a == 0 || b < a || b > len {
            return Err(format!("selection '{part}' is out of range 1..{len}"));
        }
        out.extend((a - 1)..b);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn field<'a>(label: &'static str, input: Element<'a, Message>) -> Element<'a, Message> {
    row![text(label).size(13).width(120), input]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

fn input<'a>(
    placeholder: &str,
    value: &str,
    on_input: fn(String) -> Message,
) -> text_input::TextInput<'a, Message> {
    text_input(placeholder, value)
        .on_input(on_input)
        .on_submit(Message::IoSubmit)
        .padding(4)
}

fn secret<'a>(
    placeholder: &str,
    value: &Sensitive,
    on_input: fn(Sensitive) -> Message,
) -> text_input::TextInput<'a, Message> {
    text_input(placeholder, value.as_str())
        .secure(true)
        .on_input(move |v| on_input(Sensitive::new(v)))
        .on_submit(Message::IoSubmit)
        .padding(4)
}

pub fn view(form: &IoForm) -> Element<'_, Message> {
    let mut body = column![text(form.title()).size(18)].spacing(8);
    let hint;
    match (&form.preview, form.kind) {
        (Some(Preview::Backup { counts, .. }), _) => {
            body = body.push(
                text(format!(
                    "{} new, {} conflicting, {} identical",
                    counts.new, counts.conflicting, counts.identical
                ))
                .size(14),
            );
            body = body.push(field(
                "On conflict",
                input("skip | overwrite | both", &form.extra, Message::IoExtra)
                    .id(PATH_ID)
                    .into(),
            ));
            hint = "Skip keeps yours, overwrite replaces yours, both keeps copies ('(imported)'). All or nothing.";
        }
        (Some(Preview::SshConfig(hosts)), _) => {
            for (i, h) in hosts.iter().enumerate().take(200) {
                let line = format!(
                    "{:>3}. {}  →  {}{}",
                    i + 1,
                    h.alias,
                    h.hostname.as_deref().unwrap_or(&h.alias),
                    h.proxy_jump
                        .as_deref()
                        .map(|j| format!("  via {j}"))
                        .unwrap_or_default()
                );
                body = body.push(text(line).size(13).font(crate::app::MONO));
            }
            body = body.push(field(
                "Import",
                input("all | none | 1,3,5-7", &form.extra, Message::IoExtra)
                    .id(PATH_ID)
                    .into(),
            ));
            body = body.push(field(
                "Key passphrase",
                secret(
                    "(for encrypted IdentityFiles)",
                    &form.password,
                    Message::IoPassword,
                )
                .into(),
            ));
            hint = "Referenced IdentityFiles are imported as keys + identities.";
        }
        (Some(Preview::Keys(keys)), _) => {
            for (i, k) in keys.iter().enumerate() {
                body = body.push(
                    text(format!("{:>3}. {}", i + 1, k.name))
                        .size(13)
                        .font(crate::app::MONO),
                );
            }
            body = body.push(field(
                "Import",
                input("all | none | 1,3,5-7", &form.extra, Message::IoExtra)
                    .id(PATH_ID)
                    .into(),
            ));
            body = body.push(field(
                "Passphrase",
                secret("(for encrypted keys)", &form.password, Message::IoPassword).into(),
            ));
            hint = "Encrypted keys need their passphrase; those that fail are reported.";
        }
        (None, kind) => {
            match kind {
                IoKind::AutoBackup => {
                    body = body.push(
                        text("Enter the backup password to write today's scheduled backup.")
                            .size(13),
                    );
                    body = body.push(field(
                        "Password",
                        secret("backup password", &form.password, Message::IoPassword)
                            .id(PASSWORD_ID)
                            .into(),
                    ));
                }
                IoKind::BackupSchedule => {
                    body = body.push(field(
                        "Folder",
                        input("/path/to/synced/folder", &form.path, Message::IoPath)
                            .id(PATH_ID)
                            .into(),
                    ));
                    body = body.push(field(
                        "Enabled",
                        input("on | off", &form.scope, Message::IoScope).into(),
                    ));
                    body = body.push(field(
                        "Keep last",
                        input("5", &form.extra, Message::IoExtra).into(),
                    ));
                }
                _ => {
                    body = body.push(field(
                        if matches!(kind, IoKind::ImportSshKeys) {
                            "Folder"
                        } else {
                            "File"
                        },
                        input("path", &form.path, Message::IoPath)
                            .id(PATH_ID)
                            .into(),
                    ));
                    if matches!(
                        kind,
                        IoKind::ExportBackup | IoKind::ExportPlain | IoKind::ExportSshConfig
                    ) {
                        body = body.push(field(
                            "Group",
                            input("(blank = everything)", &form.scope, Message::IoScope).into(),
                        ));
                    }
                    if matches!(kind, IoKind::ExportBackup | IoKind::ImportBackup) {
                        body = body.push(field(
                            "Password",
                            secret("backup password", &form.password, Message::IoPassword).into(),
                        ));
                    }
                    if kind == IoKind::ExportBackup {
                        body = body.push(field(
                            "Confirm",
                            secret("repeat password", &form.confirm, Message::IoConfirm).into(),
                        ));
                    }
                    if kind == IoKind::ExportPlain {
                        body = body.push(
                            text("WARNING: plaintext secrets are readable by anyone with the file. Leave blank to exclude secrets, or type INCLUDE SECRETS.")
                                .size(12)
                                .style(text::danger),
                        );
                        body = body.push(field(
                            "Secrets",
                            input("INCLUDE SECRETS", &form.extra, Message::IoExtra).into(),
                        ));
                    }
                }
            }
            hint = match kind {
                IoKind::ExportBackup => {
                    "The backup password is separate from your master password."
                }
                IoKind::BackupSchedule => {
                    "A backup is written after each unlock; older ones beyond 'keep' are removed."
                }
                _ => "",
            };
        }
    }
    if form.busy {
        body = body.push(text("Working…").size(13));
    }
    if let Some(e) = &form.error {
        body = body.push(text(e.as_str()).size(13).style(text::danger));
    }
    if !hint.is_empty() {
        body = body.push(text(hint).size(12));
    }
    body = body.push(text("Enter / Ctrl+S continue   Tab next field   Esc cancel").size(12));
    let dialog = container(body)
        .padding(20)
        .max_width(640)
        .style(|theme: &Theme| {
            let mut s = container::bordered_box(theme);
            s.background = Some(theme.extended_palette().background.base.color.into());
            s
        });
    container(dialog)
        .center(Fill)
        .style(|_| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selections() {
        assert_eq!(parse_selection("", 3).unwrap(), vec![0, 1, 2]);
        assert_eq!(parse_selection("none", 3).unwrap(), Vec::<usize>::new());
        assert_eq!(parse_selection("1, 3", 3).unwrap(), vec![0, 2]);
        assert_eq!(parse_selection("2-3,3", 5).unwrap(), vec![1, 2]);
        assert!(parse_selection("0", 3).is_err());
        assert!(parse_selection("4", 3).is_err());
        assert!(parse_selection("x", 3).is_err());
    }
}
