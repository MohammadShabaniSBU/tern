//! The action registry (S2-T01): every user command, declared in exactly one place.
//!
//! To add an action, add one line to the `actions!` table below. The enum, [`Action::ALL`] and
//! [`Action::meta`] are all generated from it, so the keymap, command palette and cheat sheet
//! (which iterate [`Action::ALL`]) pick it up automatically. Execution goes through
//! `State::dispatch`, the single entry point shared by the keymap and the palette.

use std::ops::BitOr;

/// Which modes an action is available in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModeMask(u8);

impl ModeMask {
    pub const NORMAL: Self = Self(1);
    pub const INSERT: Self = Self(2);
    pub const PALETTE: Self = Self(4);
    pub const MODAL: Self = Self(8);
    /// Normal and Insert: the "global chord" actions that work whenever the app is interactive.
    pub const GLOBAL: Self = Self(1 | 2);
    pub const ANY: Self = Self(15);

    pub const fn or(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for ModeMask {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Grouping used by the palette and the cheat sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    General,
    Navigation,
    List,
    Tabs,
    Vault,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::General => "General",
            Category::Navigation => "Navigation",
            Category::List => "Lists",
            Category::Tabs => "Tabs",
            Category::Vault => "Vault",
        }
    }
}

/// Top-level sections shown in the sidebar (`1`..`4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    Hosts,
    Keys,
    Identities,
    Tunnels,
}

impl Section {
    pub const ALL: [Section; 4] = [
        Section::Hosts,
        Section::Keys,
        Section::Identities,
        Section::Tunnels,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::Hosts => "Hosts",
            Section::Keys => "Keys",
            Section::Identities => "Identities",
            Section::Tunnels => "Tunnels",
        }
    }
}

/// Static description of an [`Action`].
#[derive(Debug, Clone, Copy)]
pub struct ActionMeta {
    /// Stable identifier used in the user's keymap overrides. Never rename.
    pub id: &'static str,
    pub title: &'static str,
    pub category: Category,
    /// Default key sequences in the syntax parsed by the keymap engine (`"ctrl+k"`, `"g g"`).
    pub default_keys: &'static [&'static str],
    pub available_in: ModeMask,
}

macro_rules! actions {
    ($( $variant:ident => $id:literal, $title:literal, $cat:ident, [$($key:literal),*], $mask:expr; )*) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Action { $($variant),* }

        impl Action {
            /// Every action, in registry order.
            pub const ALL: &'static [Action] = &[$(Action::$variant),*];

            pub fn meta(self) -> ActionMeta {
                match self {
                    $(Action::$variant => ActionMeta {
                        id: $id,
                        title: $title,
                        category: Category::$cat,
                        default_keys: &[$($key),*],
                        available_in: $mask,
                    }),*
                }
            }
        }
    };
}

const N: ModeMask = ModeMask::NORMAL;
const G: ModeMask = ModeMask::GLOBAL;

actions! {
    // General
    OpenPalette  => "palette.open",   "Open command palette",  General, ["ctrl+k", "ctrl+shift+p"], ModeMask::GLOBAL.or(ModeMask::PALETTE);
    CheatSheet   => "help.cheatsheet","Show keybinding cheat sheet", General, ["f1", "?"], ModeMask::GLOBAL.or(ModeMask::MODAL);
    Settings     => "settings.open",  "Open settings",         General, ["ctrl+,"], G;
    ToggleTheme  => "theme.toggle",   "Toggle dark/light theme", General, [], G;
    Back         => "ui.back",        "Back / close",          General, ["esc"], ModeMask::ANY;
    // Navigation
    GoHosts      => "nav.hosts",      "Go to Hosts",           Navigation, ["1"], N;
    GoKeys       => "nav.keys",       "Go to Keys",            Navigation, ["2"], N;
    GoIdentities => "nav.identities", "Go to Identities",      Navigation, ["3"], N;
    GoTunnels    => "nav.tunnels",    "Go to Tunnels",         Navigation, ["4"], N;
    // Lists
    MoveDown     => "list.down",      "Move selection down",   List, ["j", "down"], N;
    MoveUp       => "list.up",        "Move selection up",     List, ["k", "up"], N;
    First        => "list.first",     "Jump to first item",    List, ["g g", "home"], N;
    Last         => "list.last",      "Jump to last item",     List, ["shift+g", "end"], N;
    PageDown     => "list.page_down", "Page down",             List, ["pagedown"], N;
    PageUp       => "list.page_up",   "Page up",               List, ["pageup"], N;
    Collapse     => "list.collapse",  "Collapse group / focus sidebar", List, ["h", "left"], N;
    Expand       => "list.expand",    "Expand group / focus list", List, ["l", "right"], N;
    Filter       => "list.filter",    "Filter list",           List, ["/"], N;
    Open         => "item.open",      "Open / connect selected", List, ["enter"], N;
    Add          => "item.add",       "Add new item",          List, ["a"], N;
    Edit         => "item.edit",      "Edit selected",         List, ["e"], N;
    Delete       => "item.delete",    "Delete selected",       List, ["d d"], N;
    Duplicate    => "item.duplicate", "Duplicate selected",    List, ["y"], N;
    ToggleTunnels => "item.tunnels",  "Start/stop host tunnels", List, ["t"], N;
    // Palette
    PaletteDown  => "palette.down",   "Palette: next result",  General, ["down", "ctrl+n"], ModeMask::PALETTE;
    PaletteUp    => "palette.up",     "Palette: previous result", General, ["up", "ctrl+p"], ModeMask::PALETTE;
    PaletteRun   => "palette.run",    "Palette: run selected", General, ["enter"], ModeMask::PALETTE;
    // Modal dialogs
    Confirm      => "modal.confirm",  "Confirm dialog",        General, ["y", "enter"], ModeMask::MODAL;
    Cancel       => "modal.cancel",   "Cancel dialog",         General, ["n"], ModeMask::MODAL;
    // Tabs
    QuickConnect => "tab.quick_connect", "Quick connect",      Tabs, ["ctrl+shift+o"], G;
    NewTab       => "tab.new",        "New tab from host picker", Tabs, ["ctrl+shift+t"], G;
    CloseTab     => "tab.close",      "Close current tab",     Tabs, ["ctrl+shift+w"], G;
    NextTab      => "tab.next",       "Next tab",              Tabs, ["ctrl+tab"], G;
    PrevTab      => "tab.prev",       "Previous tab",          Tabs, ["ctrl+shift+tab"], G;
    // Vault
    LockVault    => "vault.lock",     "Lock vault",            Vault, ["ctrl+shift+l"], G;
}

impl Action {
    /// Section targeted by a `Go*` action.
    pub fn section(self) -> Option<Section> {
        match self {
            Action::GoHosts => Some(Section::Hosts),
            Action::GoKeys => Some(Section::Keys),
            Action::GoIdentities => Some(Section::Identities),
            Action::GoTunnels => Some(Section::Tunnels),
            _ => None,
        }
    }

    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.iter().copied().find(|a| a.meta().id == id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn ids_are_unique() {
        let mut seen = HashSet::new();
        for a in Action::ALL {
            assert!(seen.insert(a.meta().id), "duplicate id {}", a.meta().id);
        }
    }

    #[test]
    fn ids_round_trip() {
        for a in Action::ALL {
            assert_eq!(Action::from_id(a.meta().id), Some(*a));
        }
        assert_eq!(Action::from_id("nope"), None);
    }

    #[test]
    fn every_action_is_available_somewhere_and_titled() {
        for a in Action::ALL {
            let m = a.meta();
            assert!(!m.title.is_empty());
            assert!(m.available_in != ModeMask(0), "{} is unreachable", m.id);
        }
    }

    #[test]
    fn mask_contains() {
        assert!(ModeMask::GLOBAL.contains(ModeMask::INSERT));
        assert!(!ModeMask::NORMAL.contains(ModeMask::INSERT));
        assert!((ModeMask::NORMAL | ModeMask::MODAL).contains(ModeMask::MODAL));
    }
}
