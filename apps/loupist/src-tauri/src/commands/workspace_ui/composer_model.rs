//! Profile-local choices refer to the model catalog of this app installation.

use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::{map_store, text, with_draft_chat, DraftIdentity};
use crate::app::{AppState, CommandError};
use crate::store::{self, StoreResult};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct ComposerModelSelection {
    model_profile_id: String,
    thinking_level: String,
    #[cfg_attr(test, ts(type = "Record<string, string>"))]
    run_options: BTreeMap<String, String>,
}

impl ComposerModelSelection {
    fn validate(&self) -> Result<(), CommandError> {
        text(&self.model_profile_id, 512, false)?;
        text(&self.thinking_level, 128, true)?;
        if self.run_options.len() > 32 {
            return Err(CommandError::invalid("too many composer model options"));
        }
        for (key, value) in &self.run_options {
            text(key, 128, false)?;
            text(value, 256, true)?;
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ComposerModelRead {
    /// Null reads the last model sent from any Chat on this profile.
    chat: Option<DraftIdentity>,
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum ComposerModelWrite {
    Set {
        chat: DraftIdentity,
        selection: Option<ComposerModelSelection>,
        #[serde(rename = "lastSent")]
        last_sent: Option<ComposerModelSelection>,
    },
    Reset {
        #[serde(rename = "loupistId")]
        loupist_id: Option<String>,
    },
}

fn initialize(db: &Connection) -> StoreResult<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS composer_model_selections (
            loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE CASCADE,
            thread_id TEXT NOT NULL,
            data_json TEXT NOT NULL,
            PRIMARY KEY(loupist_id, thread_id)
        ) STRICT;",
    )?;
    Ok(())
}

fn validate_chat(chat: &DraftIdentity) -> Result<(), CommandError> {
    text(&chat.loupist_id, 256, false)?;
    text(&chat.thread_id, 256, false)?;
    if chat.thread_id == "__empty-chat__" {
        return Err(CommandError::invalid("a composer model needs a saved Chat"));
    }
    Ok(())
}

#[tauri::command(async)]
pub(crate) fn chat_composer_model_read(
    request: ComposerModelRead,
    state: State<'_, AppState>,
) -> Result<Option<ComposerModelSelection>, CommandError> {
    read(&state, request.chat)
}

fn read(
    state: &AppState,
    chat: Option<DraftIdentity>,
) -> Result<Option<ComposerModelSelection>, CommandError> {
    let Some(chat) = chat else {
        let db = state.store.lock().map_err(map_store)?;
        return store::last_composer_model(&db).map_err(map_store);
    };
    validate_chat(&chat)?;
    state.ensure_loupist_available(&chat.loupist_id)?;
    with_draft_chat(state, &chat, |db| {
        initialize(db).map_err(map_store)?;
        let raw: Option<String> = db
            .query_row(
                "SELECT data_json FROM composer_model_selections WHERE loupist_id=?1 AND thread_id=?2",
                [&chat.loupist_id, &chat.thread_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| map_store(error.into()))?;
        raw.as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|error| map_store(error.into()))
    })
}

#[tauri::command(async)]
pub(crate) fn chat_composer_model_write(
    request: ComposerModelWrite,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    write(&state, request)
}

fn write(state: &AppState, request: ComposerModelWrite) -> Result<(), CommandError> {
    match request {
        ComposerModelWrite::Set {
            chat,
            selection,
            last_sent,
        } => {
            validate_chat(&chat)?;
            for value in selection.iter().chain(&last_sent) {
                value.validate()?;
            }
            state.ensure_loupist_available(&chat.loupist_id)?;
            with_draft_chat(state, &chat, |db| {
                set(db, &chat, selection.as_ref(), last_sent.as_ref()).map_err(map_store)
            })
        }
        ComposerModelWrite::Reset { loupist_id } => {
            if let Some(id) = &loupist_id {
                text(id, 256, false)?;
            }
            let mut db = state.store.lock().map_err(map_store)?;
            reset(&mut db, loupist_id.as_deref()).map_err(map_store)
        }
    }
}

fn set(
    db: &mut Connection,
    chat: &DraftIdentity,
    selection: Option<&ComposerModelSelection>,
    last_sent: Option<&ComposerModelSelection>,
) -> StoreResult<()> {
    let tx = db.transaction()?;
    initialize(&tx)?;
    if let Some(selection) = selection {
        tx.execute(
            "INSERT INTO composer_model_selections VALUES (?1,?2,?3)
             ON CONFLICT(loupist_id,thread_id) DO UPDATE SET data_json=excluded.data_json",
            params![
                chat.loupist_id,
                chat.thread_id,
                serde_json::to_string(selection)?
            ],
        )?;
    } else {
        tx.execute(
            "DELETE FROM composer_model_selections WHERE loupist_id=?1 AND thread_id=?2",
            [&chat.loupist_id, &chat.thread_id],
        )?;
    }
    if let Some(last_sent) = last_sent {
        store::put_last_composer_model(&tx, Some(last_sent))?;
    }
    tx.commit()?;
    Ok(())
}

fn reset(db: &mut Connection, loupist_id: Option<&str>) -> StoreResult<()> {
    let tx = db.transaction()?;
    initialize(&tx)?;
    if let Some(id) = loupist_id {
        tx.execute(
            "DELETE FROM composer_model_selections WHERE loupist_id=?1",
            [id],
        )?;
    } else {
        tx.execute("DELETE FROM composer_model_selections", [])?;
    }
    store::put_last_composer_model(&tx, None)?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::{chat_session, create_folder_loupist};

    #[test]
    fn composer_model_choices_survive_reopen_and_default_clears_the_choice() {
        let temp = tempfile::tempdir().unwrap();
        let profile = temp.path().join("profile");
        let workspace = temp.path().join("workspace");
        let state = AppState::for_folder_test(profile.clone());
        create_folder_loupist(&state.store, "loupist", &workspace);
        for thread in ["chat-a", "chat-b"] {
            state
                .store
                .create_chat(&chat_session(thread, "loupist"))
                .unwrap();
        }
        let identity = |thread: &str| DraftIdentity {
            loupist_id: "loupist".into(),
            thread_id: thread.into(),
        };
        let first = ComposerModelSelection {
            model_profile_id: "model-a".into(),
            thinking_level: "high".into(),
            run_options: BTreeMap::from([("performance".into(), "fast".into())]),
        };
        let second = ComposerModelSelection {
            model_profile_id: "model-b".into(),
            thinking_level: "low".into(),
            run_options: BTreeMap::new(),
        };
        assert_eq!(read(&state, Some(identity("chat-a"))).unwrap(), None);
        let choose = |state: &AppState, thread, selection, last_sent| {
            write(
                state,
                ComposerModelWrite::Set {
                    chat: identity(thread),
                    selection,
                    last_sent,
                },
            )
        };
        choose(&state, "chat-a", Some(first.clone()), None).unwrap();
        choose(&state, "chat-b", Some(second.clone()), Some(second.clone())).unwrap();
        assert!(choose(&state, "missing", Some(first.clone()), Some(first.clone())).is_err());
        drop(state);

        let reopened = AppState::for_folder_test(profile);
        assert_eq!(
            read(&reopened, Some(identity("chat-a"))).unwrap(),
            Some(first.clone())
        );
        assert_eq!(
            read(&reopened, Some(identity("chat-b"))).unwrap(),
            Some(second.clone())
        );
        assert_eq!(
            read(&reopened, Some(identity("chat-a"))).unwrap(),
            Some(first)
        );
        assert_eq!(read(&reopened, None).unwrap(), Some(second.clone()));
        choose(&reopened, "chat-a", None, Some(second.clone())).unwrap();
        assert_eq!(read(&reopened, Some(identity("chat-a"))).unwrap(), None);
        assert_eq!(
            read(&reopened, Some(identity("chat-b"))).unwrap(),
            Some(second)
        );
        write(
            &reopened,
            ComposerModelWrite::Reset {
                loupist_id: Some("loupist".into()),
            },
        )
        .unwrap();
        assert_eq!(read(&reopened, Some(identity("chat-b"))).unwrap(), None);
        assert_eq!(read(&reopened, None).unwrap(), None);
    }
}
