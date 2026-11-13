//! `ask_user`: the model asks the user one question mid-reply and the call
//! waits for the answer, the wait limit or the run stopping. A question nobody
//! answers is not an error and never grants destructive authority.
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
    options: Vec<OptionInput>,
    #[serde(default)]
    recommended: Option<u8>,
}

/// An option as the model writes it, checked exactly. A saved question's
/// options (`ChatQuestionOption`) are read leniently from the Chat instead.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionInput {
    label: String,
    #[serde(default)]
    description: Option<String>,
}

fn invalid(field: &str, expected: String) -> BrokerError {
    BrokerError::InvalidArguments {
        field: field.into(),
        expected,
    }
}

fn unanswered(question: &ChatQuestion) -> Value {
    json!({"answered": false, "assumed": question.recommended_label(), "next":
        "The user did not answer. For a research or presentation choice, use the recommended option and say which one you used. Do not delete, stop, edit or pause a schedule; disable a watch or briefing; retire a Lens section; unfollow an asset or replace the research focus; or update, resolve or forget Bot memory. Finish the reply and say what you need answered before making any of those changes. Other research, saves and work products may continue."})
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
                "ask_user is unavailable in a {} turn: there is no one to answer; do not treat a missing answer as permission to change anything",
                self.capture.surface.as_ref().map_or("host", ChatSurface::name)
            )));
        }
        let request: Input = serde_json::from_value(params).map_err(|_| {
            invalid(
                "ask_user",
                "{question, options: [{label, description?}] (2 to 4), recommended?: index}".into(),
            )
        })?;
        let options = request
            .options
            .into_iter()
            .map(|option| ChatQuestionOption {
                label: option.label,
                description: option.description,
            })
            .collect::<Vec<_>>();
        check_question(&request.question, &options, request.recommended)
            .map_err(|(field, expected)| invalid(&field, expected))?;
        let mut question = ChatQuestion {
            request_id: uuid::Uuid::new_v4().to_string(),
            item_id: None,
            question: request.question.trim().to_owned(),
            options: options
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
        // Raise the guard before publishing a card, including synchronous sinks.
        let pending = self.capture.turn_taint.begin_question();
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
        pending.settle(answer.is_some());
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
