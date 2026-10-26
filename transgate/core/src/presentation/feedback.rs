//! The job feedback dialog (`FeedbackModal.tsx`): three ratings and an
//! optional comment for the job the person opened it from.

use serde::{Deserialize, Serialize};

use crate::account::FeedbackRating;
use crate::ids::JobId;
use crate::presentation::failure::ApiFailure;
use crate::presentation::msg::{Msg, common};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Opened { job: JobId },
    Closed,
    Selected { rating: FeedbackRating },
    CommentChanged { text: String },
    Submitted,
    Answered { result: Result<(), ApiFailure> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    Submit { job: JobId, rating: FeedbackRating, comment: String },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeedbackState {
    job: Option<JobId>,
    rating: Option<FeedbackRating>,
    comment: String,
    submitting: bool,
    thanked: bool,
    notice: Option<Msg>,
}

pub const SELECT_FIRST: Msg = common("Please select an option and provide your feedback.");
pub const SUBMIT_FAILED: Msg = common("An error occurred while submitting your feedback. Please try again.");

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RatingOption {
    pub rating: FeedbackRating,
    pub label: Msg,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FeedbackVm {
    pub open: bool,
    /// After a stored submission: the thank-you body and a Close button.
    pub thanked: bool,
    pub options: Vec<RatingOption>,
    pub comment: String,
    pub submitting: bool,
    pub can_submit: bool,
    /// The source's `alert()` texts, shown as a dialog notice.
    pub notice: Option<Msg>,
}

pub fn label(rating: FeedbackRating) -> Msg {
    match rating {
        FeedbackRating::Disappointed => common("Disappointed"),
        FeedbackRating::Satisfied => common("Satisfied"),
        FeedbackRating::Outstanding => common("Outstanding"),
    }
}

impl FeedbackState {
    pub fn update(&mut self, event: Event) -> Vec<Effect> {
        match event {
            Event::Opened { job } => {
                *self = FeedbackState { job: Some(job), ..FeedbackState::default() };
                vec![]
            }
            Event::Closed => {
                *self = FeedbackState::default();
                vec![]
            }
            Event::Selected { rating } if !self.submitting => {
                self.rating = Some(rating);
                self.notice = None;
                vec![]
            }
            Event::CommentChanged { text } if !self.submitting => {
                self.comment = text;
                vec![]
            }
            Event::Submitted => {
                let Some(job) = self.job.filter(|_| !self.submitting && !self.thanked) else { return vec![] };
                let Some(rating) = self.rating else {
                    self.notice = Some(SELECT_FIRST);
                    return vec![];
                };
                self.submitting = true;
                self.notice = None;
                vec![Effect::Submit { job, rating, comment: self.comment.clone() }]
            }
            Event::Answered { result } if self.submitting => {
                self.submitting = false;
                match result {
                    Ok(()) => {
                        self.thanked = true;
                        self.rating = None;
                        self.comment.clear();
                    }
                    Err(_) => self.notice = Some(SUBMIT_FAILED),
                }
                vec![]
            }
            _ => vec![],
        }
    }

    pub fn job(&self) -> Option<JobId> {
        self.job
    }

    pub fn view(&self) -> FeedbackVm {
        FeedbackVm {
            open: self.job.is_some(),
            thanked: self.thanked,
            options: FeedbackRating::ALL.into_iter().map(|rating| RatingOption { rating, label: label(rating), selected: self.rating == Some(rating) }).collect(),
            comment: self.comment.clone(),
            submitting: self.submitting,
            can_submit: !self.submitting,
            notice: self.notice.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened() -> FeedbackState {
        let mut s = FeedbackState::default();
        s.update(Event::Opened { job: JobId(4) });
        s
    }

    #[test]
    fn submitting_without_a_rating_asks_for_one_and_sends_nothing() {
        let mut s = opened();
        assert!(s.update(Event::Submitted).is_empty());
        assert_eq!(s.view().notice, Some(SELECT_FIRST));
        s.update(Event::Selected { rating: FeedbackRating::Satisfied });
        assert_eq!(s.view().notice, None);
    }

    #[test]
    fn one_submission_is_in_flight_and_success_thanks_and_clears_the_form() {
        let mut s = opened();
        s.update(Event::Selected { rating: FeedbackRating::Outstanding });
        s.update(Event::CommentChanged { text: "great".into() });
        assert_eq!(s.update(Event::Submitted), vec![Effect::Submit { job: JobId(4), rating: FeedbackRating::Outstanding, comment: "great".into() }]);
        assert!(s.update(Event::Submitted).is_empty(), "no second request while one is in flight");
        assert!(s.view().submitting && !s.view().can_submit);
        s.update(Event::Answered { result: Ok(()) });
        let vm = s.view();
        assert!(vm.thanked && vm.comment.is_empty() && vm.options.iter().all(|o| !o.selected));
        assert!(s.update(Event::Submitted).is_empty(), "the thank-you view has only Close");
    }

    #[test]
    fn a_failed_submission_keeps_the_answers_and_says_so() {
        let mut s = opened();
        s.update(Event::Selected { rating: FeedbackRating::Disappointed });
        s.update(Event::CommentChanged { text: "slow".into() });
        s.update(Event::Submitted);
        s.update(Event::Answered { result: Err(ApiFailure::network()) });
        let vm = s.view();
        assert_eq!(vm.notice, Some(SUBMIT_FAILED));
        assert!(!vm.thanked && vm.comment == "slow" && vm.options[0].selected && vm.can_submit);
    }

    #[test]
    fn closing_forgets_everything_and_a_late_answer_changes_nothing() {
        let mut s = opened();
        s.update(Event::Selected { rating: FeedbackRating::Satisfied });
        s.update(Event::Submitted);
        s.update(Event::Closed);
        s.update(Event::Answered { result: Ok(()) });
        assert_eq!(s, FeedbackState::default());
        assert!(!s.view().open);
    }

    #[test]
    fn the_options_are_the_three_source_ratings_in_order() {
        let labels: Vec<Msg> = opened().view().options.into_iter().map(|o| o.label).collect();
        assert_eq!(labels, vec![common("Disappointed"), common("Satisfied"), common("Outstanding")]);
    }
}
