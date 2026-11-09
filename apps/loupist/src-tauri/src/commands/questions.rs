//! The user's answer to a question card in a running Chat (`ask_user`).

use serde::Deserialize;
use tauri::State;

use crate::{
    app::{AppState, CommandError},
    chat::questions::{self, AnswerError, QuestionAnswer},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QuestionAnswerRequest {
    loupist_id: String,
    request_id: String,
    choice: Option<u8>,
    text: Option<String>,
}

/// Resumes the waiting `ask_user` call with an option or the user's own words.
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn chat_question_answer(
    request: QuestionAnswerRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state.ensure_loupist_available(&request.loupist_id)?;
    answer_question(request)
}

fn answer_question(request: QuestionAnswerRequest) -> Result<(), CommandError> {
    let answer = QuestionAnswer::from_parts(request.choice, request.text)
        .ok_or_else(|| CommandError::invalid("Send exactly one of choice and text"))?;
    questions::answer(&request.loupist_id, &request.request_id, answer).map_err(|error| match error
    {
        AnswerError::NotWaiting => CommandError::conflict("This question is no longer waiting"),
        AnswerError::Invalid(reason) => CommandError::invalid(reason),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(choice: Option<u8>, text: Option<&str>) -> QuestionAnswerRequest {
        QuestionAnswerRequest {
            loupist_id: "loupist-a".into(),
            request_id: "req-cmd".into(),
            choice,
            text: text.map(Into::into),
        }
    }

    #[test]
    fn the_command_resolves_the_waiting_call_and_refuses_bad_or_foreign_answers() {
        let mut receiver = questions::register("req-cmd", "loupist-a", 2);
        for bad in [
            request(None, None),
            request(Some(0), Some("x")),
            request(Some(2), None),
            request(None, Some("  ")),
        ] {
            assert_eq!(answer_question(bad).unwrap_err().code, "invalid_request");
        }
        let mut foreign = request(Some(0), None);
        foreign.loupist_id = "loupist-b".into();
        assert_eq!(answer_question(foreign).unwrap_err().code, "conflict");
        answer_question(request(Some(1), None)).unwrap();
        assert_eq!(receiver.try_recv().unwrap(), QuestionAnswer::Choice(1));
        assert_eq!(
            answer_question(request(Some(1), None)).unwrap_err().code,
            "conflict"
        );
    }
}
