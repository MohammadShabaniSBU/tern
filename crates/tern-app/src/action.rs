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
    Terminal,
    Vault,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::General => "General",
            Category::Navigation => "Navigation",
            Category::List => "Lists",
            Category::Tabs => "Tabs",
            Category::Terminal => "Terminal",
            Category::Vault => "Vault",
        }
    }
}

/// Top-level sections shown in the sidebar (`1`..`3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    Hosts,
    Keys,
    Tunnels,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::Hosts, Section::Keys, Section::Tunnels];

    pub fn label(self) -> &'static str {
        match self {
            Section::Hosts => "Hosts",
            Section::Keys => "Keys",
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
    Back         => "ui.back",        "Back / close",          General, ["esc", "ctrl+shift+esc"], ModeMask::ANY;
    // Navigation
    GoHosts      => "nav.hosts",      "Go to Hosts",           Navigation, ["1"], N;
    GoKeys       => "nav.keys",       "Go to Keys",            Navigation, ["2"], N;
    GoTunnels    => "nav.tunnels",    "Go to Tunnels",         Navigation, ["3"], N;
    // Lists
    MoveDown     => "list.down",      "Move selection down",   List, ["j", "down"], N;
    MoveUp       => "list.up",        "Move selection up",     List, ["k", "up"], N;
    First        => "list.first",     "Jump to first item",    List, ["g g", "home"], N;
    Last         => "list.last",      "Jump to last item",     List, ["shift+g", "end"], N;
    PageDown     => "list.page_down", "Page down",             List, ["pagedown"], N;
    PageUp       => "list.page_up",   "Page up",               List, ["pageup"], N;
    Collapse     => "list.collapse",  "Collapse group / focus sidebar", List, ["h", "left"], N;
    Expand       => "list.expand",    "Expand group / focus list", List, ["l", "right"], N;
    SortToggle   => "list.sort",      "Toggle host sort (label / last connected)", List, ["s"], N;
    Filter       => "list.filter",    "Filter list",           List, ["/"], N;
    Open         => "item.open",      "Open / connect selected", List, ["enter"], N;
    Add          => "item.add",       "Add new item",          List, ["a"], N;
    Edit         => "item.edit",      "Edit selected",         List, ["e"], N;
    Delete       => "item.delete",    "Delete selected",       List, ["d d"], N;
    NewGroup     => "group.new",      "New group (nested under selected group)", List, ["shift+a"], N;
    MoveToGroup  => "host.move",      "Move host to group",    List, ["m"], N;
    GenerateKey  => "key.generate",   "Generate a new SSH key", List, ["ctrl+n"], N;
    ImportKey    => "key.import",     "Import SSH key from file", List, ["i"], N;
    ExportBackup => "io.export_backup", "Export encrypted backup… (asks master password)", Vault, [], G;
    ImportBackup => "io.import_backup", "Import backup…", Vault, [], G;
    ExportPlain  => "io.export_plain", "Export plaintext JSON… (asks master password)", Vault, [], G;
    ImportSshConfig => "io.import_ssh_config", "Import hosts from ~/.ssh/config…", Vault, [], G;
    ImportSshKeys => "io.import_ssh_keys", "Import keys from ~/.ssh…", Vault, [], G;
    ExportSshConfig => "io.export_ssh_config", "Export hosts as ssh_config snippet…", Vault, [], G;
    ScheduleBackups => "io.schedule_backups", "Scheduled backup settings…", Vault, [], G;
    PasteKey     => "key.paste",      "Add SSH key from clipboard", List, ["shift+p"], N;
    CopyPublicKey => "key.copy_public", "Copy public key",     List, ["shift+y"], N;
    ToggleMark   => "list.mark",      "Mark/unmark for bulk actions", List, ["space"], N;
    BulkTag      => "host.tag",       "Add tags to marked hosts", List, ["shift+t"], N;
    RevealStored => "host.reveal_password", "Reveal host password (asks master password)", List, ["shift+r"], N;
    ExportKey    => "key.export",     "Export private key to file (asks master password)", List, ["shift+e"], N;
    Duplicate    => "item.duplicate", "Duplicate selected",    List, ["y"], N;
    ToggleTunnels => "item.tunnels",  "Start/stop host tunnels", List, ["t"], N;
    // Forms
    FormSave     => "form.save",      "Form: save",            General, ["ctrl+s"], ModeMask::INSERT;
    FormNext     => "form.next",      "Form: next field",      General, ["tab"], ModeMask::INSERT;
    FormPrev     => "form.prev",      "Form: previous field",  General, ["shift+tab"], ModeMask::INSERT;
    FormReveal   => "form.reveal",    "Form: reveal/hide password", General, ["ctrl+r"], ModeMask::INSERT;
    FormCopy     => "form.copy",      "Form: copy password (clears in 30 s); in a terminal: copy selection", General, ["ctrl+shift+c"], ModeMask::INSERT;
    // Palette
    PaletteDown  => "palette.down",   "Palette: next result",  General, ["down", "ctrl+n"], ModeMask::PALETTE;
    PaletteUp    => "palette.up",     "Palette: previous result", General, ["up", "ctrl+p"], ModeMask::PALETTE;
    PaletteRun   => "palette.run",    "Palette: run selected", General, ["enter"], ModeMask::PALETTE;
    // Modal dialogs
    Confirm      => "modal.confirm",  "Confirm dialog",        General, ["y", "enter"], ModeMask::MODAL;
    Cancel       => "modal.cancel",   "Cancel dialog",         General, ["n"], ModeMask::MODAL;
    ReplaceHostKey => "hostkey.replace", "Trust the changed host key (only after verifying it)", General, ["shift+r"], ModeMask::MODAL;
    // Tabs
    QuickConnect => "tab.quick_connect", "Quick connect",      Tabs, ["ctrl+shift+o"], G;
    NewTab       => "tab.new",        "New tab from host picker", Tabs, ["ctrl+shift+t"], G;
    CloseTab     => "tab.close",      "Close current tab",     Tabs, ["ctrl+shift+w"], G;
    NextTab      => "tab.next",       "Next tab",              Tabs, ["ctrl+tab"], G;
    PrevTab      => "tab.prev",       "Previous tab",          Tabs, ["ctrl+shift+tab"], G;
    Reconnect    => "session.reconnect", "Reconnect disconnected session", Tabs, ["r"], N;
    JumpHome     => "tab.jump_home",  "Go to Hosts (home tab)", Tabs, ["ctrl+shift+0"], G;
    JumpTab1     => "tab.jump_1",     "Go to tab 1",           Tabs, ["ctrl+shift+1"], G;
    JumpTab2     => "tab.jump_2",     "Go to tab 2",           Tabs, ["ctrl+shift+2"], G;
    JumpTab3     => "tab.jump_3",     "Go to tab 3",           Tabs, ["ctrl+shift+3"], G;
    JumpTab4     => "tab.jump_4",     "Go to tab 4",           Tabs, ["ctrl+shift+4"], G;
    JumpTab5     => "tab.jump_5",     "Go to tab 5",           Tabs, ["ctrl+shift+5"], G;
    JumpTab6     => "tab.jump_6",     "Go to tab 6",           Tabs, ["ctrl+shift+6"], G;
    JumpTab7     => "tab.jump_7",     "Go to tab 7",           Tabs, ["ctrl+shift+7"], G;
    JumpTab8     => "tab.jump_8",     "Go to tab 8",           Tabs, ["ctrl+shift+8"], G;
    JumpTab9     => "tab.jump_9",     "Go to tab 9",           Tabs, ["ctrl+shift+9"], G;
    RenameTab    => "tab.rename",     "Rename current tab",    Tabs, ["ctrl+shift+r"], G;
    DuplicateTab => "tab.duplicate",  "Duplicate current tab (new session to the same host)", Tabs, ["ctrl+shift+d"], G;
    // Terminal
    TermCopy     => "term.copy",      "Copy terminal selection", Terminal, [], G;
    TermPaste    => "term.paste",     "Paste into terminal",   Terminal, ["ctrl+shift+v"], G;
    TermSearch   => "term.search",    "Search terminal scrollback", Terminal, ["ctrl+shift+f"], G;
    SearchNext   => "term.search_next", "Next search match",   Terminal, ["ctrl+shift+g"], G;
    SearchPrev   => "term.search_prev", "Previous search match", Terminal, ["ctrl+alt+shift+g"], G;
    CopyMode     => "term.copy_mode", "Keyboard copy mode (hjkl, v select, y copy)", Terminal, ["ctrl+shift+space"], G;
    ScrollPageUp => "term.scroll_up", "Scroll terminal page up", Terminal, ["shift+pageup"], G;
    ScrollPageDown => "term.scroll_down", "Scroll terminal page down", Terminal, ["shift+pagedown"], G;
    FontBigger   => "term.font_bigger", "Terminal font bigger", Terminal, ["ctrl+="], G;
    FontSmaller  => "term.font_smaller", "Terminal font smaller", Terminal, ["ctrl+-"], G;
    FontReset    => "term.font_reset", "Terminal font size reset", Terminal, ["ctrl+alt+0"], G;
    CycleScheme  => "term.scheme",    "Cycle terminal color scheme", Terminal, [], G;
    CycleCursor  => "term.cursor",    "Cycle terminal cursor style", Terminal, [], G;
    ToggleBell   => "term.bell",      "Toggle visual bell",    Terminal, [], G;
    TogglePasteWarn => "term.paste_warn", "Toggle multi-line paste warning", Terminal, [], G;
    // Vault
    LockVault    => "vault.lock",     "Lock vault",            Vault, ["ctrl+shift+l"], G;
}

impl Action {
    /// Section targeted by a `Go*` action.
    pub fn section(self) -> Option<Section> {
        match self {
            Action::GoHosts => Some(Section::Hosts),
            Action::GoKeys => Some(Section::Keys),
            Action::GoTunnels => Some(Section::Tunnels),
            _ => None,
        }
    }

    /// 0-based tab index for the `JumpTabN` actions.
    pub fn jump_index(self) -> Option<usize> {
        Some(match self {
            Action::JumpTab1 => 0,
            Action::JumpTab2 => 1,
            Action::JumpTab3 => 2,
            Action::JumpTab4 => 3,
            Action::JumpTab5 => 4,
            Action::JumpTab6 => 5,
            Action::JumpTab7 => 6,
            Action::JumpTab8 => 7,
            Action::JumpTab9 => 8,
            _ => return None,
        })
    }

    /// The `JumpTabN` action for a 0-based tab index.
    pub fn jump_tab(index: usize) -> Option<Action> {
        [
            Action::JumpTab1,
            Action::JumpTab2,
            Action::JumpTab3,
            Action::JumpTab4,
            Action::JumpTab5,
            Action::JumpTab6,
            Action::JumpTab7,
            Action::JumpTab8,
            Action::JumpTab9,
        ]
        .get(index)
        .copied()
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
