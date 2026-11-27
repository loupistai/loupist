//! `learning_handoff`: a research session forwards the user's request for a
//! learning program to Bot, which drafts the proposal with
//! `learning_program_save`. Modelled on `schedule_handoff`'s question path: the
//! request waits in Bot's queue and is posted there even while Bot is busy.
//! The session saves no program and starts no run.
use super::*;
use crate::{
    chat::surface::ChatSurface,
    scheduled_tasks::{
        questions::{self, BotQuestion},
        types::{QuestionReceipt, ScheduleReceipt, TaskSubject, TaskWatch},
    },
};

/// Pending learning hand-offs in Bot's queue. Not `bot-handoff-`: that prefix
/// is what `schedule_create` accepts as a `handoff_id` for a custom task.
pub(crate) const LEARNING_HANDOFF_PREFIX: &str = "bot-learning-";

/// The same byte limit as a learning program's brief.
const MAX_REQUEST_BYTES: usize = 8 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    request: String,
    #[serde(default)]
    subjects: Option<Vec<TaskSubject>>,
}

fn refused(message: &str) -> BrokerError {
    BrokerError::invalid("learning_handoff", message)
}

fn no_schedule_context() -> BrokerError {
    refused("This reply has no scheduling context, so nothing was handed to Bot. Continue without it and tell the user they can ask again in their next message.")
}

impl AppToolExecutor {
    pub(super) async fn learning_handoff(
        &self,
        policy: &ToolPolicy,
        params: Value,
    ) -> Result<Value, BrokerError> {
        self.ensure_scope(policy)?;
        if self.capture.bound_chat_id.as_deref() != Some(policy.session_id.as_str()) {
            return Err(refused("This request is not bound to this research session, so nothing was handed to Bot. Continue here and tell the user it was not forwarded."));
        }
        match self.capture.surface.as_ref() {
            Some(ChatSurface::Session) => {}
            Some(ChatSurface::Bot) => {
                return Err(refused(
                    "Already in Bot: propose the program here with learning_program_save.",
                ));
            }
            _ => {
                return Err(refused(
                    "Only a research session the user is in can hand a learning request to Bot. Do not retry from this run.",
                ));
            }
        }
        if policy.as_of.is_some() {
            return Err(refused("A learning program needs a present-day request, and this turn has an as-of date. Nothing was handed to Bot: tell the user to ask again without the as-of date."));
        }
        // The guard `learning_program_save` passes, by its row: once this turn
        // holds third-party text, nothing it carries forward reaches Bot.
        self.capture
            .turn_taint
            .enforce(ToolMethod::LearningProgramSave, &params)?;
        if self.capture.search_cancel.is_cancelled() {
            return Err(BrokerError::Execution("This Chat has stopped".into()));
        }
        let request: Request =
            serde_json::from_value(params).map_err(|_| BrokerError::Malformed)?;
        if request.request.trim().is_empty()
            || request.request.len() > MAX_REQUEST_BYTES
            || request.request.contains('\0')
        {
            return Err(BrokerError::invalid(
                "request",
                "the user's request in a few sentences that stand on their own, at most 8 KB",
            ));
        }
        let subjects = request.subjects.unwrap_or_default();
        TaskWatch {
            subjects: Some(subjects.clone()),
            ..TaskWatch::default()
        }
        .validate()
        .map_err(|_| BrokerError::invalid("subjects", "up to eight, each {asset} with a followed asset's id, {world} with a World node id, {lens} with a Lens page id, or {name} with a plain name"))?;
        let context = self
            .capture
            .schedule_context
            .as_ref()
            .ok_or_else(no_schedule_context)?;
        let message = self
            .capture
            .source_message_id
            .as_deref()
            .ok_or_else(no_schedule_context)?;
        let id = format!(
            "{LEARNING_HANDOFF_PREFIX}{}",
            &crate::util::sha256(
                format!("{}\0{message}\0{}", self.loupist_id, request.request).as_bytes()
            )[..32]
        );
        let mut prompt = request.request;
        if !subjects.is_empty() {
            prompt.push_str("\n\nSubjects for the program: ");
            prompt.push_str(&serde_json::to_string(&subjects).map_err(|_| BrokerError::Malformed)?);
        }
        let receipt = QuestionReceipt {
            task_id: id,
            title: "Learning program request".into(),
            question: "Shall I draft a learning program for this? Say how often it should run if you have a preference; nothing runs until you turn it on.".into(),
        };
        ScheduleReceipt::Question(receipt.clone())
            .validate()
            .map_err(|_| BrokerError::Malformed)?;
        let saved = questions::enqueue(
            &self.state,
            &self.loupist_id,
            BotQuestion {
                receipt,
                prompt,
                model_profile_id: context.model_profile_id.clone(),
                thinking_level: context.thinking_level.clone(),
                run_options: context.run_options.clone(),
                use_memory: context.use_memory,
                update_memory: context.update_memory,
                created_at_ms: crate::commands::now_ms(),
            },
        )
        .map_err(command_error)?;
        if let Some(sink) = &self.capture.schedule_sink {
            sink(ScheduleReceipt::Question(saved.receipt.clone()));
        }
        let _ = crate::chat::home::deliver_finished_runs(&self.state, &self.loupist_id);
        Ok(
            json!({"handed_off": true, "notice": "Bot will ask the user about it and, if they want it, draft the program, which runs only once they turn it on. Tell the user to answer in Bot and keep working with them here; do not ask about the program or draft it in this session."}),
        )
    }
}

#[cfg(test)]
#[path = "learning_handoff/tests.rs"]
mod tests;
