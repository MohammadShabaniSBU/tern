//! Host add/edit form (S3-T02): pure model + view. Saving is wired in `app.rs`.

use iced::widget::{column, pick_list, row, space, text, text_input};
use iced::{Element, Fill};
use tern_core::{Host, HostId};

use crate::app::Message;
use crate::hosts_view::HostsData;
use crate::theme::tokens::*;

/// An entry of a picker; `id == None` is the "(none)" entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: Option<String>,
    pub label: String,
}

impl Choice {
    pub fn none() -> Self {
        Self {
            id: None,
            label: "(none)".into(),
        }
    }
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Label,
    Hostname,
    Port,
    Username,
    Tags,
    Keepalive,
    Notes,
}

impl Field {
    /// Widget ids are `hf-<n>`; the first is focused on open.
    pub const FIRST_ID: &'static str = "hf-label";
    fn id(self) -> &'static str {
        match self {
            Field::Label => Self::FIRST_ID,
            Field::Hostname => "hf-hostname",
            Field::Port => "hf-port",
            Field::Username => "hf-username",
            Field::Tags => "hf-tags",
            Field::Keepalive => "hf-keepalive",
            Field::Notes => "hf-notes",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Picker {
    Identity,
    Group,
    Jump,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fields {
    label: String,
    hostname: String,
    port: String,
    username: String,
    tags: String,
    keepalive: String,
    notes: String,
    identity: Choice,
    group: Choice,
    jump: Choice,
}

#[derive(Debug, Clone)]
pub struct HostForm {
    /// The stored host when editing; `None` when adding.
    editing: Option<Host>,
    fields: Fields,
    initial: Fields,
    identities: Vec<Choice>,
    groups: Vec<Choice>,
    jumps: Vec<Choice>,
    pub error: Option<String>,
    /// Set after the first `Esc` on a dirty form; a second one discards.
    discard_armed: bool,
}

fn choice_for(options: &[Choice], id: Option<String>) -> Choice {
    options
        .iter()
        .find(|c| c.id == id)
        .cloned()
        .unwrap_or_else(Choice::none)
}

fn parse_id<T: std::str::FromStr>(c: &Choice) -> Result<Option<T>, String> {
    c.id.as_deref()
        .map(|s| s.parse().map_err(|_| "invalid reference".to_owned()))
        .transpose()
}

impl HostForm {
    pub fn new(data: &HostsData, host: Option<&Host>, default_group: Option<String>) -> Self {
        let with_none = |it: Vec<Choice>| std::iter::once(Choice::none()).chain(it).collect();
        let identities: Vec<Choice> = with_none(
            data.identities
                .iter()
                .map(|i| Choice {
                    id: Some(i.id.to_string()),
                    label: format!("{} ({})", i.label, i.username),
                })
                .collect(),
        );
        let groups: Vec<Choice> = with_none(
            data.groups
                .iter()
                .map(|g| Choice {
                    id: Some(g.id.to_string()),
                    label: g.name.clone(),
                })
                .collect(),
        );
        let jumps: Vec<Choice> = with_none(
            data.hosts
                .iter()
                .filter(|h| Some(h.id) != host.map(|e| e.id))
                .map(|h| Choice {
                    id: Some(h.id.to_string()),
                    label: h.label.clone(),
                })
                .collect(),
        );
        let fields = match host {
            Some(h) => Fields {
                label: h.label.clone(),
                hostname: h.hostname.clone(),
                port: h.port.to_string(),
                username: h.username.clone().unwrap_or_default(),
                tags: h.tags.join(", "),
                keepalive: h.keepalive_secs.map(|k| k.to_string()).unwrap_or_default(),
                notes: h.notes.clone().unwrap_or_default(),
                identity: choice_for(&identities, h.identity_id.map(|i| i.to_string())),
                group: choice_for(&groups, h.group_id.map(|g| g.to_string())),
                jump: choice_for(&jumps, h.jump_host_id.map(|j| j.to_string())),
            },
            None => Fields {
                label: String::new(),
                hostname: String::new(),
                port: "22".into(),
                username: String::new(),
                tags: String::new(),
                keepalive: String::new(),
                notes: String::new(),
                identity: Choice::none(),
                group: choice_for(&groups, default_group),
                jump: Choice::none(),
            },
        };
        Self {
            editing: host.cloned(),
            initial: fields.clone(),
            fields,
            identities,
            groups,
            jumps,
            error: None,
            discard_armed: false,
        }
    }

    pub fn is_edit(&self) -> bool {
        self.editing.is_some()
    }

    pub fn is_dirty(&self) -> bool {
        self.fields != self.initial
    }

    pub fn set(&mut self, field: Field, value: String) {
        let f = &mut self.fields;
        match field {
            Field::Label => f.label = value,
            Field::Hostname => f.hostname = value,
            Field::Port => f.port = value,
            Field::Username => f.username = value,
            Field::Tags => f.tags = value,
            Field::Keepalive => f.keepalive = value,
            Field::Notes => f.notes = value,
        }
        self.error = None;
        self.discard_armed = false;
    }

    pub fn pick(&mut self, picker: Picker, choice: Choice) {
        match picker {
            Picker::Identity => self.fields.identity = choice,
            Picker::Group => self.fields.group = choice,
            Picker::Jump => self.fields.jump = choice,
        }
        self.error = None;
        self.discard_armed = false;
    }

    /// `Esc`: returns `true` if the form should close now. A dirty form needs a second `Esc`.
    pub fn request_close(&mut self) -> bool {
        if !self.is_dirty() || self.discard_armed {
            return true;
        }
        self.discard_armed = true;
        self.error = Some("Unsaved changes. Esc again to discard, Ctrl+S to save.".into());
        false
    }

    /// Validate and build the host to store.
    pub fn build(&self) -> Result<Host, String> {
        let f = &self.fields;
        let port: u16 = f
            .port
            .trim()
            .parse()
            .map_err(|_| "port must be a number between 1 and 65535".to_owned())?;
        let keepalive_secs = match f.keepalive.trim() {
            "" => None,
            s => Some(
                s.parse::<u32>()
                    .map_err(|_| "keepalive must be a number of seconds".to_owned())?,
            ),
        };
        let mut host = self.editing.clone().unwrap_or_else(|| Host {
            id: HostId::new(),
            ..Host::new("", "")
        });
        host.label = f.label.trim().to_owned();
        host.hostname = f.hostname.trim().to_owned();
        host.port = port;
        host.username = Some(f.username.trim().to_owned()).filter(|u| !u.is_empty());
        host.identity_id = parse_id(&f.identity)?;
        host.group_id = parse_id(&f.group)?;
        host.jump_host_id = parse_id(&f.jump)?;
        host.tags = f
            .tags
            .split(',')
            .map(|t| t.trim().trim_start_matches('#').to_owned())
            .filter(|t| !t.is_empty())
            .collect();
        host.notes = Some(f.notes.trim().to_owned()).filter(|n| !n.is_empty());
        host.keepalive_secs = keepalive_secs;
        host.validate().map_err(|e| e.to_string())?;
        Ok(host)
    }
}

pub fn view(form: &HostForm) -> Element<'_, Message> {
    let f = &form.fields;
    let input = |label: &'static str, field: Field, value: &str| {
        row![
            text(label).size(TEXT_BODY).width(100),
            text_input("", value)
                .style(crate::theme::style::input(false))
                .id(field.id())
                .on_input(move |v| Message::FormEdit(field, v))
                .on_submit(Message::FormSave)
                .size(TEXT_LABEL)
                .padding(SPACE_4),
        ]
        .spacing(SPACE_8)
        .align_y(iced::Center)
    };
    let picker = |label: &'static str, which: Picker, options: &[Choice], current: &Choice| {
        row![
            text(label).size(TEXT_BODY).width(100),
            pick_list(options.to_vec(), Some(current.clone()), move |c| {
                Message::FormPick(which, c)
            })
            .text_size(14)
            .width(Fill),
        ]
        .spacing(SPACE_8)
        .align_y(iced::Center)
    };
    let title = if form.is_edit() {
        "Edit host"
    } else {
        "New host"
    };
    let mut body = column![
        text(title).size(TEXT_TITLE),
        input("Label", Field::Label, &f.label),
        input("Hostname", Field::Hostname, &f.hostname),
        input("Port", Field::Port, &f.port),
        input("Username", Field::Username, &f.username),
        picker("Identity", Picker::Identity, &form.identities, &f.identity),
        picker("Group", Picker::Group, &form.groups, &f.group),
        picker("Jump host", Picker::Jump, &form.jumps, &f.jump),
        input("Tags", Field::Tags, &f.tags),
        input("Keepalive (s)", Field::Keepalive, &f.keepalive),
        input("Notes", Field::Notes, &f.notes),
    ]
    .spacing(SPACE_8);
    if let Some(e) = &form.error {
        body = body.push(
            text(e.as_str())
                .size(TEXT_BODY)
                .style(crate::theme::style::text_error),
        );
    }
    body = body
        .push(space().height(4))
        .push(text("Tab next field   Ctrl+S / Enter save   Esc cancel").size(TEXT_SMALL));
    crate::theme::widgets::dialog(body, 560.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_form() -> HostForm {
        HostForm::new(&HostsData::default(), None, None)
    }

    #[test]
    fn empty_hostname_and_bad_port_are_rejected_with_messages() {
        let mut f = new_form();
        f.set(Field::Label, "box".into());
        assert!(f.build().unwrap_err().contains("hostname"));
        f.set(Field::Hostname, "example.com".into());
        f.set(Field::Port, "0".into());
        assert!(f.build().unwrap_err().contains("port"));
        f.set(Field::Port, "abc".into());
        assert!(f.build().unwrap_err().contains("port"));
        f.set(Field::Port, "2222".into());
        assert_eq!(f.build().unwrap().port, 2222);
    }

    #[test]
    fn tags_and_optionals_are_normalised() {
        let mut f = new_form();
        f.set(Field::Label, " box ".into());
        f.set(Field::Hostname, "h".into());
        f.set(Field::Tags, "#db, prod,, ".into());
        f.set(Field::Keepalive, "30".into());
        let h = f.build().unwrap();
        assert_eq!(h.label, "box");
        assert_eq!(h.tags, ["db", "prod"]);
        assert_eq!(h.keepalive_secs, Some(30));
        assert_eq!(h.username, None);
        f.set(Field::Keepalive, "x".into());
        assert!(f.build().is_err());
    }

    #[test]
    fn editing_keeps_the_host_id() {
        let host = Host::new("a", "a.example.com");
        let mut f = HostForm::new(&HostsData::default(), Some(&host), None);
        f.set(Field::Label, "b".into());
        let built = f.build().unwrap();
        assert_eq!((built.id, built.label.as_str()), (host.id, "b"));
    }

    #[test]
    fn dirty_form_needs_two_escapes_clean_form_one() {
        let mut f = new_form();
        assert!(f.request_close());
        f.set(Field::Label, "x".into());
        assert!(!f.request_close());
        assert!(f.error.is_some());
        assert!(f.request_close());
        // Editing again disarms the discard.
        let mut f = new_form();
        f.set(Field::Label, "x".into());
        assert!(!f.request_close());
        f.set(Field::Label, "xy".into());
        assert!(!f.request_close());
    }

    #[test]
    fn reverting_to_the_initial_values_is_not_dirty() {
        let mut f = new_form();
        f.set(Field::Label, "x".into());
        f.set(Field::Label, String::new());
        assert!(!f.is_dirty());
    }
}
