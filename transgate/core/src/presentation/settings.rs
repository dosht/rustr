//! The settings dialog (`SettingsView.tsx`): which tabs exist and which is
//! open. Each tab's content is its own state machine, composed by the app:
//! the profile from the session, the AI tab from the consent panel, the
//! subscription tab from billing, and Manage Data from `data_management`.
//! Theme and language controls on the profile tab are shell mechanics.

use serde::{Deserialize, Serialize};

use crate::account::SessionProfile;
use crate::flags::Capabilities;
use crate::ids::Micros;
use crate::presentation::msg::{Msg, ai, common};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Tab {
    #[default]
    Profile,
    AiFeatures,
    Subscription,
    ManageData,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Profile, Tab::AiFeatures, Tab::Subscription, Tab::ManageData];

    pub fn label(self) -> Msg {
        match self {
            Tab::Profile => common("Profile"),
            Tab::AiFeatures => ai("AI Features"),
            Tab::Subscription => common("Subscription"),
            Tab::ManageData => common("Manage Data"),
        }
    }

    /// The AI tab exists only with the `AI_FEATURES` capability.
    pub fn available(self, capabilities: &Capabilities) -> bool {
        self != Tab::AiFeatures || capabilities.ai
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Opened,
    Closed,
    TabSelected { tab: Tab },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SettingsState {
    open: bool,
    tab: Tab,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct TabVm {
    pub tab: Tab,
    pub label: Msg,
    pub active: bool,
}

/// WEB-IDN-021/022: email, "Joined on" (the confirmed date) and the display
/// name; nothing is editable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct ProfileVm {
    pub email: String,
    pub display_name: String,
    #[cfg_attr(feature = "ts", ts(type = "number | null"))]
    pub joined_on: Option<Micros>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SettingsVm {
    pub open: bool,
    pub title: Msg,
    pub tabs: Vec<TabVm>,
    pub active: Tab,
    pub profile: ProfileVm,
}

impl SettingsState {
    /// The source opens on Profile each time the dialog mounts.
    pub fn update(&mut self, event: Event, capabilities: &Capabilities) {
        match event {
            Event::Opened => *self = SettingsState { open: true, tab: Tab::Profile },
            Event::Closed => *self = SettingsState::default(),
            Event::TabSelected { tab } if self.open && tab.available(capabilities) => self.tab = tab,
            Event::TabSelected { .. } => {}
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The open tab, falling back to Profile if its capability was withdrawn.
    pub fn active(&self, capabilities: &Capabilities) -> Tab {
        if self.tab.available(capabilities) { self.tab } else { Tab::Profile }
    }

    pub fn view(&self, profile: &SessionProfile) -> SettingsVm {
        let active = self.active(&profile.capabilities);
        SettingsVm {
            open: self.open,
            title: common("Settings"),
            tabs: Tab::ALL
                .into_iter()
                .filter(|t| t.available(&profile.capabilities))
                .map(|tab| TabVm { tab, label: tab.label(), active: tab == active })
                .collect(),
            active,
            profile: ProfileVm { email: profile.email.clone(), display_name: profile.display_name.clone(), joined_on: profile.joined_on },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::AccountId;

    fn profile(ai: bool) -> SessionProfile {
        SessionProfile {
            account: AccountId(uuid::Uuid::from_u128(1)),
            email: "ada@example.test".into(),
            full_name: None,
            display_name: "ada".into(),
            joined_on: Some(Micros(5)),
            capabilities: Capabilities { ai, ..Capabilities::default() },
        }
    }

    #[test]
    fn the_ai_tab_exists_only_with_the_ai_capability() {
        let mut s = SettingsState::default();
        s.update(Event::Opened, &profile(false).capabilities);
        let tabs: Vec<Tab> = s.view(&profile(false)).tabs.into_iter().map(|t| t.tab).collect();
        assert_eq!(tabs, vec![Tab::Profile, Tab::Subscription, Tab::ManageData]);
        s.update(Event::TabSelected { tab: Tab::AiFeatures }, &profile(false).capabilities);
        assert_eq!(s.view(&profile(false)).active, Tab::Profile);
        let with_ai: Vec<Tab> = s.view(&profile(true)).tabs.into_iter().map(|t| t.tab).collect();
        assert_eq!(with_ai, Tab::ALL.to_vec());
    }

    #[test]
    fn reopening_starts_on_profile_and_a_withdrawn_tab_falls_back() {
        let mut s = SettingsState::default();
        s.update(Event::Opened, &profile(true).capabilities);
        s.update(Event::TabSelected { tab: Tab::AiFeatures }, &profile(true).capabilities);
        assert_eq!(s.view(&profile(false)).active, Tab::Profile, "capability withdrawn by a session refresh");
        s.update(Event::Closed, &profile(true).capabilities);
        s.update(Event::Opened, &profile(true).capabilities);
        let vm = s.view(&profile(true));
        assert_eq!(vm.active, Tab::Profile);
        assert_eq!(vm.profile, ProfileVm { email: "ada@example.test".into(), display_name: "ada".into(), joined_on: Some(Micros(5)) });
    }
}
