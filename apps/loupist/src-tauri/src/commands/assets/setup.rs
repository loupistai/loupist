//! A delegated, bounded Chat for assets that need a saved read.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use serde_json::json;

use crate::{
    app::{AppState, CommandError},
    commands::{
        chat_runtime::{send_chat, ChatAdmission, ChatEventSink},
        map_store, new_id, now_ms,
        types::{ChatSendRequest, ChatStreamEvent},
    },
    domain::{ChatSession, MemoryPolicy},
    knowledge::assets::saved_read,
};

use super::{
    changed, replay_current,
    wire::{AssetRow, PriceReadState},
    with_assets,
};

const PROMPT: &str = include_str!("../../../../agent/prompts/price-setup.md");
const BURST: Duration = Duration::from_secs(3);
const MAX_ASSETS: usize = 20;
/// Price-setup sessions are host work, so an asset keeps only its latest two:
/// the one that set up (or last tried to set up) its current read, and the one
/// before it to compare against when a read that worked breaks and a new
/// setup fails. Setup runs only for an asset without a working read, so an
/// older session is an attempt that a later one replaced.
const SESSIONS_KEPT_PER_ASSET: usize = 2;

#[derive(Clone, Default)]
pub(crate) struct PriceSetup {
    bursts: Arc<Mutex<BTreeMap<String, Burst>>>,
    pending: Arc<Mutex<BTreeMap<String, BTreeSet<String>>>>,
}

#[derive(Default)]
struct Burst {
    revision: uuid::Uuid,
    assets: BTreeSet<String>,
}

struct PendingSetup {
    setup: PriceSetup,
    loupist: String,
    assets: BTreeSet<String>,
}

impl Drop for PendingSetup {
    fn drop(&mut self) {
        let mut pending = self.setup.pending.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(assets) = pending.get_mut(&self.loupist) {
            assets.retain(|asset| !self.assets.contains(asset));
            if assets.is_empty() {
                pending.remove(&self.loupist);
            }
        }
        drop(pending);
        changed();
    }
}

impl PriceSetup {
    pub(super) fn is_running(&self, loupist: &str, asset: &str) -> bool {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(loupist)
            .is_some_and(|assets| assets.contains(asset))
    }

    pub(super) fn queue(&self, state: &AppState, loupist: &str, asset: &str) {
        let revision = {
            let Ok(mut pending) = self.bursts.lock() else {
                return;
            };
            let burst = pending.entry(loupist.to_owned()).or_default();
            burst.revision = uuid::Uuid::new_v4();
            burst.assets.insert(asset.to_owned());
            burst.revision
        };
        let state = state.clone();
        let loupist = loupist.to_owned();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(BURST).await;
            let assets = {
                let Ok(mut pending) = state.price_setup.bursts.lock() else {
                    return;
                };
                if pending
                    .get(&loupist)
                    .is_none_or(|burst| burst.revision != revision)
                {
                    return;
                }
                pending.remove(&loupist).expect("checked burst").assets
            };
            if let Err(error) = launch(&state, &loupist, &assets, false).await {
                eprintln!("Asset price setup could not start: {}", error.code);
            }
        });
    }
}

pub(super) async fn launch(
    state: &AppState,
    loupist: &str,
    ids: &BTreeSet<String>,
    retry_failed: bool,
) -> Result<(), CommandError> {
    // Only what this Loupist still follows gets a setup.
    let rows = state.store.assets_list(loupist, true).map_err(map_store)?;
    let (reservation, candidates) = {
        let mut pending = state
            .price_setup
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let in_flight = pending.get(loupist);
        let candidates = with_assets(state, loupist, false, |tx| {
            let mut candidates = Vec::new();
            for row in rows.into_iter().filter(|row| {
                ids.contains(&row.asset_id)
                    && !in_flight.is_some_and(|assets| assets.contains(&row.asset_id))
            }) {
                let status = saved_read::load(tx, loupist, &row.asset_id)?
                    .map_or(PriceReadState::None, |saved| saved.read.state);
                if matches!(status, PriceReadState::None | PriceReadState::Failed) {
                    candidates.push((row, status));
                }
                if candidates.len() >= MAX_ASSETS {
                    break;
                }
            }
            Ok(candidates)
        })?;
        if candidates.is_empty() {
            return Ok(());
        }
        let assets: BTreeSet<_> = candidates
            .iter()
            .map(|(row, _)| row.asset_id.clone())
            .collect();
        pending
            .entry(loupist.into())
            .or_default()
            .extend(assets.iter().cloned());
        (
            PendingSetup {
                setup: state.price_setup.clone(),
                loupist: loupist.into(),
                assets,
            },
            candidates,
        )
    };
    changed();
    let mut assets = Vec::new();
    for (row, status) in candidates {
        if retry_failed && status == PriceReadState::Failed {
            replay_current(state, loupist, &row.asset_id, None).await?;
            let read = with_assets(state, loupist, false, |tx| {
                saved_read::load(tx, loupist, &row.asset_id)
            })?;
            if read.is_some_and(|saved| saved.read.state != PriceReadState::Failed) {
                continue;
            }
        }
        assets.push(row);
    }
    if assets.is_empty() {
        return Ok(());
    }
    let config = crate::commands::loupist_configuration(state, loupist)?;
    let mut model = config.chat.model_profile_id.unwrap_or_default();
    let mut thinking = config.chat.thinking_level.unwrap_or_default();
    let mut run_options = config.chat.run_options;
    // A Chat default that cannot run gives way to Auto, as in Chat.
    if let Some(auto) =
        crate::providers::auto::stored_fallback(state, &model, &thinking, &run_options)
    {
        (model, thinking, run_options) = (auto.model.id, auto.thinking_level, auto.run_options);
    }
    let execution = state.pi_execution(&model, &thinking, &run_options)?;
    // An explicit model choice can still be configured after its account signs out.
    if crate::providers::catalog::provider_credential_kind(
        &state.pi_agent_data_dir()?,
        &execution.model.provider,
    )?
    .is_none()
    {
        return Err(CommandError::new("signed_out", "Connect an AI account."));
    }
    let mut language = config.appearance.language;
    crate::agent_harness::interface::resolve_language(&mut language);
    let at = now_ms();
    let chat = ChatSession {
        id: new_id("price-setup"),
        loupist_id: loupist.to_owned(),
        pi_session_id: uuid::Uuid::new_v4().to_string(),
        pi_session_cache: None,
        title: "Price setup".into(),
        title_provisional: false,
        memory_policy: MemoryPolicy {
            use_memory: true,
            update_memory: true,
        },
        set_aside_memory: Vec::new(),
        versions: Vec::new(),
        messages: Vec::new(),
        created_at_ms: at,
        updated_at_ms: at,
        bot_memory: Default::default(),
    };
    let run_id = new_id("price-setup");
    let mut admission = ChatAdmission::new(state, &run_id, loupist, &chat.id)?;
    admission.price_setup = true;
    state.store.create_chat(&chat).map_err(map_store)?;
    let request = ChatSendRequest {
        run_id: run_id.clone(),
        loupist_id: loupist.into(),
        thread_id: chat.id,
        prompt: setup_request(&language, &assets),
        use_memory: true,
        update_memory: true,
        as_of: None,
        model_profile_id: model,
        thinking_level: thinking,
        run_options,
        mentioned_connection_ids: Vec::new(),
        attachments: Vec::new(),
        pins: Vec::new(),
        selected_artifact: None,
    };
    let state = state.clone();
    let loupist = loupist.to_owned();
    tauri::async_runtime::spawn(async move {
        let (terminal, settled) = tokio::sync::oneshot::channel();
        let terminal = Mutex::new(Some(terminal));
        let mut sink = ChatEventSink::new(move |event| {
            if matches!(
                event,
                ChatStreamEvent::Completed { .. }
                    | ChatStreamEvent::Aborted { .. }
                    | ChatStreamEvent::Error { .. }
            ) {
                if let Some(terminal) = terminal.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    let _ = terminal.send(event);
                }
            }
            Ok(())
        });
        let connect_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        sink.price_setup_connect_failed = Some(connect_failed.clone());
        let reply = match send_chat(request, sink, &state, Some(admission)).await {
            Ok(_) => match settled.await {
                Ok(
                    ChatStreamEvent::Completed { final_text, .. }
                    | ChatStreamEvent::Error {
                        final_text,
                        placeholder: false,
                        ..
                    },
                ) => Some(final_text),
                Ok(ChatStreamEvent::Aborted { .. }) => return,
                _ => None,
            },
            Err(error) => {
                eprintln!("Asset price setup failed: {}", error.code);
                None
            }
        };
        let report = report_missing_prices(
            &state,
            &loupist,
            &run_id,
            &assets,
            &language,
            reply.as_deref(),
            connect_failed.load(std::sync::atomic::Ordering::Relaxed),
        );
        drop(reservation);
        if let Err(error) = report {
            eprintln!(
                "Asset price setup could not report its result: {}",
                error.code
            );
        }
        release_sessions(&state, &loupist);
    });
    Ok(())
}

/// The setup request: the host's instructions, then one line of JSON naming
/// the assets, which [`request_assets`] reads back.
fn setup_request(language: &str, assets: &[AssetRow]) -> String {
    let assets = assets
        .iter()
        .map(|row| json!({"assetId":row.asset_id,"name":row.name,"reference":row.reference}))
        .collect::<Vec<_>>();
    format!(
        "{PROMPT}\n{}",
        json!({"appLanguage":language,"assets":assets})
    )
}

/// The asset ids a stored setup request names, from its last line.
fn request_assets(request: &str) -> Option<Vec<String>> {
    let (_, line) = request.rsplit_once('\n')?;
    let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
    value
        .get("assets")?
        .as_array()?
        .iter()
        .map(|asset| asset.get("assetId")?.as_str().map(str::to_owned))
        .collect()
}

/// Deletes the price-setup sessions no asset keeps any more, through the same
/// path as the user's Delete: each asset keeps its latest
/// [`SESSIONS_KEPT_PER_ASSET`], and a session the user wrote in (any message
/// after the host's request) always stays. A session with no request at all
/// set nothing up and goes; one whose request cannot be read is left alone. A
/// session that cannot go now (a turn is running in it) waits for the next
/// setup or startup. Returns how many went.
pub(crate) fn release_sessions(state: &AppState, loupist: &str) -> usize {
    let released = match released_sessions(state, loupist) {
        Ok(released) => released,
        Err(_) => {
            eprintln!("Price setup sessions could not be read for release");
            return 0;
        }
    };
    let mut count = 0;
    for thread_id in released {
        // Checked again while the session is held: the user may have written in it.
        let unwritten = |chat: &ChatSession| {
            let requests = chat
                .messages
                .iter()
                .filter(|message| message.role == crate::domain::ChatRole::User);
            if requests.count() > 1 {
                Err(CommandError::conflict("the user wrote in this session"))
            } else {
                Ok(())
            }
        };
        match crate::commands::records::delete_chat_session(state, loupist, &thread_id, unwritten) {
            Ok(()) => count += 1,
            Err(error) => eprintln!("A price setup session was kept for now: {}", error.code),
        }
    }
    count
}

fn released_sessions(state: &AppState, loupist: &str) -> crate::store::StoreResult<Vec<String>> {
    let connection = state.store.lock_loupist(loupist)?;
    let mut statement = connection.prepare(
        "SELECT id,
                (SELECT count(*) FROM json_each(data_json,'$.messages') WHERE json_extract(value,'$.role')='user'),
                (SELECT json_extract(value,'$.content') FROM json_each(data_json,'$.messages')
                 WHERE json_extract(value,'$.role')='user' ORDER BY key LIMIT 1)
         FROM chat_sessions WHERE loupist_id=?1 AND id GLOB 'price-setup-*'
         ORDER BY json_extract(data_json,'$.createdAtMs') DESC, id DESC",
    )?;
    let sessions = statement
        .query_map([loupist], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut kept = BTreeMap::<String, usize>::new();
    let mut released = Vec::new();
    for (id, requests, request) in sessions {
        let Some(request) = request else {
            released.push(id);
            continue;
        };
        let Some(assets) = request_assets(&request) else {
            continue;
        };
        let mut latest = false;
        for asset in assets {
            let count = kept.entry(asset).or_default();
            *count += 1;
            latest |= *count <= SESSIONS_KEPT_PER_ASSET;
        }
        if !latest && requests <= 1 {
            released.push(id);
        }
    }
    Ok(released)
}

fn report_missing_prices(
    state: &AppState,
    loupist: &str,
    run_id: &str,
    assets: &[AssetRow],
    language: &str,
    reply: Option<&str>,
    platform_unavailable: bool,
) -> Result<(), CommandError> {
    let missing = with_assets(state, loupist, false, |tx| {
        let mut names = Vec::new();
        for asset in assets {
            let saved = saved_read::load(tx, loupist, &asset.asset_id)?;
            if saved.is_none_or(|saved| saved.read.state != PriceReadState::Ready) {
                names.push(asset.name.as_str());
            }
        }
        Ok(names)
    })?;
    if missing.is_empty() {
        return Ok(());
    }
    let content = if platform_unavailable {
        let names = missing.join(", ");
        let reason = if language == "ko" {
            format!("시장 데이터 플랫폼에 연결하지 못해 {names}의 가격을 가져오지 못했습니다.")
        } else {
            format!("The market data platform could not connect to look up a price for {names}.")
        };
        match reply.filter(|text| !text.trim().is_empty()) {
            Some(detail) => format!("{reason}\n\n{detail}"),
            None => reason,
        }
    } else {
        reply.filter(|text| !text.trim().is_empty()).map_or_else(
            || {
                let names = missing.join(", ");
                if language == "ko" {
                    format!("{names}의 가격을 찾지 못했습니다.")
                } else {
                    format!("Could not find a price for {names}.")
                }
            },
            str::to_owned,
        )
    };
    crate::chat::home::queue_price_setup_reply(state, loupist, run_id, &content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        chat::home::{deliver_finished_runs, ensure_home_thread},
        runs::coordinator::{RunKind, RunTarget},
    };

    #[test]
    fn price_setup_posts_one_reply_if_any_read_is_missing_and_survives_a_busy_bot() {
        let root = tempfile::tempdir().unwrap();
        let app_data = root.path().join("app");
        let state = AppState::for_folder_test(app_data.clone());
        let loupist = "loupist-a";
        crate::commands::tests::seed_profile(
            &state.store,
            &crate::commands::tests::profile(loupist, &root.path().join("workspace"), 1),
        );
        let samsung = state
            .store
            .seed_followed_listing(loupist, "005930", "KRX", Some("삼성전자"))
            .unwrap();
        let nvidia = state
            .store
            .seed_followed_listing(loupist, "NVDA", "NASDAQ", Some("Nvidia"))
            .unwrap();
        with_assets(&state, loupist, true, |tx| {
            crate::knowledge::assets::ensure_asset_page(tx, loupist, &nvidia, now_ms())?;
            saved_read::save(
                tx,
                loupist,
                &saved_read::StagedPriceRead {
                    asset_id: nvidia.asset_id.clone(),
                    read: serde_json::from_value(json!({
                        "state":"ready",
                        "current":{
                            "tool":"quote",
                            "arguments":{"symbol":"NVDA"},
                            "fields":{"value":"/value","date":"/date","currency":"/currency"}
                        }
                    }))?,
                    price: serde_json::from_value(json!({
                        "value":159.0,
                        "date":"2026-11-18",
                        "currency":"USD",
                        "provider":"Yahoo Finance",
                        "dataset":{"dataset_id":"fixture","revision":1,"digest":"fixture"}
                    }))?,
                    history: Vec::new(),
                },
            )
        })
        .unwrap();

        let home = ensure_home_thread(&state, loupist).unwrap();
        report_missing_prices(
            &state,
            loupist,
            "price-setup-ready",
            std::slice::from_ref(&nvidia),
            "en",
            Some("Nvidia: saved."),
            true,
        )
        .unwrap();
        assert!(ensure_home_thread(&state, loupist)
            .unwrap()
            .messages
            .is_empty());

        let running = state
            .register_run(
                "bot-turn".into(),
                loupist.into(),
                RunKind::Chat,
                RunTarget::ChatThread(home.id),
            )
            .unwrap();
        let assets = [samsung, nvidia];
        let reply = "삼성전자: 시세 기준일을 확인할 수 없습니다.\nNvidia: saved.";
        report_missing_prices(
            &state,
            loupist,
            "price-setup-mixed",
            &assets,
            "ko",
            Some(reply),
            false,
        )
        .unwrap();
        assert!(ensure_home_thread(&state, loupist)
            .unwrap()
            .messages
            .is_empty());

        drop(running);
        state.close_for_restart_test();
        let state = AppState::for_folder_test(app_data);
        assert_eq!(deliver_finished_runs(&state, loupist).unwrap(), 1);
        assert_eq!(deliver_finished_runs(&state, loupist).unwrap(), 0);
        let messages = ensure_home_thread(&state, loupist).unwrap().messages;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, reply);

        report_missing_prices(
            &state,
            loupist,
            "platform-unavailable",
            &assets,
            "en",
            None,
            true,
        )
        .unwrap();
        let messages = ensure_home_thread(&state, loupist).unwrap().messages;
        assert_eq!(
            messages.last().unwrap().content,
            "The market data platform could not connect to look up a price for 삼성전자."
        );

        for (language, expected) in [
            ("en", "Could not find a price for 삼성전자."),
            ("ko", "삼성전자의 가격을 찾지 못했습니다."),
        ] {
            let id = format!("price-setup-failed-{language}");
            report_missing_prices(&state, loupist, &id, &assets, language, None, false).unwrap();
            let messages = ensure_home_thread(&state, loupist).unwrap().messages;
            assert_eq!(messages.last().unwrap().content, expected);
        }
    }

    #[tokio::test]
    async fn price_setup_skips_in_flight_assets_per_loupist_and_releases_them() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        crate::commands::tests::seed_profile(
            &state.store,
            &crate::commands::tests::profile("loupist-a", root.path(), 1),
        );
        let asset = state
            .store
            .seed_followed_listing("loupist-a", "005930", "KRX", Some("Samsung"))
            .unwrap();
        let assets = BTreeSet::from([asset.asset_id.clone()]);
        state
            .price_setup
            .pending
            .lock()
            .unwrap()
            .insert("loupist-a".into(), assets.clone());
        let reservation = PendingSetup {
            setup: state.price_setup.clone(),
            loupist: "loupist-a".into(),
            assets: assets.clone(),
        };

        assert!(state.price_setup.is_running("loupist-a", &asset.asset_id));
        assert!(!state.price_setup.is_running("loupist-b", &asset.asset_id));
        // There is no configured model, so success proves we skipped launch.
        launch(&state, "loupist-a", &assets, true).await.unwrap();
        drop(reservation);
        assert!(!state.price_setup.is_running("loupist-a", &asset.asset_id));
        assert!(launch(&state, "loupist-a", &assets, true).await.is_err());
        assert!(!state.price_setup.is_running("loupist-a", &asset.asset_id));
    }

    #[tokio::test]
    async fn each_asset_keeps_its_latest_two_setup_sessions_and_any_the_user_wrote_in() {
        use crate::{domain::ChatRole, test_fixtures::chat_message};
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let loupist = "loupist-a";
        crate::commands::tests::seed_profile(
            &state.store,
            &crate::commands::tests::profile(loupist, root.path(), 1),
        );
        let row = |id: &str| AssetRow {
            asset_id: id.into(),
            name: id.to_uppercase(),
            reference: None,
            followed_at_ms: None,
            created_at_ms: 0,
        };
        let session = |id: &str, at: i64, request: Option<String>, written: bool| {
            let mut chat = crate::commands::tests::chat(id, loupist, at);
            if let Some(request) = request {
                chat.messages.push(chat_message(
                    &format!("{id}-request"),
                    ChatRole::User,
                    &request,
                ));
                chat.messages.push(chat_message(
                    &format!("{id}-answer"),
                    ChatRole::Assistant,
                    "Saved a price read.",
                ));
            }
            if written {
                chat.messages.push(chat_message(
                    &format!("{id}-reply"),
                    ChatRole::User,
                    "Use the KRX close instead.",
                ));
            }
            state.store.create_chat(&chat).unwrap();
        };
        let asks = |ids: &[&str]| {
            let rows = ids.iter().map(|id| row(id)).collect::<Vec<_>>();
            let request = setup_request("en", &rows);
            assert_eq!(
                request_assets(&request),
                Some(ids.iter().map(|id| id.to_string()).collect())
            );
            Some(request)
        };
        session("price-setup-written", 1, asks(&["a"]), true);
        session("price-setup-oldest", 2, asks(&["a"]), false);
        session("price-setup-only-b", 3, asks(&["a", "b"]), false);
        session("price-setup-second", 4, asks(&["a"]), false);
        session("price-setup-latest", 5, asks(&["a"]), false);
        session("price-setup-empty", 6, None, false);
        session("price-setup-unread", 0, Some("Find a price.".into()), false);
        // Another chat with a setup-looking request is not price setup.
        session("chat-a", 0, asks(&["a"]), false);

        assert_eq!(release_sessions(&state, loupist), 2);
        let left = state
            .store
            .list_chats_for_loupist(loupist)
            .unwrap()
            .into_iter()
            .map(|chat| chat.id)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            left,
            BTreeSet::from(
                [
                    "chat-a",
                    "price-setup-latest",
                    "price-setup-only-b",
                    "price-setup-second",
                    "price-setup-unread",
                    "price-setup-written",
                ]
                .map(String::from)
            )
        );
        assert_eq!(release_sessions(&state, loupist), 0);
    }

    #[tokio::test]
    async fn price_setup_runs_only_for_what_its_loupist_follows() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        for id in ["loupist-a", "loupist-b"] {
            crate::commands::tests::seed_profile(
                &state.store,
                &crate::commands::tests::profile(id, &root.path().join(id), 1),
            );
        }
        let asset = state
            .store
            .seed_followed_listing("loupist-a", "005930", "KRX", Some("Samsung"))
            .unwrap();
        let assets = BTreeSet::from([asset.asset_id.clone()]);
        // B does not follow it, so nothing starts; there is no model, so A's attempt fails.
        launch(&state, "loupist-b", &assets, true).await.unwrap();
        assert!(launch(&state, "loupist-a", &assets, true).await.is_err());
    }
}
