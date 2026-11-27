use super::*;
use crate::store::scripts::{ScriptDraft, ScriptOrigin};

impl AppToolExecutor {
    pub(super) fn script_tool(
        &self,
        policy: &ToolPolicy,
        method: ToolMethod,
        params: Value,
    ) -> Result<Value, BrokerError> {
        let value = match method {
            ToolMethod::ScriptSave => {
                let draft: ScriptDraft =
                    serde_json::from_value(params).map_err(|_| BrokerError::Malformed)?;
                // The store's own rule for a name, code, summary and arguments
                // is the repair; any other store failure keeps its private text.
                crate::store::scripts::validate_draft(&draft).map_err(|error| match error {
                    crate::store::StoreError::Invalid(rule) => BrokerError::InvalidArguments {
                        field: String::new(),
                        expected: rule,
                    },
                    error => store_error(error),
                })?;
                let origin = if let Some(run_id) = self.capture.scheduled_run_id() {
                    let run = self
                        .state
                        .store
                        .scheduled_task_run_get(&self.loupist_id, run_id)
                        .map_err(store_error)?
                        .ok_or(BrokerError::MethodDenied)?;
                    ScriptOrigin::Run {
                        task_id: run.task_id,
                        run_id: run_id.to_owned(),
                    }
                } else {
                    ScriptOrigin::Chat {
                        chat_id: policy.session_id.clone(),
                        message_id: self
                            .capture
                            .source_message_id
                            .clone()
                            .ok_or(BrokerError::MethodDenied)?,
                    }
                };
                let saved = self
                    .state
                    .store
                    .script_save(
                        &self.loupist_id,
                        draft,
                        origin,
                        self.capture.turn_taint.has_untrusted_content(),
                    )
                    .map_err(store_error)?;
                json!(saved.summary)
            }
            ToolMethod::ScriptList => {
                let arguments = exact_object(&params, &[], &["query"])?;
                let query = arguments
                    .get("query")
                    .map(|value| arguments::bounded_text(Some(value), 1, 200))
                    .transpose()?;
                json!(self
                    .state
                    .store
                    .script_list(&self.loupist_id, query.as_deref())
                    .map_err(store_error)?)
            }
            ToolMethod::ScriptRead => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Read {
                    name: String,
                    version: Option<u32>,
                }
                let request: Read =
                    serde_json::from_value(params).map_err(|_| BrokerError::Malformed)?;
                let saved = self
                    .state
                    .store
                    .script_get(&self.loupist_id, &request.name, request.version)
                    .map_err(store_error)?;
                if saved.summary.metadata.after_outside_text {
                    self.capture.turn_taint.mark_untrusted_text();
                }
                json!(saved)
            }
            _ => return Err(BrokerError::MethodDenied),
        };
        Ok(value)
    }
}
