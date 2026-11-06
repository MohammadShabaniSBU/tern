//! Logic behind the settings screen (S8-T04). Child module of `app` so it can reach `State`.

use iced::Task;

use super::{Message, Mode, State, ToastKind};
use crate::action::Action;
use crate::auth::AuthFailure;
use crate::autolock;
use crate::io_form::IoKind;
use crate::keymap::{Chord, Keymap};
use crate::settings::{Capture, Cost, SettingsMsg, SettingsScreen, Tab, WINDOW};
use crate::term_settings;

fn parse_in_range(text: &str, lo: u32, hi: u32, what: &str) -> Result<u32, String> {
    match text.trim().parse::<u32>() {
        Ok(v) if (lo..=hi).contains(&v) => Ok(v),
        _ => Err(format!("{what} must be a number from {lo} to {hi}")),
    }
}

impl State {
    pub(super) fn open_settings(&mut self) -> Task<Message> {
        if self.settings.is_some() {
            return Task::none();
        }
        self.settings = Some(SettingsScreen::new(
            self.autolock.idle_minutes,
            self.autolock.keep_sessions,
            self.term_settings.scrollback,
        ));
        self.mode = Mode::Insert;
        self.matcher.reset();
        let store = self.store.clone();
        Task::batch([
            Task::perform(async move { store.kdf_params().await }, |r| {
                Message::Settings(SettingsMsg::Kdf(r.map_err(|e| e.to_string())))
            }),
            iced::widget::operation::focus(crate::settings::IDLE_ID),
        ])
    }

    pub(super) fn close_settings(&mut self) -> Task<Message> {
        self.settings = None;
        self.leave_insert()
    }

    fn set_setting(&self, key: &'static str, json: String) -> Task<Message> {
        let store = self.store.clone();
        Task::perform(
            async move { store.query(move |db| db.settings().set(key, &json)).await },
            |r| Message::SettingSaved(r.map_err(|e| e.to_string())),
        )
    }

    /// Keys while the settings screen is open. `None` lets the normal keymap handle the key.
    pub(super) fn settings_key(&mut self, chord: &Chord) -> Option<Task<Message>> {
        let text = chord.to_text();
        let s = self.settings.as_mut()?;
        match text.as_str() {
            "ctrl+tab" => {
                let t = s.tab.next();
                return Some(self.on_settings(SettingsMsg::Tab(t)));
            }
            "ctrl+shift+tab" => {
                let t = s.tab.prev();
                return Some(self.on_settings(SettingsMsg::Tab(t)));
            }
            _ => {}
        }
        match s.capture.clone() {
            Capture::Waiting => {
                if text == "esc" {
                    s.capture = Capture::Idle;
                    Some(Task::none())
                } else {
                    Some(self.kb_try_bind(text))
                }
            }
            Capture::Conflict { text: keys, .. } => {
                match text.as_str() {
                    "enter" => return Some(self.kb_commit(keys)),
                    "esc" => s.capture = Capture::Idle,
                    _ => {}
                }
                Some(Task::none())
            }
            Capture::Idle if s.tab == Tab::Keybindings => {
                match text.as_str() {
                    "up" | "k" => s.move_selection(-1),
                    "down" | "j" => s.move_selection(1),
                    "pageup" => s.move_selection(-(WINDOW as isize)),
                    "pagedown" => s.move_selection(WINDOW as isize),
                    "enter" => s.capture = Capture::Waiting,
                    "delete" | "backspace" => return Some(self.on_settings(SettingsMsg::KbUnbind)),
                    "r" => return Some(self.on_settings(SettingsMsg::KbReset)),
                    _ => return None,
                }
                Some(Task::none())
            }
            Capture::Idle => None,
        }
    }

    /// `Esc` with the settings open: cancel a key capture, otherwise close.
    pub(super) fn settings_back(&mut self) -> Task<Message> {
        match &mut self.settings {
            Some(s) if s.capture != Capture::Idle => {
                s.capture = Capture::Idle;
                Task::none()
            }
            _ => self.close_settings(),
        }
    }

    pub(super) fn on_settings(&mut self, m: SettingsMsg) -> Task<Message> {
        let Some(s) = self.settings.as_mut() else {
            return Task::none();
        };
        match m {
            SettingsMsg::Tab(t) => {
                s.tab = t;
                s.capture = Capture::Idle;
                s.message = None;
                s.error = None;
                match s.first_field() {
                    Some(id) => iced::widget::operation::focus(id),
                    None => iced::advanced::widget::operate(
                        iced::advanced::widget::operation::focusable::unfocus(),
                    ),
                }
            }
            SettingsMsg::Idle(v) => {
                s.idle = v;
                Task::none()
            }
            SettingsMsg::Scrollback(v) => {
                s.scrollback = v;
                Task::none()
            }
            SettingsMsg::Keep(v) => {
                s.keep = v;
                self.autolock.keep_sessions = v;
                self.set_setting(autolock::KEEP_SESSIONS, v.to_string())
            }
            SettingsMsg::Current(v) => {
                s.current = v;
                Task::none()
            }
            SettingsMsg::New(v) => {
                s.new = v;
                Task::none()
            }
            SettingsMsg::Confirm(v) => {
                s.confirm = v;
                Task::none()
            }
            SettingsMsg::PickCost(c) => {
                s.cost = c;
                Task::none()
            }
            SettingsMsg::Save => self.settings_save(),
            SettingsMsg::Saved(Ok(note)) => {
                s.busy = false;
                s.error = None;
                s.message = Some(note);
                s.current.clear();
                s.new.clear();
                s.confirm.clear();
                s.stored_cost = Some(s.cost.params());
                self.gate.mark_verified(std::time::Instant::now());
                Task::none()
            }
            SettingsMsg::Saved(Err(e)) => {
                s.busy = false;
                s.message = None;
                s.error = Some(e);
                Task::none()
            }
            SettingsMsg::Kdf(Ok(p)) => {
                s.stored_cost = Some(p);
                s.cost = Cost::from_params(p).unwrap_or(Cost::Standard);
                Task::none()
            }
            SettingsMsg::Kdf(Err(e)) => {
                tracing::warn!(error = %e, "could not read KDF parameters");
                Task::none()
            }
            SettingsMsg::KbSelect(i) => {
                s.selected = i;
                s.capture = Capture::Idle;
                Task::none()
            }
            SettingsMsg::KbRebind => {
                s.capture = Capture::Waiting;
                Task::none()
            }
            SettingsMsg::KbUnbind => match s.selected_action() {
                Some(a) => {
                    self.keymap_overrides
                        .insert(a.meta().id.to_owned(), Vec::new());
                    self.apply_overrides()
                }
                None => Task::none(),
            },
            SettingsMsg::KbReset => match s.selected_action() {
                Some(a) => {
                    self.keymap_overrides.remove(a.meta().id);
                    self.apply_overrides()
                }
                None => Task::none(),
            },
            SettingsMsg::KbResetAll => {
                self.keymap_overrides.clear();
                self.apply_overrides()
            }
            SettingsMsg::OpenBackupSettings => {
                self.settings = None;
                self.open_io(IoKind::BackupSchedule)
            }
        }
    }

    fn settings_save(&mut self) -> Task<Message> {
        let Some(s) = self.settings.as_mut() else {
            return Task::none();
        };
        s.error = None;
        s.message = None;
        match s.tab {
            Tab::General => match parse_in_range(&s.idle, 0, 24 * 60, "Auto-lock minutes") {
                Ok(v) => {
                    self.autolock.idle_minutes = v;
                    s.message = Some(if v == 0 {
                        "Auto-lock is off".to_owned()
                    } else {
                        format!("Auto-lock after {v} min of inactivity")
                    });
                    self.autolock.touch(std::time::SystemTime::now());
                    self.set_setting(autolock::IDLE_MINUTES, v.to_string())
                }
                Err(e) => {
                    s.error = Some(e);
                    Task::none()
                }
            },
            Tab::Terminal => match parse_in_range(&s.scrollback, 100, 200_000, "Scrollback") {
                Ok(v) => {
                    self.term_settings.scrollback = v as usize;
                    for t in &mut self.tabs {
                        t.term.set_scrollback(v as usize);
                        t.refresh();
                    }
                    s.message = Some("Saved".to_owned());
                    self.term_settings_changed(term_settings::SCROLLBACK)
                }
                Err(e) => {
                    s.error = Some(e);
                    Task::none()
                }
            },
            Tab::Security => self.save_security(),
            Tab::Keybindings | Tab::About => Task::none(),
        }
    }

    fn save_security(&mut self) -> Task<Message> {
        let Some(s) = self.settings.as_mut() else {
            return Task::none();
        };
        if s.busy {
            return Task::none();
        }
        if s.current.is_empty() {
            s.error = Some("Enter your current master password.".into());
            return Task::none();
        }
        let new_requested = !s.new.is_empty();
        if new_requested && s.new.as_str() != s.confirm.as_str() {
            s.error = Some("The new passwords do not match.".into());
            return Task::none();
        }
        let params = s.cost.params();
        let cost_changed = s.stored_cost != Some(params);
        if !new_requested && !cost_changed {
            s.error = Some("Nothing to change: enter a new password or pick another cost.".into());
            return Task::none();
        }
        s.busy = true;
        let old = s.current.take_secret();
        let new = if new_requested {
            s.new.take_secret()
        } else {
            use secrecy::ExposeSecret;
            secrecy::SecretString::from(old.expose_secret().to_owned())
        };
        let note = match (new_requested, cost_changed) {
            (true, true) => "Master password and key derivation cost changed",
            (true, false) => "Master password changed",
            _ => "Key derivation cost changed",
        };
        let store = self.store.clone();
        Task::perform(
            async move {
                store
                    .change_security(old, new, cost_changed.then_some(params))
                    .await
                    .map(|()| note.to_owned())
                    .map_err(|e| AuthFailure::from_store(&e).message())
            },
            |r| Message::Settings(SettingsMsg::Saved(r)),
        )
    }

    /// The key the user pressed while rebinding: apply it, or ask first if it clashes.
    fn kb_try_bind(&mut self, text: String) -> Task<Message> {
        let Some(action) = self
            .settings
            .as_ref()
            .and_then(SettingsScreen::selected_action)
        else {
            return Task::none();
        };
        let mut candidate = self.keymap_overrides.clone();
        candidate.insert(action.meta().id.to_owned(), vec![text.clone()]);
        let json = serde_json::to_string(&candidate).unwrap_or_default();
        let built = Keymap::build(self.leader_setting.as_deref(), Some(&json));
        let before = self.keymap.conflicts_with(action);
        let with: Vec<Action> = built
            .conflicts_with(action)
            .into_iter()
            .filter(|a| !before.contains(a))
            .collect();
        if with.is_empty() {
            return self.kb_commit(text);
        }
        if let Some(s) = &mut self.settings {
            s.capture = Capture::Conflict { text, with };
        }
        Task::none()
    }

    fn kb_commit(&mut self, text: String) -> Task<Message> {
        let Some(s) = self.settings.as_mut() else {
            return Task::none();
        };
        s.capture = Capture::Idle;
        let Some(action) = s.selected_action() else {
            return Task::none();
        };
        self.keymap_overrides
            .insert(action.meta().id.to_owned(), vec![text]);
        self.apply_overrides()
    }

    /// Rebuild the live keymap from the override map and persist it.
    fn apply_overrides(&mut self) -> Task<Message> {
        let json = serde_json::to_string(&self.keymap_overrides).unwrap_or_else(|_| "{}".into());
        self.keymap = Keymap::build(self.leader_setting.as_deref(), Some(&json));
        self.matcher.reset();
        if let Some(s) = &mut self.settings {
            s.message = None;
            s.error = (!self.keymap.warnings.is_empty()).then(|| self.keymap.warnings.join("; "));
        }
        let problems = self.keymap.conflicts.len();
        if problems > 0 {
            self.toast(
                ToastKind::Info,
                format!("{problems} overlapping key binding(s); see Keybindings"),
            );
        }
        self.set_setting("keymap", json)
    }
}
