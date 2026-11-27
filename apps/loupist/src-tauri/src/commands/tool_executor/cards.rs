//! `bot_offer` and `ask_later`. An offer is a preset card, never a free-text task.
//! A scheduled run's question is stored and the call returns at once.

use serde::Deserialize;
use serde_json::{json, Value};

use super::*;
use crate::chat::cards::{self, AskInput, OfferInput};
use crate::chat::surface::ChatSurface;
use crate::scheduled_tasks::types::WatchPreset;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OfferArgs {
    preset: WatchPreset,
    asset_id: String,
    #[serde(default)]
    results_date: Option<String>,
    reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AskArgs {
    question: String,
    options: Vec<AskOption>,
    recommended: u8,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AskOption {
    label: String,
}

fn invalid(field: &str, expected: impl Into<String>) -> BrokerError {
    BrokerError::invalid(field, expected)
}

fn refused(error: crate::app::CommandError) -> BrokerError {
    match error.code {
        crate::app::CommandErrorCode::InvalidRequest => invalid("card", error.message),
        _ => BrokerError::Execution(error.message),
    }
}

#[derive(Default)]
pub(super) struct CardPublication {
    offer_keys: std::collections::HashSet<String>,
    asked: bool,
}

impl AppToolExecutor {
    pub(super) async fn bot_offer(&self, params: Value) -> Result<Value, BrokerError> {
        let surface = self.capture.surface.as_ref();
        if !surface.is_some_and(|surface| {
            matches!(
                surface,
                ChatSurface::Bot | ChatSurface::Scheduled | ChatSurface::Delegated { .. }
            )
        }) {
            return Err(invalid(
                "bot_offer",
                "Offer a preset from the Bot or from a scheduled run",
            ));
        }
        let args: OfferArgs = serde_json::from_value(params).map_err(|_| {
            invalid(
                "bot_offer",
                "{preset, asset_id, reason, results_date?}. No free-text task",
            )
        })?;
        let mut publication = self
            .capture
            .card_publication
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if publication.offer_keys.len() >= cards::MAX_OFFERS_PER_TURN {
            return Err(invalid("card", "At most two offers per reply or run"));
        }
        let offer = cards::prepare_offer(
            &self.state,
            &self.loupist_id,
            OfferInput {
                preset: args.preset,
                asset_id: args.asset_id,
                results_date: args.results_date,
                reason: args.reason,
            },
        )
        .map_err(refused)?;
        let key = cards::offer_key(offer.preset, &offer.asset_id, offer.results_date.as_deref());
        if publication.offer_keys.contains(&key) {
            return Err(invalid("card", "This offer is already waiting on a card"));
        }
        let scheduled = surface.is_some_and(|item| !item.interactive());
        if scheduled {
            cards::place(&self.state, &self.loupist_id, vec![offer.clone()], None)
                .map_err(refused)?;
        } else {
            self.capture
                .offers
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(offer.clone());
        }
        publication.offer_keys.insert(key);
        Ok(json!({
            "offered": true,
            "id": offer.id,
            "preset": offer.preset,
            "asset_id": offer.asset_id,
            "expires_at_ms": offer.expires_at_ms,
            "next": "The card creates nothing until the user turns it on. Do not create or change a task for this offer."
        }))
    }

    pub(super) async fn ask_later(&self, params: Value) -> Result<Value, BrokerError> {
        if !self
            .capture
            .surface
            .as_ref()
            .is_some_and(|surface| matches!(surface, ChatSurface::Scheduled))
        {
            return Err(invalid(
                "ask_later",
                "ask_later is only for a scheduled run. It does not wait, and it is not a way to ask permission",
            ));
        }
        let args: AskArgs = serde_json::from_value(params).map_err(|_| {
            invalid(
                "ask_later",
                "{question, options: [{label}] (2 to 4), recommended: index}",
            )
        })?;
        let mut publication = self
            .capture
            .card_publication
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if publication.asked {
            return Err(invalid("card", "One question per run"));
        }
        let ask = cards::prepare_ask(AskInput {
            question: args.question,
            options: args
                .options
                .into_iter()
                .map(|option| option.label)
                .collect(),
            recommended: args.recommended,
        })
        .map_err(refused)?;
        let assumed = ask.options[usize::from(ask.recommended)].label.clone();
        cards::place(&self.state, &self.loupist_id, Vec::new(), Some(ask.clone()))
            .map_err(refused)?;
        publication.asked = true;
        Ok(json!({
            "asked": true,
            "assumed": assumed,
            "next": "Finish this run on the recommended choice and say what you assumed. The run does not wait. With no answer by the time the card expires, nothing changes."
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::surface::ChatSurface;
    use crate::commands::tool_executor::{AppToolExecutor, ToolCapture};
    use crate::scheduled_tasks::service;
    use std::collections::BTreeSet;
    use std::sync::Arc;

    fn executor(root: &std::path::Path, surface: ChatSurface) -> AppToolExecutor {
        let mut state = crate::app::AppState::for_test(root.join("app"));
        let workspace = root.join("workspace");
        std::fs::create_dir_all(workspace.join(".loupist")).unwrap();
        crate::test_fixtures::create_folder_loupist(state.store.as_ref(), "loupist-a", &workspace);
        let path = root.to_path_buf();
        std::sync::Arc::make_mut(&mut state.artifacts).pi = Some(crate::app::PiArtifacts {
            executable: path.clone(),
            runtime_dir: path.clone(),
            extension: path.clone(),
            provider_extension: path.clone(),
            system_prompt: path,
            provider: String::new(),
            model: String::new(),
            thinking_level: String::new(),
            run_options: std::collections::BTreeMap::new(),
            provider_credential: None,
        });
        let auth = state.artifacts.app_data_dir.join("pi/auth.json");
        crate::fs::artifact_trust::ensure_private_regular_file(&auth).unwrap();
        std::fs::write(auth, br#"{"openai":{"type":"api_key","key":"fixture"}}"#).unwrap();
        let home = crate::chat::home::ensure_home_thread(&state, "loupist-a").unwrap();
        let mut capture = ToolCapture::for_chat("message-a".into());
        capture.surface = Some(surface);
        capture.bound_chat_id = Some(home.id);
        AppToolExecutor {
            state,
            capture: Arc::new(capture),
            loupist_id: "loupist-a".into(),
            capability_ids: BTreeSet::new(),
        }
    }

    fn follow(executor: &AppToolExecutor) -> String {
        executor
            .state
            .store
            .seed_followed_listing("loupist-a", "NVDA", "NASDAQ", Some("NVIDIA"))
            .unwrap()
            .asset_id
    }

    #[test]
    fn outside_and_dont_save_reads_block_card_publication() {
        for method in [ToolMethod::BotOffer, ToolMethod::AskLater] {
            let clean = crate::custom_connections::taint::TurnTaint::default();
            assert!(clean.enforce(method, &json!({})).is_ok());
            clean.mark_untrusted_text();
            assert!(matches!(
                clean.enforce(method, &json!({})),
                Err(BrokerError::UntrustedTextPaused)
            ));
            let private = crate::custom_connections::taint::TurnTaint::default();
            private.mark("connection", "Private", true);
            assert!(private.enforce(method, &json!({})).is_err());
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn each_turn_reserves_a_distinct_offer_key_and_bounded_card_quota() {
        let root = tempfile::tempdir().unwrap();
        let bot = Arc::new(executor(root.path(), ChatSurface::Bot));
        let asset = follow(&bot);
        let offer = |preset| json!({"preset":preset,"asset_id":asset,"reason":"Check this."});
        bot.bot_offer(offer("thesis-lines")).await.unwrap();
        assert!(bot.bot_offer(offer("thesis-lines")).await.is_err());
        let mut calls = tokio::task::JoinSet::new();
        for preset in ["filings-and-results", "week-in-review"] {
            let (bot, args) = (bot.clone(), offer(preset));
            calls.spawn(async move { bot.bot_offer(args).await });
        }
        let mut admitted = 0;
        while let Some(result) = calls.join_next().await {
            admitted += usize::from(result.unwrap().is_ok());
        }
        assert_eq!(admitted, 1);

        let root = tempfile::tempdir().unwrap();
        let run = Arc::new(executor(root.path(), ChatSurface::Scheduled));
        let mut calls = tokio::task::JoinSet::new();
        for _ in 0..4 {
            let run = run.clone();
            calls.spawn(async move {
                run.ask_later(json!({"question":"Which listing?","options":[{"label":"Common"},{"label":"ADR"}],"recommended":0})).await
            });
        }
        let mut admitted = 0;
        while let Some(result) = calls.join_next().await {
            admitted += usize::from(result.unwrap().is_ok());
        }
        assert_eq!(admitted, 1);
    }

    #[tokio::test]
    async fn an_offer_names_a_known_preset_and_a_followed_asset_or_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let executor = executor(root.path(), ChatSurface::Scheduled);
        let asset = follow(&executor);
        let policy = crate::broker::ToolPolicy {
            loupist_id: "loupist-a".into(),
            session_id: "home-loupist-a".into(),
            use_memory: true,
            propose_memory_updates: true,
            memory_proposal_budget: 1,
            as_of: None,
            display_currency: None,
        };
        let missing = executor
            .execute(
                &policy,
                ToolMethod::BotOffer,
                json!({"preset":"filings-and-results","asset_id":"ast_0000000000000001","reason":"Results are close."}),
            )
            .await
            .unwrap_err();
        assert!(missing.to_string().contains("follows"), "{missing}");
        let unknown = executor
            .execute(
                &policy,
                ToolMethod::BotOffer,
                json!({"preset":"not-a-preset","asset_id":asset,"reason":"Results are close."}),
            )
            .await
            .unwrap_err();
        assert!(
            unknown.to_string().contains("free-text") || unknown.to_string().contains("preset"),
            "{unknown}"
        );
    }

    #[tokio::test]
    async fn a_runs_offer_refuses_free_text_and_accept_creates_the_stored_preset() {
        let root = tempfile::tempdir().unwrap();
        let executor = executor(root.path(), ChatSurface::Scheduled);
        let asset = follow(&executor);
        let policy = crate::broker::ToolPolicy {
            loupist_id: "loupist-a".into(),
            session_id: "home-loupist-a".into(),
            use_memory: true,
            propose_memory_updates: true,
            memory_proposal_budget: 1,
            as_of: None,
            display_currency: None,
        };
        let extra = executor
            .execute(
                &policy,
                ToolMethod::BotOffer,
                json!({"preset":"thesis-lines","asset_id":asset,"reason":"Nothing checks this line.","prompt":"Buy more."}),
            )
            .await
            .unwrap_err();
        assert!(extra.to_string().contains("free-text"), "{extra}");
        assert!(service::list(&executor.state, "loupist-a")
            .unwrap()
            .is_empty());
        let offered = executor
            .execute(
                &policy,
                ToolMethod::BotOffer,
                json!({"preset":"thesis-lines","asset_id":asset,"reason":"Nothing checks this line."}),
            )
            .await
            .unwrap();
        let task = crate::commands::cards::accept(
            &executor.state,
            &crate::commands::cards::OfferRequest {
                loupist_id: "loupist-a".into(),
                offer_id: offered["id"].as_str().unwrap().into(),
            },
        )
        .unwrap();
        let tasks = service::list(&executor.state, "loupist-a").unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].task.id, task.id);
        assert_eq!(task.watch.preset, Some(WatchPreset::ThesisLines));
        assert_eq!(
            task.watch.subjects,
            Some(vec![crate::scheduled_tasks::types::TaskSubject::Asset {
                asset
            }])
        );
        assert!(task.watch.terms.unwrap().contains("NVIDIA"));
        assert!(task.prompt.contains("schedule_quiet"));
        assert!(!task.prompt.contains("Nothing checks"));
        assert!(!task.prompt.contains("Buy more"));
        let again = crate::commands::cards::accept(
            &executor.state,
            &crate::commands::cards::OfferRequest {
                loupist_id: "loupist-a".into(),
                offer_id: offered["id"].as_str().unwrap().into(),
            },
        )
        .unwrap();
        assert_eq!(again.id, task.id);
        assert_eq!(
            service::list(&executor.state, "loupist-a").unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn accepting_a_fact_changes_offer_creates_a_paused_check() {
        use crate::scheduled_tasks::types::{TaskStatus, TriggerPhase};
        let root = tempfile::tempdir().unwrap();
        let executor = executor(root.path(), ChatSurface::Scheduled);
        let offer = cards::prepare_offer(
            &executor.state,
            "loupist-a",
            OfferInput {
                preset: WatchPreset::FactChanges,
                asset_id: follow(&executor),
                results_date: None,
                reason: "Guidance can change.".into(),
            },
        )
        .unwrap();
        cards::place(&executor.state, "loupist-a", vec![offer.clone()], None).unwrap();
        let task = cards::accept(&executor.state, "loupist-a", &offer.id).unwrap();
        assert_eq!(task.status, TaskStatus::Paused);
        assert!(task.awaiting_confirmation);
        assert!(task.is_trigger_task());
        let trigger = task.trigger.expect("the offer keeps its check");
        assert_eq!(trigger.script.name, "world-change");
        assert_eq!(trigger.state, TriggerPhase::Proposed);
        assert!(executor
            .state
            .store
            .scheduled_task_claim_with("loupist-a", &task.id, i64::MAX / 4, false, |_| Ok(()))
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn accepting_a_results_follow_up_uses_the_run_time_on_the_card() {
        use crate::scheduled_tasks::types::Schedule;
        let root = tempfile::tempdir().unwrap();
        let executor = executor(root.path(), ChatSurface::Scheduled);
        let mut offer = cards::prepare_offer(
            &executor.state,
            "loupist-a",
            OfferInput {
                preset: WatchPreset::ResultsFollowUp,
                asset_id: follow(&executor),
                results_date: Some("2099-02-01".into()),
                reason: "Check the results.".into(),
            },
        )
        .unwrap();
        let resolved = offer.run_at_ms.unwrap();
        // A time zone change after publication must not change the approved instant.
        let displayed = resolved + 3_600_000;
        offer.run_at_ms = Some(displayed);
        cards::place(&executor.state, "loupist-a", vec![offer.clone()], None).unwrap();
        let task = cards::accept(&executor.state, "loupist-a", &offer.id).unwrap();
        assert_eq!(task.schedule, Schedule::Once { at_ms: displayed });
    }

    #[tokio::test]
    async fn displaying_a_legacy_results_follow_up_pins_the_instant_for_acceptance() {
        use crate::scheduled_tasks::types::Schedule;
        let root = tempfile::tempdir().unwrap();
        let executor = executor(root.path(), ChatSurface::Scheduled);
        let state = &executor.state;
        let mut offer = cards::prepare_offer(
            state,
            "loupist-a",
            OfferInput {
                preset: WatchPreset::ResultsFollowUp,
                asset_id: follow(&executor),
                results_date: Some("2099-02-01".into()),
                reason: "Check the results.".into(),
            },
        )
        .unwrap();
        offer.run_at_ms = None;
        cards::place(state, "loupist-a", vec![offer.clone()], None).unwrap();
        let old = crate::chat::home::ensure_home_thread(state, "loupist-a").unwrap();
        let view = crate::commands::types::chat_dto_with_memory(state, &old).unwrap();
        let displayed = view
            .messages
            .iter()
            .flat_map(|msg| &msg.offers)
            .find(|row| row.id == offer.id)
            .unwrap()
            .run_at_ms
            .unwrap();
        let stored = state.store.get_chat(&old.id).unwrap().unwrap();
        assert_eq!(
            stored
                .messages
                .iter()
                .flat_map(|msg| &msg.offers)
                .find(|row| row.id == offer.id)
                .unwrap()
                .run_at_ms,
            Some(displayed)
        );
        // A second display from a stale snapshot must reuse the committed instant.
        let again = crate::commands::types::chat_dto_with_memory(state, &old).unwrap();
        assert_eq!(
            again
                .messages
                .iter()
                .flat_map(|msg| &msg.offers)
                .find(|row| row.id == offer.id)
                .unwrap()
                .run_at_ms,
            Some(displayed)
        );
        let task = cards::accept(state, "loupist-a", &offer.id).unwrap();
        assert_eq!(task.schedule, Schedule::Once { at_ms: displayed });
    }

    #[tokio::test]
    async fn an_expired_offer_creates_nothing() {
        let root = tempfile::tempdir().unwrap();
        let executor = executor(root.path(), ChatSurface::Bot);
        let asset = follow(&executor);
        let policy = crate::broker::ToolPolicy {
            loupist_id: "loupist-a".into(),
            session_id: "home-loupist-a".into(),
            use_memory: true,
            propose_memory_updates: true,
            memory_proposal_budget: 1,
            as_of: None,
            display_currency: None,
        };
        let offered = executor
            .execute(
                &policy,
                ToolMethod::BotOffer,
                json!({"preset":"week-in-review","asset_id":asset,"reason":"Nothing reviews the week."}),
            )
            .await
            .unwrap();
        let home_id = crate::chat::home::home_thread_id("loupist-a");
        // The interactive offer rides on the reply. Place it as the host would on delivery.
        let offer = executor
            .capture
            .offers
            .lock()
            .unwrap()
            .first()
            .cloned()
            .unwrap();
        crate::chat::cards::place(&executor.state, "loupist-a", vec![offer], None).unwrap();
        let expected = executor.state.store.get_chat(&home_id).unwrap().unwrap();
        let mut chat = expected.clone();
        chat.messages[0].offers[0].expires_at_ms = 1;
        chat.updated_at_ms += 1;
        executor.state.store.replace_chat(&expected, &chat).unwrap();
        let error = crate::commands::cards::accept(
            &executor.state,
            &crate::commands::cards::OfferRequest {
                loupist_id: "loupist-a".into(),
                offer_id: offered["id"].as_str().unwrap().into(),
            },
        )
        .unwrap_err();
        assert!(error.message.contains("expired"), "{}", error.message);
        assert!(service::list(&executor.state, "loupist-a")
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn answering_an_ask_records_a_choice_without_user_authored_text() {
        let root = tempfile::tempdir().unwrap();
        let executor = executor(root.path(), ChatSurface::Scheduled);
        let policy = crate::broker::ToolPolicy {
            loupist_id: "loupist-a".into(),
            session_id: "home-loupist-a".into(),
            use_memory: true,
            propose_memory_updates: true,
            memory_proposal_budget: 1,
            as_of: None,
            display_currency: None,
        };
        let asked = executor
            .execute(
                &policy,
                ToolMethod::AskLater,
                json!({"question":"Which listing did you mean?","options":[{"label":"The common stock"},{"label":"The ADR"}],"recommended":0}),
            )
            .await
            .unwrap();
        assert_eq!(asked["assumed"], "The common stock");
        let home_id = crate::chat::home::home_thread_id("loupist-a");
        let ask_id = executor
            .state
            .store
            .get_chat(&home_id)
            .unwrap()
            .unwrap()
            .messages
            .iter()
            .find_map(|message| message.ask.as_ref().map(|ask| ask.id.clone()))
            .unwrap();
        let answer = |choice| {
            crate::chat::cards::answer(&executor.state, "loupist-a", &ask_id, choice).unwrap();
        };
        answer(1);
        answer(1);
        let home = executor.state.store.get_chat(&home_id).unwrap().unwrap();
        let choices: Vec<_> = home
            .messages
            .iter()
            .filter(|message| message.role == crate::domain::ChatRole::User)
            .map(|message| message.content.as_str())
            .collect();
        assert!(choices.is_empty());
        assert!(crate::chat::cards::answer(&executor.state, "loupist-a", &ask_id, 0).is_err());
        assert_eq!(
            home.messages
                .iter()
                .filter_map(|message| message.ask.as_ref())
                .next()
                .unwrap()
                .choice,
            Some(1)
        );
    }

    #[tokio::test]
    async fn a_decline_reserves_the_card_before_config_write_and_can_be_retried() {
        let root = tempfile::tempdir().unwrap();
        let executor = executor(root.path(), ChatSurface::Scheduled);
        let state = &executor.state;
        let loupist = "loupist-a";
        let offer = cards::prepare_offer(
            state,
            loupist,
            OfferInput {
                preset: WatchPreset::FilingsAndResults,
                asset_id: follow(&executor),
                results_date: None,
                reason: "Check for filings.".into(),
            },
        )
        .unwrap();
        cards::place(state, loupist, vec![offer.clone()], None).unwrap();
        let directory = crate::commands::loupist_config_directory(state, loupist).unwrap();
        std::fs::create_dir_all(&directory).unwrap();
        let config = directory.join("config.toml");
        std::fs::write(&config, "[").unwrap();
        assert!(cards::dismiss(state, loupist, &offer.id).is_err());
        std::fs::write(&config, "").unwrap();
        assert!(cards::accept(state, loupist, &offer.id)
            .unwrap_err()
            .message
            .contains("declined"));
        assert!(service::list(state, loupist).unwrap().is_empty());
        cards::dismiss(state, loupist, &offer.id).unwrap();
        assert_eq!(
            crate::scoped_config::declined_offer_keys(
                &state.artifacts.app_data_dir,
                &directory,
                loupist,
            )
            .unwrap(),
            [cards::offer_key(offer.preset, &offer.asset_id, None)]
        );
    }

    #[tokio::test]
    async fn a_declined_offer_is_not_made_again() {
        let root = tempfile::tempdir().unwrap();
        let executor = executor(root.path(), ChatSurface::Scheduled);
        let asset = follow(&executor);
        let policy = crate::broker::ToolPolicy {
            loupist_id: "loupist-a".into(),
            session_id: "home-loupist-a".into(),
            use_memory: true,
            propose_memory_updates: true,
            memory_proposal_budget: 1,
            as_of: None,
            display_currency: None,
        };
        let offered = executor
            .execute(
                &policy,
                ToolMethod::BotOffer,
                json!({"preset":"filings-and-results","asset_id":asset,"reason":"Results are within two weeks."}),
            )
            .await
            .unwrap();
        crate::chat::cards::dismiss(
            &executor.state,
            "loupist-a",
            offered["id"].as_str().unwrap(),
        )
        .unwrap();
        let again = executor
            .execute(
                &policy,
                ToolMethod::BotOffer,
                json!({"preset":"filings-and-results","asset_id":asset,"reason":"Results are within two weeks."}),
            )
            .await
            .unwrap_err();
        assert!(again.to_string().contains("not made again"), "{again}");
    }
}
