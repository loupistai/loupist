use crate::{
    app::{AppState, CommandError},
    python_run::{
        self,
        ledger::{self, PythonStep, ReadSummary, RunLedger},
    },
    store::scripts::{ScriptDraft, ScriptOrigin, ScriptSummary, ScriptVersion},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tauri::State;

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScriptsListRequest {
    loupist_id: String,
    #[cfg_attr(test, ts(optional))]
    query: Option<String>,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScriptGetRequest {
    loupist_id: String,
    name: String,
    #[cfg_attr(test, ts(optional))]
    version: Option<u32>,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScriptSaveRequest {
    loupist_id: String,
    draft: ScriptDraft,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScriptTestRequest {
    loupist_id: String,
    name: String,
    version: u32,
    #[serde(default)]
    #[cfg_attr(test, ts(type = "Record<string, unknown>", optional))]
    args: Map<String, Value>,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PythonStepReplayRequest {
    chat_id: String,
    message_id: String,
    step_id: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PythonReplay {
    pub step: PythonStep,
    pub matches: bool,
    pub stdout: String,
    pub stderr: String,
}

fn store_error(error: crate::store::StoreError) -> CommandError {
    match error {
        crate::store::StoreError::Invalid(message) => CommandError::invalid(message),
        _ => CommandError::internal("Script storage is unavailable."),
    }
}

fn python_error(error: crate::broker::BrokerError) -> CommandError {
    CommandError::invalid(error.to_string())
}

#[tauri::command(async)]
pub fn scripts_list(
    request: ScriptsListRequest,
    state: State<'_, AppState>,
) -> Result<Vec<ScriptSummary>, CommandError> {
    state.ensure_loupist_available(&request.loupist_id)?;
    if request
        .query
        .as_ref()
        .is_some_and(|query| query.chars().count() > 200)
    {
        return Err(CommandError::invalid(
            "Search words must be at most 200 characters.",
        ));
    }
    state
        .store
        .script_list(&request.loupist_id, request.query.as_deref())
        .map_err(store_error)
}

#[tauri::command(async)]
pub fn script_get(
    request: ScriptGetRequest,
    state: State<'_, AppState>,
) -> Result<ScriptVersion, CommandError> {
    state.ensure_loupist_available(&request.loupist_id)?;
    state
        .store
        .script_get(&request.loupist_id, &request.name, request.version)
        .map_err(store_error)
}

#[tauri::command(async)]
pub fn script_save(
    request: ScriptSaveRequest,
    state: State<'_, AppState>,
) -> Result<ScriptVersion, CommandError> {
    state.ensure_loupist_available(&request.loupist_id)?;
    let saved = state
        .store
        .script_save(
            &request.loupist_id,
            request.draft,
            ScriptOrigin::User,
            false,
        )
        .map_err(store_error)?;
    crate::commands::triggers::note_saved_version(
        &state,
        &request.loupist_id,
        &saved.summary.name,
        saved.summary.version,
        &saved.summary.sha256,
    )?;
    Ok(saved)
}

#[tauri::command(async)]
pub fn script_delete(
    request: ScriptGetRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    state.ensure_loupist_available(&request.loupist_id)?;
    state
        .store
        .script_delete(&request.loupist_id, &request.name, request.version)
        .map_err(store_error)
}

#[tauri::command]
pub async fn script_test(
    request: ScriptTestRequest,
    state: State<'_, AppState>,
) -> Result<PythonStep, CommandError> {
    state.ensure_loupist_available(&request.loupist_id)?;
    let script = state
        .store
        .script_get(&request.loupist_id, &request.name, Some(request.version))
        .map_err(store_error)?;
    let authority = super::capture_chat_connector_authority(state.inner(), &request.loupist_id)?;
    let executor = super::tool_executor::AppToolExecutor::for_host_with_connector_authority(
        state.inner(),
        &request.loupist_id,
        authority,
    );
    let reference = format!("{}@{}", script.summary.name, script.summary.version);
    let result = executor.test_python_script(&reference, request.args).await;
    executor.host_shutdown().await;
    let result = result.map_err(python_error)?;
    Ok(PythonStep {
        code: script.code,
        stdout: text(&result, "stdout"),
        stderr: text(&result, "stderr"),
        status: text(&result, "status"),
        reads: reads_from_result(&result),
        rounds: result["rounds"].as_u64().unwrap_or_default() as u32,
    })
}

fn text(result: &Value, name: &str) -> String {
    result[name].as_str().unwrap_or_default().to_owned()
}

fn reads_from_result(result: &Value) -> Vec<ReadSummary> {
    result["reads"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|read| ReadSummary {
            tool: text(read, "tool"),
            status: text(read, "status"),
            citation_url: read["citation_url"].as_str().map(str::to_owned),
            provider: read["provider"].as_str().map(str::to_owned),
            as_of: read["as_of"].as_str().map(str::to_owned),
            dataset: None,
        })
        .collect()
}

pub(crate) fn saved_python_step(
    store: &crate::store::SqliteStore,
    loupist: &str,
    message: &crate::domain::ChatMessage,
    step_id: &str,
) -> Option<Value> {
    let reference = message
        .tool_trace
        .as_ref()?
        .entries
        .iter()
        .find(|entry| entry.name == "run_python" && entry.call_id.as_deref() == Some(step_id))?
        .python
        .as_ref()?
        .replay
        .as_ref()?;
    store.python_replay(loupist, reference).ok()
}

pub(crate) fn step_view(record: &Value) -> Option<PythonStep> {
    Some(PythonStep {
        code: record["code"].as_str()?.to_owned(),
        stdout: text(record, "stdout"),
        stderr: text(record, "stderr"),
        status: text(record, "status"),
        reads: serde_json::from_value(record["reads"].clone()).ok()?,
        rounds: record["rounds"].as_u64().unwrap_or_default() as u32,
    })
}

#[tauri::command]
pub async fn python_step_replay(
    request: PythonStepReplayRequest,
    state: State<'_, AppState>,
) -> Result<PythonReplay, CommandError> {
    let unavailable = || CommandError::invalid("The original Python data is no longer available.");
    let chat = state
        .store
        .get_chat(&request.chat_id)
        .map_err(store_error)?
        .ok_or_else(unavailable)?;
    state.ensure_loupist_available(&chat.loupist_id)?;
    let message = chat
        .messages
        .iter()
        .find(|message| message.id == request.message_id)
        .ok_or_else(unavailable)?;
    let record = saved_python_step(&state.store, &chat.loupist_id, message, &request.step_id)
        .ok_or_else(unavailable)?;
    if record["bootstrap"] != ledger::bootstrap_digest() {
        return Err(unavailable());
    }
    let mut run: RunLedger =
        serde_json::from_value(record["ledger"].clone()).map_err(|_| unavailable())?;
    run.final_round = true;
    let code = record["code"].as_str().ok_or_else(unavailable)?;
    let timeout = record["timeout"].as_u64().ok_or_else(unavailable)?;
    let (_workspace, roots) = ledger::empty_roots().map_err(python_error)?;
    let result = python_run::execute_round(
        &roots,
        &state.artifacts.python_cache_root,
        code,
        timeout,
        crate::web::SearchCancel::default(),
        &run,
        python_run::RoundWriteback::Disabled,
    )
    .await
    .map_err(python_error)?;
    let matches = ["stdout", "stderr", "status"]
        .into_iter()
        .all(|field| result[field] == record[field]);
    Ok(PythonReplay {
        step: step_view(&record).ok_or_else(unavailable)?,
        matches,
        stdout: text(&result, "stdout"),
        stderr: text(&result, "stderr"),
    })
}
