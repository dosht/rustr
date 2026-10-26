//! The support dialog (`SupportModal.tsx`): one message, sent with the job
//! route the person has open. The source closed the dialog whatever the
//! answer was; here a failure stays visible with the message kept (UI-04),
//! and a success closes it as before.

use serde::{Deserialize, Serialize};

use crate::ids::JobId;
use crate::presentation::failure::ApiFailure;
use crate::presentation::msg::{Msg, app};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Opened,
    Closed,
    MessageChanged { text: String },
    /// `job` is the route's job id when sent (`router.query.jobId`).
    Submitted { job: Option<JobId> },
    Answered { result: Result<(), ApiFailure> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    Send { message: String, job: Option<JobId> },
}

/// UI-04: the new failure notice (English meaning in `implementation/core.md`).
pub const SEND_FAILED: Msg = app("p5.support.send_failed");

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SupportState {
    open: bool,
    message: String,
    sending: bool,
    failed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct SupportVm {
    pub open: bool,
    pub message: String,
    pub sending: bool,
    pub notice: Option<Msg>,
}

impl SupportState {
    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Opened => {
                self.open = true;
                self.failed = false;
                vec![]
            }
            Event::Closed if !self.sending => {
                *self = SupportState { message: std::mem::take(&mut self.message), ..SupportState::default() };
                vec![]
            }
            Event::MessageChanged { text } if !self.sending => {
                self.message = text;
                vec![]
            }
            Event::Submitted { job } if self.open && !self.sending => {
                self.sending = true;
                self.failed = false;
                vec![Effect::Send { message: self.message.clone(), job }]
            }
            Event::Answered { result } if self.sending => {
                self.sending = false;
                match result {
                    Ok(()) => *self = SupportState::default(),
                    Err(_) => self.failed = true,
                }
                vec![]
            }
            _ => vec![],
        }
    }

    pub fn view(&self) -> SupportVm {
        SupportVm { open: self.open, message: self.message.clone(), sending: self.sending, notice: self.failed.then_some(SEND_FAILED) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_is_sent_as_typed_with_the_open_job_and_success_closes_and_clears() {
        let mut s = SupportState::default();
        s.update(Event::Opened);
        s.update(Event::MessageChanged { text: "help".into() });
        assert_eq!(s.update(Event::Submitted { job: Some(JobId(3)) }), vec![Effect::Send { message: "help".into(), job: Some(JobId(3)) }]);
        assert!(s.update(Event::Submitted { job: None }).is_empty(), "one send at a time");
        assert!(s.update(Event::Closed).is_empty() && s.view().open, "the dialog stays while sending, like the disabled button");
        s.update(Event::Answered { result: Ok(()) });
        assert_eq!(s.view(), SupportVm { open: false, message: String::new(), sending: false, notice: None });
    }

    #[test]
    fn a_failure_stays_visible_and_keeps_the_message_for_another_try() {
        let mut s = SupportState::default();
        s.update(Event::Opened);
        s.update(Event::MessageChanged { text: "help".into() });
        s.update(Event::Submitted { job: None });
        s.update(Event::Answered { result: Err(ApiFailure::http(500, None)) });
        let vm = s.view();
        assert!(vm.open && vm.message == "help" && vm.notice == Some(SEND_FAILED));
        assert_eq!(s.update(Event::Submitted { job: None }).len(), 1);
        assert_eq!(s.view().notice, None);
    }

    #[test]
    fn an_empty_message_is_sent_like_the_source_and_closing_keeps_a_draft() {
        let mut s = SupportState::default();
        s.update(Event::Opened);
        assert_eq!(s.update(Event::Submitted { job: None }), vec![Effect::Send { message: String::new(), job: None }]);
        s.update(Event::Answered { result: Ok(()) });
        s.update(Event::Opened);
        s.update(Event::MessageChanged { text: "draft".into() });
        s.update(Event::Closed);
        s.update(Event::Opened);
        assert_eq!(s.view().message, "draft", "the source dialog kept its textarea state");
    }
}
