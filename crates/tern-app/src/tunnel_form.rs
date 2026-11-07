//! Tunnel add/edit form (S6-T01): pure model + view. Saving is wired in `app.rs`.

use iced::Element;
use iced::widget::{checkbox, column, row, space, text, text_input};
use tern_core::{Host, HostId, Tunnel, TunnelId, TunnelKind};

use crate::app::Message;
use crate::host_form::Choice;
use crate::select::{self, SelectSpec};
use crate::theme::tokens::*;
use crate::tunnels_view::kind_label;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TField {
    Label,
    BindAddr,
    BindPort,
    TargetHost,
    TargetPort,
}

pub const FIRST_ID: &str = "tf-label";

impl TField {
    fn id(self) -> &'static str {
        match self {
            TField::Label => FIRST_ID,
            TField::BindAddr => "tf-bind-addr",
            TField::BindPort => "tf-bind-port",
            TField::TargetHost => "tf-target-host",
            TField::TargetPort => "tf-target-port",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fields {
    label: String,
    host: Option<String>,
    kind: TunnelKind,
    bind_addr: String,
    bind_port: String,
    target_host: String,
    target_port: String,
    auto_start: bool,
}

#[derive(Debug, Clone)]
pub struct TunnelForm {
    editing: Option<Tunnel>,
    fields: Fields,
    initial: Fields,
    hosts: Vec<Choice>,
    kinds: Vec<Choice>,
    pub error: Option<String>,
    discard_armed: bool,
}

impl TunnelForm {
    pub fn new(hosts: &[Host], tunnel: Option<&Tunnel>) -> Self {
        let fields = match tunnel {
            Some(t) => Fields {
                label: t.label.clone(),
                host: Some(t.host_id.to_string()),
                kind: t.kind,
                bind_addr: t.bind_addr.clone(),
                bind_port: t.bind_port.to_string(),
                target_host: t.target_host.clone().unwrap_or_default(),
                target_port: t.target_port.map(|p| p.to_string()).unwrap_or_default(),
                auto_start: t.auto_start,
            },
            None => Fields {
                label: String::new(),
                host: hosts.first().map(|h| h.id.to_string()),
                kind: TunnelKind::Local,
                bind_addr: "127.0.0.1".into(),
                bind_port: String::new(),
                target_host: String::new(),
                target_port: String::new(),
                auto_start: false,
            },
        };
        Self {
            editing: tunnel.cloned(),
            initial: fields.clone(),
            fields,
            hosts: hosts
                .iter()
                .map(|h| Choice {
                    id: Some(h.id.to_string()),
                    label: h.label.clone(),
                })
                .collect(),
            kinds: [TunnelKind::Local, TunnelKind::Remote, TunnelKind::Dynamic]
                .into_iter()
                .map(|k| Choice {
                    id: Some(k.as_str().to_owned()),
                    label: kind_label(k).to_owned(),
                })
                .collect(),
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

    fn touched(&mut self) {
        self.error = None;
        self.discard_armed = false;
    }

    pub fn set(&mut self, field: TField, value: String) {
        let f = &mut self.fields;
        match field {
            TField::Label => f.label = value,
            TField::BindAddr => f.bind_addr = value,
            TField::BindPort => f.bind_port = value,
            TField::TargetHost => f.target_host = value,
            TField::TargetPort => f.target_port = value,
        }
        self.touched();
    }

    pub fn pick_host(&mut self, c: Choice) {
        self.fields.host = c.id;
        self.touched();
    }

    pub fn pick_kind(&mut self, c: Choice) {
        if let Some(k) = c.id.as_deref().and_then(TunnelKind::parse) {
            self.fields.kind = k;
        }
        self.touched();
    }

    pub fn set_auto_start(&mut self, on: bool) {
        self.fields.auto_start = on;
        self.touched();
    }

    /// `Esc`: `true` if the form should close now (a dirty form needs a second `Esc`).
    pub fn request_close(&mut self) -> bool {
        if !self.is_dirty() || self.discard_armed {
            return true;
        }
        self.discard_armed = true;
        self.error = Some("Unsaved changes. Esc again to discard, Ctrl+S to save.".into());
        false
    }

    pub fn build(&self) -> Result<Tunnel, String> {
        let f = &self.fields;
        let port = |s: &str, what: &str| {
            s.trim()
                .parse::<u16>()
                .map_err(|_| format!("{what} must be a number between 1 and 65535"))
        };
        let host_id: HostId = f
            .host
            .as_deref()
            .ok_or_else(|| "choose a host".to_owned())?
            .parse()
            .map_err(|_| "invalid host".to_owned())?;
        let bind_port = port(&f.bind_port, "bind port")?;
        let dynamic = f.kind == TunnelKind::Dynamic;
        let (target_host, target_port) = if dynamic {
            (None, None)
        } else {
            (
                Some(f.target_host.trim().to_owned()),
                Some(port(&f.target_port, "target port")?),
            )
        };
        let t = Tunnel {
            id: self.editing.as_ref().map_or_else(TunnelId::new, |t| t.id),
            label: f.label.trim().to_owned(),
            host_id,
            kind: f.kind,
            bind_addr: f.bind_addr.trim().to_owned(),
            bind_port,
            target_host,
            target_port,
            auto_start: f.auto_start,
        };
        t.validate().map_err(|e| e.to_string())?;
        Ok(t)
    }
}

pub fn view(form: &TunnelForm) -> Element<'_, Message> {
    let f = &form.fields;
    let input = |label: &'static str, field: TField, value: &str| {
        row![
            text(label).size(TEXT_BODY).width(100),
            text_input("", value)
                .style(crate::theme::style::input(false))
                .id(field.id())
                .on_input(move |v| Message::TunnelEdit(field, v))
                .on_submit(Message::TunnelSave)
                .size(TEXT_LABEL)
                .padding(SPACE_4),
        ]
        .spacing(SPACE_8)
        .align_y(iced::Center)
    };
    let choose = |id: &'static str,
                  options: &[Choice],
                  current: &Option<String>,
                  pick: fn(Choice) -> Message| {
        select::view(SelectSpec {
            id,
            labels: options.iter().map(|c| c.label.clone()).collect(),
            current: options.iter().position(|c| &c.id == current).unwrap_or(0),
            picks: options.iter().map(|c| pick(c.clone())).collect(),
        })
    };
    let title = if form.is_edit() {
        "Edit tunnel"
    } else {
        "New tunnel"
    };
    let kind_id = Some(f.kind.as_str().to_owned());
    let mut body = column![
        text(title).size(TEXT_TITLE),
        input("Label", TField::Label, &f.label),
        row![
            text("Host").size(TEXT_BODY).width(100),
            choose("tf-host", &form.hosts, &f.host, Message::TunnelHost),
        ]
        .spacing(SPACE_8)
        .align_y(iced::Alignment::Start),
        row![
            text("Kind").size(TEXT_BODY).width(100),
            choose("tf-kind", &form.kinds, &kind_id, Message::TunnelKind),
        ]
        .spacing(SPACE_8)
        .align_y(iced::Alignment::Start),
        input("Bind address", TField::BindAddr, &f.bind_addr),
        input("Bind port", TField::BindPort, &f.bind_port),
    ]
    .spacing(SPACE_8);
    if f.kind != TunnelKind::Dynamic {
        body = body
            .push(input("Target host", TField::TargetHost, &f.target_host))
            .push(input("Target port", TField::TargetPort, &f.target_port));
    }
    body = body.push(
        checkbox(f.auto_start)
            .label("Start automatically after unlock")
            .on_toggle(Message::TunnelAuto),
    );
    if let Ok(t) = form.build()
        && let Some(w) = t.bind_warning()
    {
        body = body.push(text(w).size(TEXT_SMALL).style(text::warning));
    }
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

    fn form() -> TunnelForm {
        TunnelForm::new(&[Host::new("bastion", "b.example.com")], None)
    }

    #[test]
    fn local_tunnel_needs_target_and_valid_ports() {
        let mut f = form();
        f.set(TField::Label, "db".into());
        assert!(f.build().unwrap_err().contains("bind port"));
        f.set(TField::BindPort, "5433".into());
        assert!(f.build().unwrap_err().contains("target port"));
        f.set(TField::TargetPort, "70000".into());
        assert!(f.build().unwrap_err().contains("target port"));
        f.set(TField::TargetPort, "5432".into());
        assert!(f.build().is_err(), "target host is required");
        f.set(TField::TargetHost, "db.internal".into());
        let t = f.build().unwrap();
        assert_eq!((t.bind_port, t.target_port), (5433, Some(5432)));
    }

    #[test]
    fn dynamic_ignores_target_fields() {
        let mut f = form();
        f.set(TField::Label, "socks".into());
        f.set(TField::BindPort, "1080".into());
        f.set(TField::TargetHost, "ignored".into());
        f.pick_kind(Choice {
            id: Some("dynamic".into()),
            label: "dynamic".into(),
        });
        let t = f.build().unwrap();
        assert!(t.target_host.is_none() && t.target_port.is_none());
    }

    #[test]
    fn no_hosts_is_an_error_and_edit_keeps_id() {
        let mut f = TunnelForm::new(&[], None);
        f.set(TField::Label, "x".into());
        assert!(f.build().unwrap_err().contains("host"));

        let host = Host::new("h", "h.example.com");
        let t = Tunnel {
            id: TunnelId::new(),
            label: "a".into(),
            host_id: host.id,
            kind: TunnelKind::Dynamic,
            bind_addr: "127.0.0.1".into(),
            bind_port: 1080,
            target_host: None,
            target_port: None,
            auto_start: false,
        };
        let mut f = TunnelForm::new(&[host], Some(&t));
        f.set(TField::Label, "b".into());
        assert_eq!(f.build().unwrap().id, t.id);
    }

    #[test]
    fn dirty_form_needs_two_escapes() {
        let mut f = form();
        assert!(f.request_close());
        f.set(TField::Label, "x".into());
        assert!(!f.request_close());
        assert!(f.request_close());
    }
}
