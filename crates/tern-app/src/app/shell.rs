//! The main window shell (docs/designs/02, 05): sidebar, list pane, detail pane, status bar and
//! toast stack. Child module of `app` so it can read `State`.

use iced::widget::{button, column, container, mouse_area, row, space, text};
use iced::{Center, Element, Fill, Theme};

use super::{Message, Mode, Pane, State};
use crate::action::{Action, Section};
use crate::list;
use crate::sessions::Phase;
use crate::theme::style::{self, ButtonKind, Dot};
use crate::theme::tokens::*;
use crate::theme::{self, palette};
use crate::toast::ToastKind;

/// A small keyboard-key box (`Ctrl K`, `e`).
pub fn kbd<'a>(label: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    container(text(label).size(MONO_KBD).font(theme::MONO))
        .padding([0.0, 5.0])
        .style(style::kbd)
        .into()
}

/// Inline hint: mono key then dim label.
fn hint<'a>(key: &'a str, label: &'a str) -> Element<'a, Message> {
    row![
        text(key)
            .size(MONO_KBD)
            .font(theme::MONO)
            .style(style::text_secondary),
        text(label).size(TEXT_SMALL).style(style::text_muted),
    ]
    .spacing(SPACE_4)
    .align_y(Center)
    .into()
}

/// Label + dim text on one line, e.g. a button caption with its key.
fn button_label<'a>(label: &'a str, key: &'a str) -> Element<'a, Message> {
    row![
        text(label).size(TEXT_BODY).font(theme::UI_MEDIUM),
        text(key)
            .size(MONO_KBD)
            .font(theme::MONO)
            .style(style::text_muted),
    ]
    .spacing(SPACE_8)
    .align_y(Center)
    .into()
}

fn action_button<'a>(
    label: &'a str,
    key: &'a str,
    kind: ButtonKind,
    action: Action,
) -> Element<'a, Message> {
    button(container(button_label(label, key)).center_y(Fill))
        .height(BUTTON_H)
        .padding([0.0, SPACE_12])
        .style(style::button(kind))
        .on_press(Message::Dispatch(action))
        .into()
}

fn phase_dot(phase: &Phase) -> Dot {
    match phase {
        Phase::Connected => Dot::Live,
        Phase::Connecting | Phase::HostKey | Phase::Auth { .. } => Dot::Connecting,
        Phase::Disconnected(None) => Dot::Idle,
        Phase::Disconnected(Some(_)) => Dot::Failed,
    }
}

pub fn dot<'a>(kind: Dot, size: f32) -> Element<'a, Message> {
    container(space())
        .width(size)
        .height(size)
        .style(style::dot(kind, size))
        .into()
}

impl State {
    pub(super) fn shell_view(&self) -> Element<'_, Message> {
        container(
            row![self.sidebar(), self.list_pane(), self.detail_pane()]
                .spacing(SPACE_8)
                .padding(SPACE_8)
                .height(Fill),
        )
        .style(style::base)
        .into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let mut nav = column![].spacing(SPACE_2);
        for (n, section) in Section::ALL.into_iter().enumerate() {
            let active = section == self.section;
            let goto = match section {
                Section::Hosts => Action::GoHosts,
                Section::Keys => Action::GoKeys,
                Section::Tunnels => Action::GoTunnels,
            };
            let count = self.lists[n].len_leaves();
            let line = row![
                text(section.label())
                    .size(TEXT_LABEL)
                    .font(if active { theme::UI_MEDIUM } else { theme::UI })
                    .style(if active {
                        style::text_primary
                    } else {
                        style::text_secondary
                    }),
                space().width(Fill),
                text(count.to_string())
                    .size(TEXT_SMALL)
                    .style(style::text_muted),
                kbd((n + 1).to_string()),
            ]
            .spacing(SPACE_8)
            .align_y(Center);
            nav = nav.push(
                mouse_area(
                    container(line)
                        .width(Fill)
                        .height(BUTTON_H + SPACE_4)
                        .padding([0.0, SPACE_8])
                        .align_y(Center)
                        .style(move |t: &Theme| container::Style {
                            background: active.then(|| palette(t).bg_hover.into()),
                            border: iced::Border::default().rounded(RADIUS_MD),
                            ..Default::default()
                        }),
                )
                .on_press(Message::Dispatch(goto)),
            );
        }

        let mut live = column![].spacing(SPACE_2);
        for (i, t) in self.tabs.iter().enumerate() {
            live = live.push(
                mouse_area(
                    row![
                        dot(phase_dot(&t.phase), STATUS_DOT),
                        text(t.display_name())
                            .size(TEXT_BODY)
                            .style(style::text_secondary),
                        space().width(Fill),
                        text(crate::sessions::key_label(
                            &self.keymap,
                            Action::jump_tab(i)
                        ))
                        .size(MONO_KBD)
                        .font(theme::MONO)
                        .style(style::text_muted),
                    ]
                    .spacing(SPACE_8)
                    .align_y(Center)
                    .padding([SPACE_4, SPACE_8]),
                )
                .on_press(Message::SelectTab(i)),
            );
        }
        let tunnels = self.active_tunnel_count();
        if tunnels > 0 {
            live = live.push(
                row![
                    text("⇄").size(TEXT_SMALL).style(style::text_success),
                    text(format!("{tunnels} tunnel(s)"))
                        .size(TEXT_BODY)
                        .style(style::text_secondary),
                ]
                .spacing(SPACE_8)
                .padding([SPACE_4, SPACE_8]),
            );
        }

        let mut top = column![
            row![
                text("tern").size(TEXT_HEADING).font(theme::UI_SEMIBOLD),
                space().width(Fill),
                container(
                    row![
                        text("🔓").size(TEXT_CAPTION),
                        text("Unlocked").size(TEXT_CAPTION)
                    ]
                    .spacing(SPACE_4)
                )
                .padding([2.0, SPACE_8])
                .style(|t: &Theme| {
                    let p = palette(t);
                    style::pill(p.success, p.success_bg)(t)
                }),
            ]
            .align_y(Center),
            space().height(SPACE_8),
            nav,
        ]
        .spacing(SPACE_4);
        if !self.tabs.is_empty() || tunnels > 0 {
            top = top.push(space().height(SPACE_12)).push(
                text("LIVE")
                    .size(TEXT_CAPTION)
                    .font(theme::UI_SEMIBOLD)
                    .style(style::text_muted),
            );
            top = top.push(live);
        }

        let commands = button(
            container(
                row![
                    text("Commands")
                        .size(TEXT_BODY)
                        .style(style::text_secondary),
                    space().width(Fill),
                    kbd("Ctrl K"),
                ]
                .align_y(Center),
            )
            .center_y(Fill),
        )
        .width(Fill)
        .height(BUTTON_H + SPACE_4)
        .padding([0.0, SPACE_10])
        .style(style::button(ButtonKind::Secondary))
        .on_press(Message::Dispatch(Action::OpenPalette));

        container(column![top, space().height(Fill), commands])
            .padding(SPACE_8)
            .width(SIDEBAR_W)
            .height(Fill)
            .style(style::pane(self.pane == Pane::Sidebar))
            .into()
    }

    fn list_pane(&self) -> Element<'_, Message> {
        let list_state = self.list();
        let header = row![
            text(self.section.label())
                .size(TEXT_TITLE)
                .font(theme::UI_SEMIBOLD),
            container(
                text(list_state.len_leaves().to_string())
                    .size(TEXT_SMALL)
                    .style(style::text_secondary)
            )
            .padding([1.0, SPACE_8])
            .style(style::chip),
            space().width(Fill),
            action_button(
                match self.section {
                    Section::Hosts => "New host",
                    Section::Keys => "New key",
                    Section::Tunnels => "New tunnel",
                },
                "n",
                ButtonKind::Secondary,
                Action::Add
            ),
        ]
        .spacing(SPACE_8)
        .align_y(Center);

        let filter = iced::widget::text_input("Filter by name, address or tag", &list_state.filter)
            .id(super::FILTER_ID)
            .on_input(Message::FilterChanged)
            .on_submit(Message::FilterSubmitted)
            .size(TEXT_BODY)
            .padding([SPACE_8, SPACE_10])
            .style(style::input(false));

        let footer = row![
            hint("j k", "move"),
            hint("gg G", "top / bottom"),
            hint("↵", "connect"),
            hint("e", "edit"),
            hint("dd", "delete"),
            hint("Tab", "next pane"),
        ]
        .spacing(SPACE_16);

        container(column![
            column![header, filter].spacing(SPACE_12).padding(SPACE_16),
            container(list::view(list_state, self.pane == Pane::List))
                .padding([0.0, SPACE_8])
                .height(Fill),
            container(footer)
                .padding([SPACE_8, SPACE_16])
                .width(Fill)
                .style(|t: &Theme| container::Style {
                    border: iced::Border {
                        color: palette(t).border_subtle,
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                }),
        ])
        .width(Fill)
        .height(Fill)
        .style(style::pane(self.pane == Pane::List))
        .into()
    }

    fn detail_text(&self) -> String {
        match self.list().selected_item() {
            Some(item) if item.id.is_some() => {
                let id = item.id.as_deref().unwrap_or_default();
                let found = match self.section {
                    Section::Hosts => self.host_data.host_detail(id).map(|mut t| {
                        if let Some((rid, pw)) = &self.revealed
                            && rid == id
                        {
                            t.push_str(&format!(
                                "\n\nRevealed password (Esc hides):\n{}",
                                pw.as_str()
                            ));
                        }
                        t
                    }),
                    Section::Keys => self.keys_data.detail(id),
                    Section::Tunnels => self.tunnels_data.detail(id),
                };
                found.unwrap_or_else(|| item.label.clone())
            }
            Some(item) => item.label.clone(),
            None => "Nothing selected".to_owned(),
        }
    }

    fn detail_pane(&self) -> Element<'_, Message> {
        let text_blob = self.detail_text();
        let mut lines = text_blob.lines();
        let title = lines.next().unwrap_or_default().to_owned();
        let has_item = self.list().selected_item().is_some_and(|i| i.id.is_some());

        let mut body = column![].spacing(SPACE_8);
        for line in lines {
            body = body.push(detail_line(line.to_owned()));
        }

        let mut head =
            column![text(title).size(TEXT_TITLE).font(theme::UI_SEMIBOLD)].spacing(SPACE_8);
        if has_item {
            let primary = match self.section {
                Section::Hosts => "Open tab",
                Section::Tunnels => "Toggle",
                _ => "Open",
            };
            head = head.push(
                row![
                    action_button(primary, "↵", ButtonKind::Primary, Action::Open),
                    action_button("Edit", "e", ButtonKind::Secondary, Action::Edit),
                    action_button("Delete", "dd", ButtonKind::Danger, Action::Delete),
                ]
                .spacing(SPACE_8),
            );
        }

        container(
            iced::widget::scrollable(
                column![head, space().height(SPACE_12), body].padding(SPACE_16),
            )
            .height(Fill),
        )
        .width(DETAIL_W)
        .height(Fill)
        .style(style::pane(self.pane == Pane::Detail))
        .into()
    }

    pub(super) fn status_bar(&self) -> Element<'_, Message> {
        let (label, color): (&str, fn(&theme::Palette) -> iced::Color) =
            if self.screen != super::Screen::Main {
                ("LOCKED", |p| p.mode_locked)
            } else if self.terminal_focused() {
                ("TERMINAL", |p| p.mode_terminal)
            } else {
                match self.mode {
                    Mode::Normal => ("NORMAL", |p| p.mode_normal),
                    Mode::Insert | Mode::Modal => (self.mode.label(), |p| p.mode_insert),
                    Mode::Palette => ("PALETTE", |p| p.mode_palette),
                }
            };
        let locked = label == "LOCKED";
        let pill = container(text(label).size(TEXT_CAPTION).font(theme::MONO_SEMIBOLD))
            .width(MODE_PILL_W)
            .height(Fill)
            .align_x(Center)
            .align_y(Center)
            .style(move |t: &Theme| style::mode_pill(color(palette(t)), locked)(t));

        let crumb = if self.screen != super::Screen::Main {
            "unlock".to_owned()
        } else if let Some(s) = self.tab() {
            s.label.clone()
        } else {
            format!(
                "{} › {} › {}",
                self.section.label().to_lowercase(),
                self.pane.label(),
                self.list()
                    .selected_item()
                    .map(|i| i.label.as_str())
                    .unwrap_or("—")
            )
        };

        let mut right = row![].spacing(SPACE_16).align_y(Center);
        if self.matcher.is_pending() {
            right = right.push(text("…").size(TEXT_SMALL).style(style::text_warning));
        }
        if !self.tabs.is_empty() {
            right = right.push(
                row![
                    dot(Dot::Live, 8.0),
                    text(format!("{} sessions", self.tabs.len()))
                        .size(TEXT_SMALL)
                        .style(style::text_secondary)
                ]
                .spacing(SPACE_6)
                .align_y(Center),
            );
        }
        let tunnels = self.active_tunnel_count();
        if tunnels > 0 {
            right = right.push(
                text(format!("⇄ {tunnels} tunnel"))
                    .size(TEXT_SMALL)
                    .style(style::text_secondary),
            );
        }
        if self.screen == super::Screen::Main {
            right = right.push(
                text("Vault unlocked")
                    .size(TEXT_SMALL)
                    .style(style::text_secondary),
            );
        }
        right = right.push(text("? help").size(TEXT_SMALL).style(style::text_muted));

        container(
            row![
                pill,
                text(crumb)
                    .size(MONO_VALUE)
                    .font(theme::MONO)
                    .style(style::text_secondary),
                space().width(Fill),
                right,
            ]
            .spacing(SPACE_12)
            .align_y(Center)
            .padding(iced::Padding {
                right: SPACE_12,
                ..iced::Padding::ZERO
            })
            .height(Fill),
        )
        .height(STATUS_BAR_H)
        .width(Fill)
        .style(style::status_bar)
        .into()
    }

    /// Bottom-right toast stack (newest at the bottom, at most 3).
    pub(super) fn toast_layer(&self) -> Option<Element<'_, Message>> {
        if self.toasts.is_empty() {
            return None;
        }
        let all: Vec<_> = self.toasts.iter().collect();
        let start = all.len().saturating_sub(3);
        let mut stack = column![].spacing(SPACE_8);
        for t in &all[start..] {
            let error = t.kind == ToastKind::Error;
            let (icon, icon_style): (&str, fn(&Theme) -> text::Style) = match t.kind {
                ToastKind::Success => ("●", style::text_success),
                ToastKind::Info => ("●", style::text_accent),
                ToastKind::Error => ("●", style::text_error),
            };
            stack = stack.push(
                container(
                    row![
                        text(icon).size(TEXT_SMALL).style(icon_style),
                        text(t.text.as_str()).size(TEXT_LABEL),
                    ]
                    .spacing(SPACE_10)
                    .align_y(Center),
                )
                .width(TOAST_W)
                .padding([SPACE_10, SPACE_12])
                .style(style::toast(error)),
            );
        }
        Some(
            container(stack)
                .width(Fill)
                .height(Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Bottom)
                .padding(iced::Padding {
                    top: 0.0,
                    left: 0.0,
                    right: SPACE_16,
                    bottom: STATUS_BAR_H + SPACE_16,
                })
                .into(),
        )
    }
}

/// `Key      value` lines become a muted key column + mono value; other lines are body text.
fn detail_line<'a>(line: String) -> Element<'a, Message> {
    if line.is_empty() {
        return space().height(SPACE_8).into();
    }
    if let Some(at) = line.find("  ") {
        let (k, v) = line.split_at(at);
        let v = v.trim_start().to_owned();
        if !k.is_empty() && k.chars().count() <= 14 && !k.contains(' ') || k == "Last used" {
            return row![
                text(k.to_owned())
                    .size(TEXT_BODY)
                    .width(100)
                    .style(style::text_muted),
                text(v)
                    .size(MONO_VALUE)
                    .font(theme::MONO)
                    .style(style::text_primary),
            ]
            .spacing(SPACE_12)
            .into();
        }
    }
    text(line)
        .size(TEXT_BODY)
        .style(style::text_secondary)
        .into()
}
