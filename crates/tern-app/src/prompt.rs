//! Single-line text prompt used for group create/rename and "move to group" (S3-T03).

use iced::Element;
use iced::widget::{column, text, text_input};
use tern_core::{Group, GroupId, HostId, KeyId};

use crate::app::Message;
use crate::theme::tokens::*;

pub const PROMPT_ID: &str = "prompt-input";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptKind {
    /// New group, optionally nested under a parent.
    NewGroup {
        parent: Option<GroupId>,
    },
    RenameGroup(GroupId),
    /// Type a group name; empty removes the host from its group.
    MoveHost(HostId),
    /// Comma-separated tags added to every listed host.
    BulkTag(Vec<HostId>),
    /// Destination file for an exported private key.
    ExportKey(KeyId),
    /// New name for a terminal tab (empty restores the host label).
    RenameTab(crate::sessions::SessionId),
}

#[derive(Debug, Clone)]
pub struct Prompt {
    pub kind: PromptKind,
    pub input: String,
    pub error: Option<String>,
}

/// What submitting the prompt should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptOutcome {
    CreateGroup(Group),
    RenameGroup(GroupId, String),
    MoveHost(HostId, Option<GroupId>),
    AddTags(Vec<HostId>, Vec<String>),
    ExportKey(KeyId, String),
    RenameTab(crate::sessions::SessionId, String),
}

impl Prompt {
    pub fn new(kind: PromptKind, initial: impl Into<String>) -> Self {
        Self {
            kind,
            input: initial.into(),
            error: None,
        }
    }

    pub fn title(&self) -> &'static str {
        match self.kind {
            PromptKind::NewGroup { .. } => "New group name",
            PromptKind::RenameGroup(_) => "Rename group",
            PromptKind::MoveHost(_) => "Move to group (empty = none)",
            PromptKind::BulkTag(_) => "Add tags (comma separated)",
            PromptKind::ExportKey(_) => "Export private key to file",
            PromptKind::RenameTab(_) => "Rename tab (empty = host name)",
        }
    }

    /// Validate the input against the existing groups.
    pub fn resolve(&self, groups: &[Group]) -> Result<PromptOutcome, String> {
        let name = self.input.trim();
        let same = |g: &&Group| g.name.eq_ignore_ascii_case(name);
        match &self.kind {
            PromptKind::NewGroup { parent } => {
                let group = Group {
                    id: GroupId::new(),
                    name: name.to_owned(),
                    parent_id: *parent,
                    sort_order: 0,
                };
                group.validate().map_err(|e| e.to_string())?;
                if groups.iter().filter(same).any(|g| g.parent_id == *parent) {
                    return Err("a group with that name already exists here".into());
                }
                Ok(PromptOutcome::CreateGroup(group))
            }
            PromptKind::RenameGroup(id) => {
                let current = groups.iter().find(|g| g.id == *id);
                let probe = Group {
                    id: *id,
                    name: name.to_owned(),
                    parent_id: None,
                    sort_order: 0,
                };
                probe.validate().map_err(|e| e.to_string())?;
                let parent = current.and_then(|g| g.parent_id);
                if groups
                    .iter()
                    .filter(same)
                    .any(|g| g.id != *id && g.parent_id == parent)
                {
                    return Err("a group with that name already exists here".into());
                }
                Ok(PromptOutcome::RenameGroup(*id, name.to_owned()))
            }
            PromptKind::RenameTab(id) => Ok(PromptOutcome::RenameTab(*id, name.to_owned())),
            PromptKind::ExportKey(id) => {
                if name.is_empty() {
                    return Err("enter a file path".into());
                }
                Ok(PromptOutcome::ExportKey(*id, name.to_owned()))
            }
            PromptKind::BulkTag(hosts) => {
                let tags: Vec<String> = name
                    .split(',')
                    .map(|t| t.trim().trim_start_matches('#').to_owned())
                    .filter(|t| !t.is_empty())
                    .collect();
                if tags.is_empty() {
                    return Err("enter at least one tag".into());
                }
                Ok(PromptOutcome::AddTags(hosts.clone(), tags))
            }
            PromptKind::MoveHost(host) => {
                if name.is_empty() {
                    return Ok(PromptOutcome::MoveHost(*host, None));
                }
                let matches: Vec<&Group> = groups.iter().filter(same).collect();
                match matches.as_slice() {
                    [g] => Ok(PromptOutcome::MoveHost(*host, Some(g.id))),
                    [] => Err(format!("no group named '{name}' (create it with Shift+A)")),
                    _ => Err(format!("several groups are named '{name}'")),
                }
            }
        }
    }
}

pub fn view(prompt: &Prompt) -> Element<'_, Message> {
    let mut body = column![
        text(prompt.title()).size(TEXT_TITLE),
        text_input("", &prompt.input)
            .style(crate::theme::style::input(false))
            .id(PROMPT_ID)
            .on_input(Message::PromptChanged)
            .on_submit(Message::PromptSubmit)
            .padding(SPACE_4),
    ]
    .spacing(SPACE_8);
    if let Some(e) = &prompt.error {
        body = body.push(
            text(e.as_str())
                .size(TEXT_BODY)
                .style(crate::theme::style::text_error),
        );
    }
    body = body.push(text("Enter confirm   Esc cancel").size(TEXT_SMALL));
    crate::theme::widgets::dialog(body, 420.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(name: &str, parent: Option<GroupId>) -> Group {
        Group {
            id: GroupId::new(),
            name: name.into(),
            parent_id: parent,
            sort_order: 0,
        }
    }

    #[test]
    fn new_group_rejects_blank_and_duplicates_per_parent() {
        let work = group("Work", None);
        let groups = vec![work.clone()];
        let p = |s: &str, parent| Prompt::new(PromptKind::NewGroup { parent }, s);
        assert!(p("  ", None).resolve(&groups).is_err());
        assert!(p("work", None).resolve(&groups).is_err());
        assert!(p("work", Some(work.id)).resolve(&groups).is_ok());
        assert!(matches!(
            p(" Home ", None).resolve(&groups),
            Ok(PromptOutcome::CreateGroup(g)) if g.name == "Home"
        ));
    }

    #[test]
    fn rename_allows_same_name_but_not_a_sibling_name() {
        let (a, b) = (group("A", None), group("B", None));
        let groups = vec![a.clone(), b];
        let rename = |s: &str| Prompt::new(PromptKind::RenameGroup(a.id), s).resolve(&groups);
        assert!(rename("A").is_ok());
        assert!(rename("a2").is_ok());
        assert!(rename("b").is_err());
        assert!(rename("").is_err());
    }

    #[test]
    fn bulk_tag_parses_and_requires_a_tag() {
        let h = HostId::new();
        let p = |s: &str| Prompt::new(PromptKind::BulkTag(vec![h]), s).resolve(&[]);
        assert!(p(" , ").is_err());
        assert_eq!(
            p("#db, prod"),
            Ok(PromptOutcome::AddTags(
                vec![h],
                vec!["db".into(), "prod".into()]
            ))
        );
    }

    #[test]
    fn move_matches_existing_groups_case_insensitively() {
        let work = group("Work", None);
        let groups = vec![work.clone()];
        let host = HostId::new();
        let mv = |s: &str| Prompt::new(PromptKind::MoveHost(host), s).resolve(&groups);
        assert_eq!(mv("work"), Ok(PromptOutcome::MoveHost(host, Some(work.id))));
        assert_eq!(mv(""), Ok(PromptOutcome::MoveHost(host, None)));
        assert!(mv("nope").is_err());
    }
}
