//! `ask_user`: the model asks the user one question mid-reply and the call
//! waits for the answer, the wait limit or the run stopping. A question nobody
//! answers is not an error: the result tells the model what to assume.
use serde::Deserialize;
use serde_json::{json, Value};

use super::*;
use crate::chat::{
    approvals::APPROVAL_WAIT,
    questions::{
        self, check_question, ChatQuestion, ChatQuestionOption, QuestionAnswer, QuestionState,
    },
    surface::ChatSurface,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    question: String,
    options: Vec<ChatQuestionOption>,
    #[serde(default)]
    recommended: Option<u8>,
}

fn invalid(field: &str, expected: String) -> BrokerError {
    BrokerError::InvalidArguments {
        field: field.into(),
        expected,
    }
}

fn unanswered(question: &ChatQuestion) -> Value {
    let (assumed, next) = match question.recommended_label() {
        Some(label) => (
            json!(label),
            format!("The user did not answer. Continue with the recommended option \"{label}\" and say in the answer which one you used."),
        ),
        None => (
            Value::Null,
            "The user did not answer. Continue with your most reasonable assumption and say in the answer which one you used.".into(),
        ),
    };
    json!({"answered": false, "assumed": assumed, "next": next})
}

impl AppToolExecutor {
    pub(super) async fn ask_user_question(&self, params: Value) -> Result<Value, BrokerError> {
        if !self
            .capture
            .surface
            .as_ref()
            .is_some_and(ChatSurface::interactive)
        {
            return Err(BrokerError::Execution(format!(
                "ask_user is unavailable in a {} turn: there is no one to answer, so assume and say what you assumed",
                self.capture.surface.as_ref().map_or("host", ChatSurface::name)
            )));
        }
        let request: Input = serde_json::from_value(params).map_err(|error| {
            invalid(
                "ask_user",
                format!("{{question, options: [{{label, description?}}] (2 to 4), recommended?: index}}: {error}"),
            )
        })?;
        check_question(&request.question, &request.options, request.recommended)
            .map_err(|(field, expected)| invalid(&field, expected))?;
        let mut question = ChatQuestion {
            request_id: uuid::Uuid::new_v4().to_string(),
            item_id: None,
            question: request.question.trim().to_owned(),
            options: request
                .options
                .into_iter()
                .map(|option| ChatQuestionOption {
                    label: option.label.trim().to_owned(),
                    description: option
                        .description
                        .map(|text| text.trim().to_owned())
                        .filter(|text| !text.is_empty()),
                })
                .collect(),
            recommended: request.recommended,
            state: QuestionState::Waiting,
            answer: None,
            requested_at_ms: chrono::Utc::now().timestamp_millis(),
        };
        // Nobody to ask: a run without cards.
        let Some(sink) = self.capture.question_sink.as_ref() else {
            question.state = QuestionState::Expired;
            return Ok(unanswered(&question));
        };
        let call_id = current_tool_call_id();
        let request_id = question.request_id.clone();
        let receiver = questions::register(&request_id, &self.loupist_id, question.options.len());
        sink(call_id.clone(), question.clone());
        let answer = tokio::select! {
            answer = receiver => answer.ok(),
            _ = tokio::time::sleep(APPROVAL_WAIT) => None,
            _ = self.capture.search_cancel.cancelled() => None,
        };
        questions::forget(&request_id);
        question.state = if answer.is_some() {
            QuestionState::Answered
        } else {
            QuestionState::Expired
        };
        question.answer = answer.clone();
        sink(call_id, question.clone());
        Ok(match answer {
            Some(QuestionAnswer::Choice(index)) => {
                let label = &question.options[usize::from(index)].label;
                json!({"answered": true, "choice": label})
            }
            Some(QuestionAnswer::Text(text)) => {
                // Sites the user names in an answer stay readable, like sites in a message,
                // and the answer itself never leaves in a URL after third-party text.
                self.capture.turn_taint.note_user_text(&text);
                self.capture.turn_taint.protect_private_text(&text);
                json!({"answered": true, "text": text.trim()})
            }
            None => unanswered(&question),
        })
    }
}
