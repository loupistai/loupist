//! Questions a running Chat asks the user (`ask_user`).
//!
//! When the agent reaches a real fork it cannot settle alone, the tool executor
//! parks the call here, shows a question card in the reply's progress and resumes
//! on the user's answer: one of 2 to 4 options or free text. Answers arrive
//! through `chat_question_answer`. A question nobody answers (the wait ran out or
//! the run stopped) is not an error: the call returns with the recommended option
//! as its assumption and the reply goes on.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use super::approvals::valid_request_id;
use crate::domain::DomainError;

pub const MAX_QUESTION_CHARS: usize = 300;
pub const MIN_OPTIONS: usize = 2;
pub const MAX_OPTIONS: usize = 4;
pub const MAX_LABEL_CHARS: usize = 60;
pub const MAX_DESCRIPTION_CHARS: usize = 160;
pub const MAX_TEXT_ANSWER_CHARS: usize = 500;
/// A reply keeps at most this many question cards.
pub const MAX_PROGRESS_QUESTIONS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum QuestionState {
    Waiting,
    Answered,
    /// The run stopped or the wait ran out before the user answered.
    Expired,
}

/// An option's index, or the user's own words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum QuestionAnswer {
    Choice(u8),
    Text(String),
}

impl QuestionAnswer {
    /// Exactly one of `choice` and `text`, as the answer command receives them.
    pub fn from_parts(choice: Option<u8>, text: Option<String>) -> Option<Self> {
        match (choice, text) {
            (Some(choice), None) => Some(Self::Choice(choice)),
            (None, Some(text)) => Some(Self::Text(text.trim().to_owned())),
            _ => None,
        }
    }

    fn validate(&self, option_count: usize) -> Result<(), &'static str> {
        match self {
            Self::Choice(index) if usize::from(*index) < option_count => Ok(()),
            Self::Choice(_) => Err("choice is not one of the options"),
            Self::Text(text) if valid_text(text, MAX_TEXT_ANSWER_CHARS) => Ok(()),
            Self::Text(_) => Err("text must be 1-500 characters"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChatQuestionOption {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub description: Option<String>,
}

/// One question the reply asked, shown as a card at its step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChatQuestion {
    #[serde(rename = "requestId")]
    pub request_id: String,
    /// The progress step that waits; absent when the call carried no tool call id.
    #[serde(rename = "itemId", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub item_id: Option<String>,
    pub question: String,
    pub options: Vec<ChatQuestionOption>,
    /// Index into `options` the agent suggests and assumes when nobody answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub recommended: Option<u8>,
    pub state: QuestionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub answer: Option<QuestionAnswer>,
    #[serde(rename = "requestedAtMs")]
    pub requested_at_ms: i64,
}

fn valid_text(value: &str, max_chars: usize) -> bool {
    !value.trim().is_empty() && value.chars().count() <= max_chars && !value.contains('\0')
}

/// The first rule a question breaks, as `(field, expected)` for the model.
pub fn check_question(
    question: &str,
    options: &[ChatQuestionOption],
    recommended: Option<u8>,
) -> Result<(), (String, String)> {
    let refuse = |field: &str, expected: &str| Err((field.to_owned(), expected.to_owned()));
    if !valid_text(question, MAX_QUESTION_CHARS) {
        return refuse("question", "1-300 characters");
    }
    if !(MIN_OPTIONS..=MAX_OPTIONS).contains(&options.len()) {
        return refuse("options", "2 to 4 options");
    }
    let mut seen = std::collections::BTreeSet::new();
    for (index, option) in options.iter().enumerate() {
        if !valid_text(&option.label, MAX_LABEL_CHARS) {
            return refuse(&format!("options[{index}].label"), "1-60 characters");
        }
        if !seen.insert(option.label.trim().to_lowercase()) {
            return refuse(
                &format!("options[{index}].label"),
                "a label no other option uses",
            );
        }
        if let Some(description) = &option.description {
            if description.chars().count() > MAX_DESCRIPTION_CHARS || description.contains('\0') {
                return refuse(
                    &format!("options[{index}].description"),
                    "at most 160 characters",
                );
            }
        }
    }
    if recommended.is_some_and(|index| usize::from(index) >= options.len()) {
        return refuse("recommended", "an index into options");
    }
    Ok(())
}

impl ChatQuestion {
    pub fn validate(&self, durable: bool) -> Result<(), DomainError> {
        let invalid = || DomainError::Invalid("chat question is invalid");
        if !valid_request_id(&self.request_id)
            || check_question(&self.question, &self.options, self.recommended).is_err()
            || self.requested_at_ms < 0
            || (durable && self.state == QuestionState::Waiting)
        {
            return Err(invalid());
        }
        match (&self.state, &self.answer) {
            (QuestionState::Answered, Some(answer)) => {
                answer.validate(self.options.len()).map_err(|_| invalid())
            }
            (QuestionState::Waiting | QuestionState::Expired, None) => Ok(()),
            _ => Err(invalid()),
        }
    }

    /// The option the agent assumes when nobody answers.
    pub fn recommended_label(&self) -> Option<&str> {
        self.recommended
            .and_then(|index| self.options.get(usize::from(index)))
            .map(|option| option.label.trim())
    }
}

struct Pending {
    loupist_id: String,
    option_count: usize,
    sender: oneshot::Sender<QuestionAnswer>,
}

fn registry() -> &'static Mutex<HashMap<String, Pending>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Pending>>> = OnceLock::new();
    REGISTRY.get_or_init(Default::default)
}

/// Parks a call until the user answers; the caller must `forget` the id when done.
pub fn register(
    request_id: &str,
    loupist_id: &str,
    option_count: usize,
) -> oneshot::Receiver<QuestionAnswer> {
    let (sender, receiver) = oneshot::channel();
    registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(
            request_id.to_owned(),
            Pending {
                loupist_id: loupist_id.to_owned(),
                option_count,
                sender,
            },
        );
    receiver
}

pub fn forget(request_id: &str) {
    registry()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(request_id);
}

#[derive(Debug, PartialEq, Eq)]
pub enum AnswerError {
    /// The question was answered, expired or never existed, or belongs to another Loupist.
    NotWaiting,
    /// The answer breaks a bound; the question keeps waiting.
    Invalid(&'static str),
}

/// Delivers the user's answer to the waiting call of this Loupist.
pub fn answer(
    loupist_id: &str,
    request_id: &str,
    answer: QuestionAnswer,
) -> Result<(), AnswerError> {
    let mut pending = registry().lock().unwrap_or_else(|error| error.into_inner());
    let Some(entry) = pending
        .get(request_id)
        .filter(|entry| entry.loupist_id == loupist_id)
    else {
        return Err(AnswerError::NotWaiting);
    };
    answer
        .validate(entry.option_count)
        .map_err(AnswerError::Invalid)?;
    let entry = pending.remove(request_id).expect("checked above");
    entry
        .sender
        .send(answer)
        .map_err(|_| AnswerError::NotWaiting)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn option(label: &str) -> ChatQuestionOption {
        ChatQuestionOption {
            label: label.into(),
            description: None,
        }
    }

    fn question(state: QuestionState, answer: Option<QuestionAnswer>) -> ChatQuestion {
        ChatQuestion {
            request_id: "req-1".into(),
            item_id: None,
            question: "Which period should the comparison use?".into(),
            options: vec![option("Last 4 quarters"), option("Last 3 years")],
            recommended: Some(0),
            state,
            answer,
            requested_at_ms: 1,
        }
    }

    #[test]
    fn answers_serialize_as_choice_or_text_and_questions_round_trip() {
        assert_eq!(
            serde_json::to_value(QuestionAnswer::Choice(1)).unwrap(),
            json!({"choice": 1})
        );
        assert_eq!(
            serde_json::to_value(QuestionAnswer::Text("Use TTM".into())).unwrap(),
            json!({"text": "Use TTM"})
        );
        let saved = question(QuestionState::Answered, Some(QuestionAnswer::Choice(1)));
        let value = serde_json::to_value(&saved).unwrap();
        assert_eq!(value["requestId"], "req-1");
        assert_eq!(value["state"], "answered");
        assert_eq!(value["answer"], json!({"choice": 1}));
        assert_eq!(
            serde_json::from_value::<ChatQuestion>(value).unwrap(),
            saved
        );
    }

    #[test]
    fn a_saved_question_cannot_be_waiting_and_an_answer_must_match_its_state() {
        let waiting = question(QuestionState::Waiting, None);
        assert!(waiting.validate(false).is_ok());
        assert!(waiting.validate(true).is_err());
        assert!(question(QuestionState::Expired, None)
            .validate(true)
            .is_ok());
        for bad in [
            question(QuestionState::Answered, None),
            question(QuestionState::Expired, Some(QuestionAnswer::Choice(0))),
            question(QuestionState::Answered, Some(QuestionAnswer::Choice(2))),
            question(
                QuestionState::Answered,
                Some(QuestionAnswer::Text(" ".into())),
            ),
            question(
                QuestionState::Answered,
                Some(QuestionAnswer::Text("x".repeat(MAX_TEXT_ANSWER_CHARS + 1))),
            ),
        ] {
            assert!(bad.validate(true).is_err(), "{bad:?}");
        }
        assert!(question(
            QuestionState::Answered,
            Some(QuestionAnswer::Text("TTM".into()))
        )
        .validate(true)
        .is_ok());
    }

    #[test]
    fn a_question_refusal_names_the_field() {
        let two = vec![option("A"), option("B")];
        let check = |question: &str, options: &[ChatQuestionOption], recommended| {
            check_question(question, options, recommended)
                .unwrap_err()
                .0
        };
        assert_eq!(check("", &two, None), "question");
        assert_eq!(check(&"q".repeat(301), &two, None), "question");
        assert_eq!(check("Which?", &two[..1], None), "options");
        assert_eq!(
            check(
                "Which?",
                &[
                    option("A"),
                    option("B"),
                    option("C"),
                    option("D"),
                    option("E")
                ],
                None
            ),
            "options"
        );
        assert_eq!(
            check("Which?", &[option("A"), option(" ")], None),
            "options[1].label"
        );
        assert_eq!(
            check("Which?", &[option("A"), option("a ")], None),
            "options[1].label"
        );
        assert_eq!(
            check("Which?", &[option("A"), option(&"l".repeat(61))], None),
            "options[1].label"
        );
        let long = ChatQuestionOption {
            label: "B".into(),
            description: Some("d".repeat(161)),
        };
        assert_eq!(
            check("Which?", &[option("A"), long], None),
            "options[1].description"
        );
        assert_eq!(check("Which?", &two, Some(2)), "recommended");
        assert!(check_question(
            "무엇을 기준으로 할까요?",
            &[option("분기"), option("연간")],
            Some(1)
        )
        .is_ok());
    }

    #[test]
    fn an_answer_reaches_only_the_waiting_call_of_the_same_loupist_and_obeys_the_bounds() {
        let mut receiver = register("req-q1", "loupist-a", 2);
        assert_eq!(
            answer("loupist-b", "req-q1", QuestionAnswer::Choice(0)),
            Err(AnswerError::NotWaiting)
        );
        assert!(matches!(
            answer("loupist-a", "req-q1", QuestionAnswer::Choice(2)),
            Err(AnswerError::Invalid(_))
        ));
        assert!(matches!(
            answer("loupist-a", "req-q1", QuestionAnswer::Text("  ".into())),
            Err(AnswerError::Invalid(_))
        ));
        // A refused answer leaves the question waiting.
        answer(
            "loupist-a",
            "req-q1",
            QuestionAnswer::Text("Use TTM".into()),
        )
        .unwrap();
        assert_eq!(
            receiver.try_recv().unwrap(),
            QuestionAnswer::Text("Use TTM".into())
        );
        assert_eq!(
            answer("loupist-a", "req-q1", QuestionAnswer::Choice(0)),
            Err(AnswerError::NotWaiting)
        );
    }

    #[test]
    fn the_command_takes_exactly_one_of_choice_and_text() {
        assert_eq!(
            QuestionAnswer::from_parts(Some(1), None),
            Some(QuestionAnswer::Choice(1))
        );
        assert_eq!(
            QuestionAnswer::from_parts(None, Some("x".into())),
            Some(QuestionAnswer::Text("x".into()))
        );
        assert_eq!(
            QuestionAnswer::from_parts(None, Some("  Use TTM \n".into())),
            Some(QuestionAnswer::Text("Use TTM".into()))
        );
        assert_eq!(QuestionAnswer::from_parts(None, None), None);
        assert_eq!(QuestionAnswer::from_parts(Some(0), Some("x".into())), None);
    }
}
