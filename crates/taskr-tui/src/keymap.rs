//! Keys are data. The hint bar and the help screen are generated from this
//! table, so they can never drift from what the keys actually do.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Ctrl(char),
    Up,
    Down,
    Home,
    End,
    Enter,
    Esc,
    Tab,
    Backspace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Down,
    Up,
    Top,
    Bottom,
    HalfPageDown,
    HalfPageUp,
    Toggle,
    Add,
    Filter,
    Clear,
    ShowOpen,
    ShowDone,
    ShowAll,
    NextScope,
    Refresh,
    Help,
    Quit,
    ShowMembers,
    ShowTasks,
    NextWorkspace,
    SignOut,
    MakeOwner,
    MakeEditor,
    MakeViewer,
    Remove,
}

pub struct Binding {
    pub keys: &'static [Key],
    pub label: &'static str,
    pub description: &'static str,
    pub action: Action,
    pub in_hint_bar: bool,
    /// Only bound when signed in to a server (workspaces, members, sign out).
    pub remote_only: bool,
}

const fn bind(
    keys: &'static [Key],
    label: &'static str,
    description: &'static str,
    action: Action,
    in_hint_bar: bool,
) -> Binding {
    Binding { keys, label, description, action, in_hint_bar, remote_only: false }
}

const fn remote(
    keys: &'static [Key],
    label: &'static str,
    description: &'static str,
    action: Action,
    in_hint_bar: bool,
) -> Binding {
    Binding { keys, label, description, action, in_hint_bar, remote_only: true }
}

use Key::*;

pub const TASKS_KEYS: &[Binding] = &[
    bind(&[Char('j'), Down], "j/↓", "Move down", Action::Down, true),
    bind(&[Char('k'), Up], "k/↑", "Move up", Action::Up, true),
    bind(&[Char('g'), Home], "g", "Go to top", Action::Top, false),
    bind(&[Char('G'), End], "G", "Go to bottom", Action::Bottom, false),
    bind(&[Ctrl('d')], "ctrl-d", "Half page down", Action::HalfPageDown, false),
    bind(&[Ctrl('u')], "ctrl-u", "Half page up", Action::HalfPageUp, false),
    bind(&[Char(' '), Char('x'), Enter], "space", "Toggle done", Action::Toggle, true),
    bind(&[Char('a'), Char('o')], "a", "Add task", Action::Add, true),
    bind(&[Char('/')], "/", "Filter", Action::Filter, true),
    bind(&[Esc], "esc", "Clear filter and messages", Action::Clear, false),
    bind(&[Char('1')], "1", "Show open", Action::ShowOpen, false),
    bind(&[Char('2')], "2", "Show done", Action::ShowDone, false),
    bind(&[Char('3')], "3", "Show all", Action::ShowAll, false),
    bind(&[Tab], "tab", "Next tab", Action::NextScope, true),
    bind(&[Char('r'), Ctrl('r')], "r", "Reload", Action::Refresh, false),
    remote(&[Char('w')], "w", "Next workspace", Action::NextWorkspace, true),
    remote(&[Char('m')], "m", "Members", Action::ShowMembers, true),
    remote(&[Char('L')], "L", "Sign out", Action::SignOut, false),
    bind(&[Char('?')], "?", "Help", Action::Help, true),
    bind(&[Char('q'), Ctrl('c')], "q", "Quit", Action::Quit, true),
];

pub const MEMBERS_KEYS: &[Binding] = &[
    bind(&[Char('j'), Down], "j/↓", "Move down", Action::Down, true),
    bind(&[Char('k'), Up], "k/↑", "Move up", Action::Up, true),
    bind(&[Char('g'), Home], "g", "Go to top", Action::Top, false),
    bind(&[Char('G'), End], "G", "Go to bottom", Action::Bottom, false),
    bind(&[Char('a')], "a", "Add member (email [role])", Action::Add, true),
    bind(&[Char('o')], "o", "Make owner", Action::MakeOwner, false),
    bind(&[Char('e')], "e", "Make editor", Action::MakeEditor, true),
    bind(&[Char('v')], "v", "Make viewer", Action::MakeViewer, true),
    bind(&[Char('d')], "d", "Remove member", Action::Remove, true),
    bind(&[Esc], "esc", "Clear messages", Action::Clear, false),
    bind(&[Char('r'), Ctrl('r')], "r", "Reload", Action::Refresh, false),
    remote(&[Char('w')], "w", "Next workspace", Action::NextWorkspace, true),
    remote(&[Char('t')], "t", "Tasks", Action::ShowTasks, true),
    remote(&[Char('L')], "L", "Sign out", Action::SignOut, false),
    bind(&[Char('?')], "?", "Help", Action::Help, true),
    bind(&[Char('q'), Ctrl('c')], "q", "Quit", Action::Quit, true),
];

pub const INPUT_MODE_HELP: &[(&str, &str)] =
    &[("enter", "Confirm"), ("esc", "Cancel"), ("ctrl-u", "Clear input")];

/// The bindings in effect: remote-only keys exist only when signed in.
pub fn active(
    table: &'static [Binding],
    signed_in: bool,
) -> impl Iterator<Item = &'static Binding> {
    table.iter().filter(move |b| signed_in || !b.remote_only)
}

pub fn action_for(table: &'static [Binding], signed_in: bool, key: Key) -> Option<Action> {
    active(table, signed_in).find(|b| b.keys.contains(&key)).map(|b| b.action)
}

pub fn hint_bar(
    table: &'static [Binding],
    signed_in: bool,
) -> impl Iterator<Item = &'static Binding> {
    active(table, signed_in).filter(|b| b.in_hint_bar)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_maps_to_exactly_one_action() {
        for table in [TASKS_KEYS, MEMBERS_KEYS] {
            let mut seen = Vec::new();
            for b in table {
                for k in b.keys {
                    assert!(!seen.contains(k), "{k:?} is bound twice");
                    seen.push(*k);
                }
            }
        }
    }

    #[test]
    fn server_keys_only_exist_when_signed_in() {
        assert_eq!(action_for(TASKS_KEYS, false, Char('w')), None);
        assert_eq!(action_for(TASKS_KEYS, true, Char('w')), Some(Action::NextWorkspace));
        assert!(hint_bar(TASKS_KEYS, false).all(|b| !b.remote_only));
    }

    #[test]
    fn vim_keys_are_bound() {
        assert_eq!(action_for(TASKS_KEYS, false, Char('j')), Some(Action::Down));
        assert_eq!(action_for(TASKS_KEYS, false, Char('k')), Some(Action::Up));
        assert_eq!(action_for(TASKS_KEYS, false, Char('g')), Some(Action::Top));
        assert_eq!(action_for(TASKS_KEYS, false, Char('G')), Some(Action::Bottom));
        assert_eq!(action_for(TASKS_KEYS, false, Char('/')), Some(Action::Filter));
        assert_eq!(action_for(TASKS_KEYS, false, Char('z')), None);
    }
}
