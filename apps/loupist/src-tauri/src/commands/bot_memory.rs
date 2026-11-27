//! Sheet edits of Bot memory. Forget and update commit through
//! [`crate::chat::bot_memory::commit_sheet`], the same apply and warm-session
//! invalidation as the `bot_memory` tool.
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    app::{AppState, CommandError},
    chat::{
        bot_memory::{self, BotMemoryAction, BotMemoryKind, StagedBotMemory},
        home::{home_thread_id, is_home},
    },
    domain::ChatSession,
};

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BotMemoryListRequest {
    pub loupist_id: String,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BotMemoryForgetRequest {
    pub loupist_id: String,
    pub id: String,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BotMemoryUpdateRequest {
    pub loupist_id: String,
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BotMemoryListEntry {
    pub id: String,
    pub kind: BotMemoryKind,
    pub text: String,
    pub since_ms: i64,
    pub updated_ms: i64,
}

/// A Lens principle whose statement folds to a forgotten entry. Forget does
/// not delete it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BotMemoryPrinciple {
    pub record_id: String,
    pub title: String,
}

/// Remaining entries, plus Lens principles that fold to the forgotten text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BotMemoryForgetResponse {
    pub entries: Vec<BotMemoryListEntry>,
    pub principles: Vec<BotMemoryPrinciple>,
}

#[tauri::command(async, rename_all = "snake_case")]
pub fn bot_memory_list(
    request: BotMemoryListRequest,
    state: State<'_, AppState>,
) -> Result<Vec<BotMemoryListEntry>, CommandError> {
    list(&state, request)
}

#[tauri::command(async, rename_all = "snake_case")]
pub fn bot_memory_forget(
    request: BotMemoryForgetRequest,
    state: State<'_, AppState>,
) -> Result<BotMemoryForgetResponse, CommandError> {
    forget(&state, request)
}

#[tauri::command(async, rename_all = "snake_case")]
pub fn bot_memory_update(
    request: BotMemoryUpdateRequest,
    state: State<'_, AppState>,
) -> Result<Vec<BotMemoryListEntry>, CommandError> {
    update(&state, request)
}

pub(crate) fn list(
    state: &AppState,
    request: BotMemoryListRequest,
) -> Result<Vec<BotMemoryListEntry>, CommandError> {
    Ok(wire(&home(state, &request.loupist_id)?))
}

pub(crate) fn forget(
    state: &AppState,
    request: BotMemoryForgetRequest,
) -> Result<BotMemoryForgetResponse, CommandError> {
    let home = home(state, &request.loupist_id)?;
    let forgotten = bot_memory::shown_entries(&home)
        .into_iter()
        .find(|entry| entry.id == request.id)
        .map(|entry| entry.text);
    let entries = commit(state, &request.loupist_id, |_| {
        Ok(vec![StagedBotMemory {
            entry_id: request.id.clone(),
            action: BotMemoryAction::Forget,
        }])
    })?;
    Ok(BotMemoryForgetResponse {
        entries,
        principles: forgotten
            .map(|text| principle_twins(state, &request.loupist_id, &text))
            .unwrap_or_default(),
    })
}

fn principle_twins(state: &AppState, loupist_id: &str, text: &str) -> Vec<BotMemoryPrinciple> {
    let Ok(principles) = crate::commands::knowledge::principles::list(
        state,
        crate::commands::knowledge::principles::PrincipleListRequest {
            loupist_id: loupist_id.to_owned(),
            include_archived: Some(false),
        },
    ) else {
        return Vec::new();
    };
    principles
        .into_iter()
        .filter(|principle| {
            crate::knowledge::principles::same_folded_text(&principle.statement, text)
        })
        .map(|principle| BotMemoryPrinciple {
            record_id: principle.record_id,
            title: crate::knowledge::principles::principle_title(&principle.statement),
        })
        .collect()
}

pub(crate) fn update(
    state: &AppState,
    request: BotMemoryUpdateRequest,
) -> Result<Vec<BotMemoryListEntry>, CommandError> {
    let text = request.text.trim().to_owned();
    if crate::util::credential_text::contains_credential_text(&text) {
        return Err(CommandError::invalid(
            "This looks like a credential, token or other secret, and Memory never keeps those.",
        ));
    }
    commit(state, &request.loupist_id, |chat| {
        let entry = bot_memory::shown_entries(chat)
            .into_iter()
            .find(|entry| entry.id == request.id)
            .ok_or_else(|| CommandError::not_found("entry"))?;
        Ok(vec![StagedBotMemory {
            entry_id: entry.id,
            action: BotMemoryAction::Update {
                text: text.clone(),
                basis: entry.basis,
            },
        }])
    })
}

fn commit(
    state: &AppState,
    loupist_id: &str,
    staged: impl Fn(&ChatSession) -> Result<Vec<StagedBotMemory>, CommandError>,
) -> Result<Vec<BotMemoryListEntry>, CommandError> {
    state.ensure_loupist_available(loupist_id)?;
    crate::commands::chat_runtime::saves::retry_chat_cas(|| {
        let expected = home(state, loupist_id)?;
        let staged = staged(&expected)?;
        let mut chat = expected.clone();
        let at_ms = crate::commands::now_ms().max(expected.updated_at_ms + 1);
        bot_memory::commit_sheet(&mut chat, at_ms, &staged).map_err(map_memory)?;
        chat.updated_at_ms = at_ms;
        state
            .store
            .replace_chat(&expected, &chat)
            .map_err(crate::commands::map_store)?;
        Ok(wire(&chat))
    })
}

fn home(state: &AppState, loupist_id: &str) -> Result<ChatSession, CommandError> {
    state.ensure_loupist_available(loupist_id)?;
    let chat = state
        .store
        .get_chat(&home_thread_id(loupist_id))
        .map_err(crate::commands::map_store)?
        .ok_or_else(|| CommandError::not_found("Bot"))?;
    if chat.loupist_id != loupist_id || !is_home(&chat) {
        return Err(CommandError::conflict(
            "home Thread belongs to another Loupist",
        ));
    }
    Ok(chat)
}

fn wire(chat: &ChatSession) -> Vec<BotMemoryListEntry> {
    bot_memory::shown_entries(chat)
        .into_iter()
        .map(|entry| BotMemoryListEntry {
            id: entry.id,
            kind: entry.kind,
            text: entry.text,
            since_ms: entry.since_ms,
            updated_ms: entry.updated_ms,
        })
        .collect()
}

fn map_memory(error: String) -> CommandError {
    if error.contains("not stored") || error.contains("not an entry") {
        CommandError::not_found("entry")
    } else {
        CommandError::invalid(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        chat::bot_memory::{BotMemoryNote, StagedBotMemory as Staged},
        commands::tests::{chat, profile, seed_profile},
        domain::ChatRole,
        test_fixtures::chat_message,
    };

    fn state() -> (tempfile::TempDir, AppState) {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let state = AppState::for_test(temporary.path().join("app"));
        seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
        (temporary, state)
    }

    fn remember(id: &str, kind: BotMemoryKind, text: &str) -> Staged {
        Staged {
            entry_id: id.into(),
            action: BotMemoryAction::Remember(BotMemoryNote {
                kind,
                text: text.into(),
                basis: vec!["u1".into()],
            }),
        }
    }

    fn populated(state: &AppState) -> ChatSession {
        let mut home = chat(&home_thread_id("loupist-a"), "loupist-a", 1);
        home.messages.push(chat_message(
            "u1",
            ChatRole::User,
            "Answer in two lines, and I decided to wait.",
        ));
        home.messages
            .push(chat_message("r1", ChatRole::Assistant, "I'll keep that."));
        crate::chat::bot_memory::settle_reply(
            &mut home,
            "r1",
            1,
            &[
                remember("bm-pref", BotMemoryKind::Preference, "Answer in two lines"),
                remember("bm-dec", BotMemoryKind::Decision, "Wait on the basket"),
            ],
            &[],
            Some("u1"),
        )
        .unwrap();
        home.pi_session_cache = Some(crate::domain::PiSessionCache {
            entries_sha256: "a".repeat(64),
            leaf_id: None,
            harness_digest: "b".repeat(64),
            runtime_policy_sha256: "c".repeat(64),
            runtime_surface_sha256: "d".repeat(64),
            connector_authority_sha256: "e".repeat(64),
            memory_access_enabled: true,
            memory_update_enabled: true,
            derived_session_id: "123e4567-e89b-42d3-a456-426614174000".into(),
            context_envelope_sha256: String::new(),
            context_envelope_stale: false,
            execution_model: crate::providers::catalog::ExecutionModelLock {
                profile_id: "openai-codex/gpt-5.6-luna".into(),
                name: "GPT-5.6 Luna".into(),
                provider: "openai-codex".into(),
                model: "gpt-5.6-luna".into(),
                thinking_level: "medium".into(),
                run_options: Default::default(),
            },
        });
        state.store.create_chat(&home).unwrap();
        home
    }

    #[test]
    fn forget_from_the_sheet_matches_the_tool_and_clears_the_warm_session() {
        let (_temporary, state) = state();
        let stored = populated(&state);
        let mut tool = stored.clone();
        tool.messages
            .push(chat_message("u2", ChatRole::User, "Forget the preference."));
        tool.messages
            .push(chat_message("r2", ChatRole::Assistant, "Forgotten."));
        let staged = [Staged {
            entry_id: "bm-pref".into(),
            action: BotMemoryAction::Forget,
        }];
        crate::chat::bot_memory::settle_reply(&mut tool, "r2", 2, &staged, &[], None).unwrap();

        let listed = forget(
            &state,
            BotMemoryForgetRequest {
                loupist_id: "loupist-a".into(),
                id: "bm-pref".into(),
            },
        )
        .unwrap();
        let saved = state
            .store
            .get_chat(&home_thread_id("loupist-a"))
            .unwrap()
            .unwrap();
        assert_eq!(saved.bot_memory, tool.bot_memory);
        assert!(saved.pi_session_cache.is_none());
        assert!(listed.principles.is_empty());
        assert_eq!(
            listed
                .entries
                .iter()
                .map(|entry| entry.text.as_str())
                .collect::<Vec<_>>(),
            ["Wait on the basket"]
        );
        assert!(listed
            .entries
            .iter()
            .all(|entry| entry.kind == BotMemoryKind::Decision));
    }

    #[test]
    fn forget_lists_a_lens_principle_with_the_same_folded_text_and_leaves_it() {
        let (_temporary, state) = state();
        populated(&state);
        let mut connection = state.store.lock_loupist("loupist-a").unwrap();
        let tx = connection.transaction().unwrap();
        let saved = crate::knowledge::principles::principle_save(
            &tx,
            "loupist-a",
            None,
            None,
            crate::knowledge::PrincipleInput {
                statement: "  answer   in TWO lines  ".into(),
                why: None,
                example: None,
                origin: "settings".into(),
                origin_chat_id: None,
                origin_message_id: None,
            },
            3,
        )
        .unwrap();
        let other = crate::knowledge::principles::principle_save(
            &tx,
            "loupist-a",
            None,
            None,
            crate::knowledge::PrincipleInput {
                statement: "Wait on the basket".into(),
                why: None,
                example: None,
                origin: "settings".into(),
                origin_chat_id: None,
                origin_message_id: None,
            },
            4,
        )
        .unwrap();
        tx.commit().unwrap();
        drop(connection);

        let listed = forget(
            &state,
            BotMemoryForgetRequest {
                loupist_id: "loupist-a".into(),
                id: "bm-pref".into(),
            },
        )
        .unwrap();
        assert_eq!(
            listed.principles,
            vec![BotMemoryPrinciple {
                record_id: saved.record_id.clone(),
                title: crate::knowledge::principles::principle_title("answer in TWO lines"),
            }]
        );
        let still = crate::commands::knowledge::principles::list(
            &state,
            crate::commands::knowledge::principles::PrincipleListRequest {
                loupist_id: "loupist-a".into(),
                include_archived: Some(false),
            },
        )
        .unwrap();
        assert!(still
            .iter()
            .any(|principle| principle.record_id == saved.record_id));
        assert!(still
            .iter()
            .any(|principle| principle.record_id == other.record_id));
    }

    #[test]
    fn an_edit_keeps_the_kind_and_a_secret_is_not_stored() {
        let (_temporary, state) = state();
        populated(&state);
        let listed = update(
            &state,
            BotMemoryUpdateRequest {
                loupist_id: "loupist-a".into(),
                id: "bm-pref".into(),
                text: "  Answer in one line  ".into(),
            },
        )
        .unwrap();
        let preference = listed.iter().find(|entry| entry.id == "bm-pref").unwrap();
        assert_eq!(preference.kind, BotMemoryKind::Preference);
        assert_eq!(preference.text, "Answer in one line");

        let before = state
            .store
            .get_chat(&home_thread_id("loupist-a"))
            .unwrap()
            .unwrap();
        let error = update(
            &state,
            BotMemoryUpdateRequest {
                loupist_id: "loupist-a".into(),
                id: "bm-pref".into(),
                text: "The key is sk-abc1234567890abcdefsecret".into(),
            },
        )
        .unwrap_err();
        assert_eq!(error.code.as_str(), "invalid_request");
        let after = state
            .store
            .get_chat(&home_thread_id("loupist-a"))
            .unwrap()
            .unwrap();
        assert_eq!(before.bot_memory, after.bot_memory);
    }
}
