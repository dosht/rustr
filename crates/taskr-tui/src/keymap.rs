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
}

pub struct Binding {
    pub keys: &'static [Key],
    pub label: &'static str,
    pub description: &'static str,
    pub action: Action,
    pub in_hint_bar: bool,
}

const fn bind(
    keys: &'static [Key],
    label: &'static str,
    description: &'static str,
    action: Action,
    in_hint_bar: bool,
) -> Binding {
    Binding { keys, label, description, action, in_hint_bar }
}

use Key::*;

pub const NORMAL_MODE: &[Binding] = &[
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
    bind(&[Char('?')], "?", "Help", Action::Help, true),
    bind(&[Char('q'), Ctrl('c')], "q", "Quit", Action::Quit, true),
];

pub const INPUT_MODE_HELP: &[(&str, &str)] =
    &[("enter", "Confirm"), ("esc", "Cancel"), ("ctrl-u", "Clear input")];

pub fn action_for(key: Key) -> Option<Action> {
    NORMAL_MODE.iter().find(|b| b.keys.contains(&key)).map(|b| b.action)
}

pub fn hint_bar() -> impl Iterator<Item = &'static Binding> {
    NORMAL_MODE.iter().filter(|b| b.in_hint_bar)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_maps_to_exactly_one_action() {
        let mut seen = Vec::new();
        for b in NORMAL_MODE {
            for k in b.keys {
                assert!(!seen.contains(k), "{k:?} is bound twice");
                seen.push(*k);
            }
        }
    }

    #[test]
    fn vim_keys_are_bound() {
        assert_eq!(action_for(Char('j')), Some(Action::Down));
        assert_eq!(action_for(Char('k')), Some(Action::Up));
        assert_eq!(action_for(Char('g')), Some(Action::Top));
        assert_eq!(action_for(Char('G')), Some(Action::Bottom));
        assert_eq!(action_for(Char('/')), Some(Action::Filter));
        assert_eq!(action_for(Char('z')), None);
    }
}
