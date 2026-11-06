//! Settings screen (S8-T04): General, Terminal, Keybindings, Security, About.
//!
//! This module holds the screen's state and view; the logic that talks to the store and the
//! keymap lives in `app/settings_ops.rs`.

use std::collections::BTreeMap;

use iced::widget::{button, checkbox, column, container, row, text, text_input};
use iced::{Color, Element, Fill, Theme};
use tern_vault::KdfParams;

use crate::action::Action;
use crate::app::Message;
use crate::auth::Sensitive;
use crate::keymap::Keymap;
use crate::term_settings::TermSettings;

pub const IDLE_ID: &str = "set-idle";
pub const SCROLL_ID: &str = "set-scrollback";
pub const CURRENT_ID: &str = "set-current";
/// How many keybinding rows are shown at once.
pub const WINDOW: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    General,
    Terminal,
    Keybindings,
    Security,
    About,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::General,
        Tab::Terminal,
        Tab::Keybindings,
        Tab::Security,
        Tab::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Terminal => "Terminal",
            Tab::Keybindings => "Keybindings",
            Tab::Security => "Security",
            Tab::About => "About",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    pub fn next(self) -> Tab {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Tab {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// Argon2 cost presets (memory / passes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cost {
    Fast,
    Standard,
    Strong,
}

impl Cost {
    pub const ALL: [Cost; 3] = [Cost::Fast, Cost::Standard, Cost::Strong];

    pub fn params(self) -> KdfParams {
        let (mib, passes) = match self {
            Cost::Fast => (64, 4),
            Cost::Standard => (128, 8),
            Cost::Strong => (256, 10),
        };
        KdfParams {
            m_cost: mib * 1024,
            t_cost: passes,
            p_cost: 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Cost::Fast => "Fast (64 MiB, 4 passes)",
            Cost::Standard => "Standard (128 MiB, 8 passes)",
            Cost::Strong => "Strong (256 MiB, 10 passes)",
        }
    }

    pub fn from_params(p: KdfParams) -> Option<Cost> {
        Self::ALL.into_iter().find(|c| c.params() == p)
    }
}

/// State of the "press the new key" interaction on the Keybindings tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capture {
    Idle,
    /// Waiting for the user to press the new key.
    Waiting,
    /// The pressed key clashes with other actions; Enter applies it anyway.
    Conflict {
        text: String,
        with: Vec<Action>,
    },
}

#[derive(Debug, Clone)]
pub enum SettingsMsg {
    Tab(Tab),
    Idle(String),
    Keep(bool),
    Scrollback(String),
    Current(Sensitive),
    New(Sensitive),
    Confirm(Sensitive),
    PickCost(Cost),
    Save,
    Saved(Result<String, String>),
    Kdf(Result<KdfParams, String>),
    KbSelect(usize),
    KbRebind,
    KbUnbind,
    KbReset,
    KbResetAll,
    OpenBackupSettings,
}

#[derive(Debug)]
pub struct SettingsScreen {
    pub tab: Tab,
    pub idle: String,
    pub keep: bool,
    pub scrollback: String,
    pub selected: usize,
    pub capture: Capture,
    pub current: Sensitive,
    pub new: Sensitive,
    pub confirm: Sensitive,
    pub cost: Cost,
    pub stored_cost: Option<KdfParams>,
    pub message: Option<String>,
    pub error: Option<String>,
    pub busy: bool,
}

impl SettingsScreen {
    pub fn new(idle_minutes: u32, keep: bool, scrollback: usize) -> Self {
        Self {
            tab: Tab::General,
            idle: idle_minutes.to_string(),
            keep,
            scrollback: scrollback.to_string(),
            selected: 0,
            capture: Capture::Idle,
            current: Sensitive::default(),
            new: Sensitive::default(),
            confirm: Sensitive::default(),
            cost: Cost::Standard,
            stored_cost: None,
            message: None,
            error: None,
            busy: false,
        }
    }

    pub fn selected_action(&self) -> Option<Action> {
        Action::ALL.get(self.selected).copied()
    }

    pub fn move_selection(&mut self, delta: isize) {
        let max = Action::ALL.len().saturating_sub(1) as isize;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    /// The id of the text input to focus when entering the current tab.
    pub fn first_field(&self) -> Option<&'static str> {
        match self.tab {
            Tab::General => Some(IDLE_ID),
            Tab::Terminal => Some(SCROLL_ID),
            Tab::Security => Some(CURRENT_ID),
            Tab::Keybindings | Tab::About => None,
        }
    }
}

fn field<'a>(label: &'static str, input: Element<'a, Message>) -> Element<'a, Message> {
    row![text(label).size(13).width(150), input]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

fn msg(m: SettingsMsg) -> Message {
    Message::Settings(m)
}

/// Everything the view needs from the rest of the app.
pub struct ViewCtx<'a> {
    pub keymap: &'a Keymap,
    pub overrides: &'a BTreeMap<String, Vec<String>>,
    pub term: &'a TermSettings,
    pub data_dir: String,
}

pub fn view<'a>(s: &'a SettingsScreen, ctx: &ViewCtx<'a>) -> Element<'a, Message> {
    let tabs = Tab::ALL.iter().fold(row![].spacing(6), |r, t| {
        let b = button(text(t.label()).size(13)).on_press(msg(SettingsMsg::Tab(*t)));
        r.push(if *t == s.tab {
            b.style(button::primary)
        } else {
            b.style(button::secondary)
        })
    });
    let mut body = column![text("Settings").size(18), tabs].spacing(10);
    body = body.push(match s.tab {
        Tab::General => general(s),
        Tab::Terminal => terminal(s, ctx),
        Tab::Keybindings => keybindings(s, ctx),
        Tab::Security => security(s),
        Tab::About => about(ctx),
    });
    if let Some(m) = &s.message {
        body = body.push(text(m.as_str()).size(13).style(text::success));
    }
    if let Some(e) = &s.error {
        body = body.push(text(e.as_str()).size(13).style(text::danger));
    }
    if s.busy {
        body = body.push(text("Working…").size(13));
    }
    body = body.push(
        text("Ctrl+Tab next section   Ctrl+S save   Esc close")
            .size(12)
            .style(text::secondary),
    );
    let dialog = container(body)
        .padding(20)
        .max_width(640)
        .style(|theme: &Theme| {
            let mut st = container::bordered_box(theme);
            st.background = Some(theme.extended_palette().background.base.color.into());
            st
        });
    container(dialog)
        .center(Fill)
        .style(|_| container::Style {
            background: Some(Color::from_rgba(0.0, 0.0, 0.0, 0.55).into()),
            ..container::Style::default()
        })
        .into()
}

fn general(s: &SettingsScreen) -> Element<'_, Message> {
    column![
        field(
            "Auto-lock (minutes)",
            text_input("0 = never", &s.idle)
                .id(IDLE_ID)
                .on_input(|v| msg(SettingsMsg::Idle(v)))
                .on_submit(msg(SettingsMsg::Save))
                .padding(4)
                .into()
        ),
        checkbox(s.keep)
            .label("Keep sessions and tunnels running while locked")
            .on_toggle(|v| msg(SettingsMsg::Keep(v))),
        button(text("Scheduled backups…").size(13))
            .on_press(msg(SettingsMsg::OpenBackupSettings))
            .style(button::secondary),
        text("0 disables auto-lock. The vault also locks after the computer sleeps.")
            .size(12)
            .style(text::secondary),
    ]
    .spacing(8)
    .into()
}

fn terminal<'a>(s: &'a SettingsScreen, ctx: &ViewCtx<'a>) -> Element<'a, Message> {
    let t = ctx.term;
    let act = |label: String, a: Action| {
        button(text(label).size(13))
            .on_press(Message::Dispatch(a))
            .style(button::secondary)
    };
    column![
        field(
            "Font size",
            row![
                act("−".into(), Action::FontSmaller),
                text(format!("{:.0}", t.font_size)).size(13),
                act("+".into(), Action::FontBigger),
                act("Reset".into(), Action::FontReset),
            ]
            .spacing(8)
            .align_y(iced::Center)
            .into()
        ),
        field(
            "Colors",
            act(format!("{} (change)", t.scheme), Action::CycleScheme).into()
        ),
        field(
            "Cursor",
            act(format!("{} (change)", t.cursor.id()), Action::CycleCursor).into()
        ),
        field(
            "Scrollback lines",
            text_input("10000", &s.scrollback)
                .id(SCROLL_ID)
                .on_input(|v| msg(SettingsMsg::Scrollback(v)))
                .on_submit(msg(SettingsMsg::Save))
                .padding(4)
                .into()
        ),
        checkbox(t.visual_bell)
            .label("Visual bell")
            .on_toggle(|_| Message::Dispatch(Action::ToggleBell)),
        checkbox(t.confirm_paste)
            .label("Warn before multi-line paste")
            .on_toggle(|_| Message::Dispatch(Action::TogglePasteWarn)),
    ]
    .spacing(8)
    .into()
}

fn keybindings<'a>(s: &'a SettingsScreen, ctx: &ViewCtx<'a>) -> Element<'a, Message> {
    let total = Action::ALL.len();
    let start = s
        .selected
        .saturating_sub(WINDOW / 2)
        .min(total.saturating_sub(WINDOW));
    let mut list = column![].spacing(2);
    for (i, action) in Action::ALL.iter().enumerate().skip(start).take(WINDOW) {
        let meta = action.meta();
        let keys = ctx.keymap.keys_for(*action);
        let keys = if keys.is_empty() {
            "—".to_owned()
        } else {
            keys.join(", ")
        };
        let mark = if ctx.overrides.contains_key(meta.id) {
            "*"
        } else {
            " "
        };
        let line = row![
            text(format!("{mark} {}", meta.title)).size(13).width(Fill),
            text(keys).size(13).font(crate::app::MONO),
        ]
        .spacing(8);
        let selected = i == s.selected;
        list = list.push(
            button(line)
                .width(Fill)
                .padding(3)
                .on_press(msg(SettingsMsg::KbSelect(i)))
                .style(move |theme: &Theme, status| {
                    if selected {
                        button::primary(theme, status)
                    } else {
                        button::text(theme, status)
                    }
                }),
        );
    }
    let hint: Element<'a, Message> = match &s.capture {
        Capture::Idle => text("↑/↓ select   Enter rebind   Del unbind   r reset   * = customised")
            .size(12)
            .style(text::secondary)
            .into(),
        Capture::Waiting => text("Press the new key… (Esc cancels)")
            .size(13)
            .style(text::primary)
            .into(),
        Capture::Conflict { text: keys, with } => {
            let names: Vec<_> = with.iter().map(|a| a.meta().title).collect();
            text(format!(
                "'{keys}' is also used by: {}. Enter to use it anyway, Esc to cancel.",
                names.join(", ")
            ))
            .size(13)
            .style(text::danger)
            .into()
        }
    };
    column![
        list,
        hint,
        row![
            button(text("Rebind").size(13))
                .on_press(msg(SettingsMsg::KbRebind))
                .style(button::secondary),
            button(text("Unbind").size(13))
                .on_press(msg(SettingsMsg::KbUnbind))
                .style(button::secondary),
            button(text("Reset").size(13))
                .on_press(msg(SettingsMsg::KbReset))
                .style(button::secondary),
            button(text("Reset all").size(13))
                .on_press(msg(SettingsMsg::KbResetAll))
                .style(button::secondary),
        ]
        .spacing(6),
        text("Rebinding replaces all of the action's keys with the one key you press.")
            .size(12)
            .style(text::secondary),
    ]
    .spacing(8)
    .into()
}

fn security(s: &SettingsScreen) -> Element<'_, Message> {
    let secret = |placeholder: &str, v: &Sensitive, f: fn(Sensitive) -> SettingsMsg| {
        text_input(placeholder, v.as_str())
            .secure(true)
            .on_input(move |t| msg(f(Sensitive::new(t))))
            .on_submit(msg(SettingsMsg::Save))
            .padding(4)
    };
    let mut costs = row![].spacing(6);
    for c in Cost::ALL {
        let b = button(text(c.label()).size(12)).on_press(msg(SettingsMsg::PickCost(c)));
        costs = costs.push(if c == s.cost {
            b.style(button::primary)
        } else {
            b.style(button::secondary)
        });
    }
    let current_cost = match s.stored_cost {
        Some(p) => format!("Current: {} MiB, {} passes", p.m_cost / 1024, p.t_cost),
        None => "Current: …".to_owned(),
    };
    column![
        field(
            "Current password",
            secret(
                "required to change anything",
                &s.current,
                SettingsMsg::Current
            )
            .id(CURRENT_ID)
            .into()
        ),
        field(
            "New password",
            secret("leave empty to keep", &s.new, SettingsMsg::New).into()
        ),
        field(
            "Confirm new",
            secret("repeat", &s.confirm, SettingsMsg::Confirm).into()
        ),
        text("Key derivation cost (higher = slower unlock, harder to brute-force)").size(13),
        costs,
        text(current_cost).size(12).style(text::secondary),
        text("Ctrl+S applies. Your data is not re-encrypted; only its key is re-wrapped.")
            .size(12)
            .style(text::secondary),
    ]
    .spacing(8)
    .into()
}

fn about<'a>(ctx: &ViewCtx<'a>) -> Element<'a, Message> {
    column![
        text(format!("tern {}", env!("CARGO_PKG_VERSION"))).size(16),
        text("Local-first, keyboard-first SSH client.").size(13),
        text(format!("Data directory: {}", ctx.data_dir)).size(13),
        text("License: MIT OR Apache-2.0").size(13),
    ]
    .spacing(6)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_cycle_both_ways() {
        assert_eq!(Tab::General.prev(), Tab::About);
        assert_eq!(Tab::About.next(), Tab::General);
        assert_eq!(Tab::Terminal.next(), Tab::Keybindings);
    }

    #[test]
    fn cost_presets_round_trip_and_are_valid() {
        for c in Cost::ALL {
            let p = c.params();
            assert!(KdfParams::new(p.m_cost, p.t_cost, p.p_cost).is_ok());
            assert_eq!(Cost::from_params(p), Some(c));
        }
        assert_eq!(
            Cost::from_params(KdfParams::default()),
            Some(Cost::Standard)
        );
    }

    #[test]
    fn selection_is_clamped() {
        let mut s = SettingsScreen::new(15, false, 1000);
        s.move_selection(-5);
        assert_eq!(s.selected, 0);
        s.move_selection(10_000);
        assert_eq!(s.selected, Action::ALL.len() - 1);
        assert!(s.selected_action().is_some());
    }
}
