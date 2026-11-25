//! Preset offers and later questions stored on a Thread message.
//!
//! An offer names a bundled watch preset and a followed asset. Accepting it
//! creates that preset's task and nothing else. Declining it records the same
//! offer on the Loupist's config so it is not made again. A question does not
//! pause the run; the card keeps the index of the user's choice.

use std::borrow::Cow;

use crate::{
    app::{AppState, CommandError},
    commands::{map_store, new_id, now_ms},
    domain::{ChatMessage, ChatSession, HomeAsk, HomeAskOption, HomeOffer},
    scheduled_tasks::{
        service, store,
        types::{Task, TaskKind, TaskStatus, TaskSubject, WatchPreset},
    },
    store::{SqliteStore, StoreError},
};

pub(crate) const CARD_TTL_MS: i64 = 14 * 24 * 60 * 60 * 1000;
pub(crate) const MAX_OFFERS_PER_TURN: usize = 2;

/// Pin old cards to the device's time zone the first time they are shown or accepted.
pub(crate) fn resolve_legacy_follow_ups<'a>(
    state: &AppState,
    chat: &'a ChatSession,
) -> Result<Cow<'a, ChatSession>, CommandError> {
    let mut current = Cow::Borrowed(chat);
    let needs_time = |offer: &HomeOffer| {
        offer.preset == WatchPreset::ResultsFollowUp
            && offer.run_at_ms.is_none()
            && offer.results_date.is_some()
    };
    while current
        .messages
        .iter()
        .flat_map(|msg| &msg.offers)
        .any(needs_time)
    {
        let mut pinned = current.as_ref().clone();
        for offer in pinned.messages.iter_mut().flat_map(|msg| &mut msg.offers) {
            if needs_time(offer) {
                offer.run_at_ms = offer.results_date.as_deref().and_then(|date| {
                    crate::scheduled_tasks::presets::results_follow_up_at(date).ok()
                });
            }
        }
        if pinned.messages == current.messages {
            break;
        }
        let _activity = state
            .maintenance
            .admit_kind(crate::runs::maintenance::MaintenanceActivityKind::ChatMutation)?;
        match state.store.replace_chat_with(&current, &pinned, |_| Ok(())) {
            Ok(()) => return Ok(Cow::Owned(pinned)),
            Err(StoreError::Conflict("Chat compare-and-swap")) => {
                current = Cow::Owned(
                    state
                        .store
                        .get_chat(&chat.id)
                        .map_err(map_store)?
                        .ok_or_else(|| CommandError::not_found("Chat"))?,
                );
            }
            Err(error) => return Err(map_store(error)),
        }
    }
    Ok(current)
}

pub(crate) fn offer_key(preset: WatchPreset, asset_id: &str, results_date: Option<&str>) -> String {
    format!(
        "{}:{asset_id}:{}",
        preset_name(preset),
        results_date.unwrap_or("")
    )
}

fn preset_name(preset: WatchPreset) -> String {
    serde_json::to_value(preset)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn one_line(value: &str, max_chars: usize) -> Result<String, CommandError> {
    let text = value.trim();
    if text.is_empty() || text.chars().count() > max_chars || text.chars().any(|ch| ch.is_control())
    {
        return Err(CommandError::invalid(
            "Use one short line, with no line breaks",
        ));
    }
    Ok(text.to_owned())
}

pub(crate) struct OfferInput {
    pub preset: WatchPreset,
    pub asset_id: String,
    pub results_date: Option<String>,
    pub reason: String,
}

/// Refuse an unknown shape before anything is stored: a known preset, one
/// followed asset, and no task that already does that work.
pub(crate) fn prepare_offer(
    state: &AppState,
    loupist_id: &str,
    input: OfferInput,
) -> Result<HomeOffer, CommandError> {
    if input.preset == WatchPreset::PriceMove {
        return Err(CommandError::invalid("Choose an offer preset"));
    }
    let reason = one_line(&input.reason, 200)?;
    let asset_id = input.asset_id.trim().to_owned();
    crate::scheduled_tasks::types::identifier(&asset_id).map_err(map_store)?;
    let followed = state
        .store
        .assets_list(loupist_id, true)
        .map_err(map_store)?;
    let asset = followed
        .iter()
        .find(|row| row.asset_id == asset_id)
        .ok_or_else(|| {
            CommandError::invalid("Offer a preset only for an asset this Loupist follows")
        })?;
    let run_at_ms = if input.preset == WatchPreset::ResultsFollowUp {
        Some(crate::scheduled_tasks::presets::results_follow_up_at(
            input.results_date.as_deref().unwrap_or(""),
        )?)
    } else if input.results_date.is_some() {
        return Err(CommandError::invalid(
            "Only a results follow-up takes a results date",
        ));
    } else {
        None
    };
    let key = offer_key(input.preset, &asset_id, input.results_date.as_deref());
    if declined(state, loupist_id)?
        .iter()
        .any(|existing| existing == &key)
    {
        return Err(CommandError::invalid(
            "This offer was declined and is not made again",
        ));
    }
    if already_watching(state, loupist_id, input.preset, &asset_id)? {
        return Err(CommandError::invalid(
            "A listed task already does this. Do not offer it again",
        ));
    }
    if open_offer_exists(state, loupist_id, &key)? {
        return Err(CommandError::invalid(
            "This offer is already waiting on a card",
        ));
    }
    let now = now_ms();
    Ok(HomeOffer {
        id: new_id("offer"),
        preset: input.preset,
        asset_id,
        asset_name: asset.name.clone(),
        results_date: input.results_date,
        run_at_ms,
        reason,
        created_at_ms: now,
        expires_at_ms: now.saturating_add(CARD_TTL_MS),
        task_id: None,
        dismissed: false,
    })
}

pub(crate) struct AskInput {
    pub question: String,
    pub options: Vec<String>,
    pub recommended: u8,
}

pub(crate) fn prepare_ask(input: AskInput) -> Result<HomeAsk, CommandError> {
    let question = one_line(&input.question, 300)?;
    if !(2..=4).contains(&input.options.len()) {
        return Err(CommandError::invalid("Give 2 to 4 choices"));
    }
    let mut options = Vec::new();
    for label in &input.options {
        let label = one_line(label, 60)?;
        if options
            .iter()
            .any(|option: &HomeAskOption| option.label == label)
        {
            return Err(CommandError::invalid("Choices must differ"));
        }
        options.push(HomeAskOption { label });
    }
    if usize::from(input.recommended) >= options.len() {
        return Err(CommandError::invalid(
            "recommended must be an index into options",
        ));
    }
    let now = now_ms();
    Ok(HomeAsk {
        id: new_id("ask"),
        question,
        options,
        recommended: input.recommended,
        created_at_ms: now,
        expires_at_ms: now.saturating_add(CARD_TTL_MS),
        choice: None,
    })
}

fn declined(state: &AppState, loupist_id: &str) -> Result<Vec<String>, CommandError> {
    let directory = crate::commands::loupist_config_directory(state, loupist_id)?;
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    crate::scoped_config::declined_offer_keys(&state.artifacts.app_data_dir, &directory, loupist_id)
}

fn already_watching(
    state: &AppState,
    loupist_id: &str,
    preset: WatchPreset,
    asset_id: &str,
) -> Result<bool, CommandError> {
    let tasks = state
        .store
        .scheduled_tasks_list(loupist_id)
        .map_err(map_store)?;
    Ok(tasks.iter().any(|task| {
        task.status != TaskStatus::Completed
            && task.watch.preset == Some(preset)
            && match task.watch.subjects.as_deref() {
                None | Some([]) => true,
                Some(subjects) => subjects.iter().any(
                    |subject| matches!(subject, TaskSubject::Asset { asset } if asset == asset_id),
                ),
            }
    }))
}

fn open_offer_exists(state: &AppState, loupist_id: &str, key: &str) -> Result<bool, CommandError> {
    let home = super::home::ensure_home_thread(state, loupist_id)?;
    let now = now_ms();
    Ok(home.messages.iter().any(|message| {
        message.offers.iter().any(|offer| {
            (offer.dismissed || offer.open(now))
                && offer_key(offer.preset, &offer.asset_id, offer.results_date.as_deref()) == key
        })
    }))
}

/// Append one host post. Used when a scheduled run offers or asks, because
/// that run's own chat is not the Thread.
pub(crate) fn place(
    state: &AppState,
    loupist_id: &str,
    offers: Vec<HomeOffer>,
    ask: Option<HomeAsk>,
) -> Result<(), CommandError> {
    if offers.is_empty() && ask.is_none() {
        return Ok(());
    }
    let now = now_ms();
    let content = offers
        .first()
        .map(|offer| offer.reason.clone())
        .or_else(|| ask.as_ref().map(|ask| ask.question.clone()))
        .unwrap_or_default();
    let mut message = super::home::host_message("bot-card", content, now)?;
    message.offers = offers;
    message.ask = ask;
    mutate_home(
        state,
        loupist_id,
        |messages| {
            let existing = messages.iter().flat_map(|message| &message.offers);
            for offer in existing {
                for pending in &message.offers {
                    if offer.id == pending.id {
                        return Ok(());
                    }
                    if (offer.dismissed || offer.open(now))
                        && offer.preset == pending.preset
                        && offer.asset_id == pending.asset_id
                        && offer.results_date == pending.results_date
                    {
                        return Err(CommandError::invalid(
                            "This offer is already waiting on a card",
                        ));
                    }
                }
            }
            if message.ask.as_ref().is_some_and(|ask| {
                messages
                    .iter()
                    .any(|msg| msg.ask.as_ref().is_some_and(|old| old.id == ask.id))
            }) {
                return Ok(());
            }
            messages.push(message.clone());
            Ok(())
        },
        |_, _| Ok(()),
    )
}

fn mutate_home<T>(
    state: &AppState,
    loupist_id: &str,
    mut change: impl FnMut(&mut Vec<ChatMessage>) -> Result<T, CommandError>,
    mut during: impl FnMut(&rusqlite::Transaction<'_>, &T) -> Result<(), StoreError>,
) -> Result<T, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(crate::runs::maintenance::MaintenanceActivityKind::ChatMutation)?;
    loop {
        let home = super::home::ensure_home_thread(state, loupist_id)?;
        let mut chat = home.clone();
        let result = change(&mut chat.messages)?;
        if chat.messages == home.messages {
            return Ok(result);
        }
        chat.pi_session_cache = None;
        chat.updated_at_ms = now_ms().max(home.updated_at_ms.saturating_add(1));
        match state
            .store
            .replace_chat_with(&home, &chat, |tx| during(tx, &result))
        {
            Err(StoreError::Conflict("Chat compare-and-swap")) => continue,
            Err(error) => return Err(map_store(error)),
            Ok(()) => {
                crate::briefings::desktop::changed(loupist_id);
                return Ok(result);
            }
        }
    }
}

pub(crate) fn accept(
    state: &AppState,
    loupist_id: &str,
    offer_id: &str,
) -> Result<Task, CommandError> {
    let offer = find_offer(state, loupist_id, offer_id)?;
    if let Some(id) = offer.task_id {
        return service::get(state, loupist_id, &id);
    }
    require_open_offer(&offer)?;
    let run_at_ms = offer.run_at_ms;
    if offer.preset == WatchPreset::ResultsFollowUp && run_at_ms.is_none() {
        return Err(CommandError::invalid(
            "This follow-up has no valid run time",
        ));
    }
    let crate::commands::scheduled_tasks::PreparedPreset {
        id: task_id,
        draft,
        definition,
        ..
    } = crate::commands::scheduled_tasks::preset_definition(
        crate::commands::scheduled_tasks::FromPresetRequest {
            loupist_id: loupist_id.to_owned(),
            preset: offer.preset,
            subjects: Some(vec![TaskSubject::Asset {
                asset: offer.asset_id,
            }]),
            params: Some(crate::scheduled_tasks::presets::PresetParams {
                language: None,
                results_date: offer.results_date,
            }),
        },
        state,
        run_at_ms,
    )?;
    service::validate_draft(state, &draft)?;
    let mut created = None;
    let id = mutate_home(
        state,
        loupist_id,
        |messages| {
            let offer = offer_mut(messages, offer_id)?;
            if let Some(id) = &offer.task_id {
                return Ok(id.clone());
            }
            require_open_offer(offer)?;
            offer.task_id = Some(task_id.clone());
            Ok(task_id.clone())
        },
        |tx, id| {
            if store::load_task(tx, loupist_id, id)?.is_none() {
                created = Some(SqliteStore::create_task_in(
                    tx,
                    loupist_id,
                    id,
                    draft.clone(),
                    TaskKind::Custom,
                    definition.clone(),
                    now_ms(),
                )?);
            }
            Ok(())
        },
    )?;
    match created {
        Some(task) => service::saved_task(state, task),
        None => service::get(state, loupist_id, &id),
    }
}

fn require_open_offer(offer: &HomeOffer) -> Result<(), CommandError> {
    if offer.dismissed {
        return Err(CommandError::conflict("This offer was declined"));
    }
    let now = now_ms();
    if now >= offer.expires_at_ms || offer.run_at_ms.is_some_and(|at| now >= at) {
        return Err(CommandError::conflict("This offer has expired"));
    }
    Ok(())
}

fn offer_mut<'a>(
    messages: &'a mut [ChatMessage],
    id: &str,
) -> Result<&'a mut HomeOffer, CommandError> {
    messages
        .iter_mut()
        .flat_map(|message| message.offers.iter_mut())
        .find(|offer| offer.id == id)
        .ok_or_else(|| CommandError::not_found("offer"))
}

pub(crate) fn dismiss(
    state: &AppState,
    loupist_id: &str,
    offer_id: &str,
) -> Result<(), CommandError> {
    let key = mutate_home(
        state,
        loupist_id,
        |messages| {
            let offer = offer_mut(messages, offer_id)?;
            if !offer.dismissed {
                if offer.task_id.is_some() {
                    return Err(CommandError::conflict("This offer is already on"));
                }
                require_open_offer(offer)?;
                offer.dismissed = true;
            }
            Ok(offer_key(
                offer.preset,
                &offer.asset_id,
                offer.results_date.as_deref(),
            ))
        },
        |_, _| Ok(()),
    )?;
    let directory = crate::commands::loupist_config_directory(state, loupist_id)?;
    crate::scoped_config::remember_declined_offer(
        &state.artifacts.app_data_dir,
        &directory,
        loupist_id,
        &key,
    )
}

pub(crate) fn answer(
    state: &AppState,
    loupist_id: &str,
    ask_id: &str,
    choice: u8,
) -> Result<(), CommandError> {
    mutate_home(
        state,
        loupist_id,
        |messages| {
            let ask = messages
                .iter_mut()
                .find_map(|message| message.ask.as_mut().filter(|ask| ask.id == ask_id))
                .ok_or_else(|| CommandError::not_found("question"))?;
            if let Some(existing) = ask.choice {
                return if existing == choice {
                    Ok(())
                } else {
                    Err(CommandError::conflict("This question was already answered"))
                };
            }
            if now_ms() >= ask.expires_at_ms {
                return Err(CommandError::conflict("This question has expired"));
            }
            if ask.options.get(usize::from(choice)).is_none() {
                return Err(CommandError::invalid("Choose one of the listed answers"));
            }
            ask.choice = Some(choice);
            Ok(())
        },
        |_, _| Ok(()),
    )
}

fn find_offer(
    state: &AppState,
    loupist_id: &str,
    offer_id: &str,
) -> Result<HomeOffer, CommandError> {
    let home = super::home::ensure_home_thread(state, loupist_id)?;
    let home = resolve_legacy_follow_ups(state, &home)?;
    home.messages
        .iter()
        .flat_map(|message| message.offers.iter())
        .find(|offer| offer.id == offer_id)
        .cloned()
        .ok_or_else(|| CommandError::not_found("offer"))
}

pub(crate) struct WaitingCard {
    pub kind: WaitingKind,
    pub id: String,
    pub title: String,
    pub at_ms: i64,
    pub results_date: Option<String>,
    pub chat_id: String,
}

pub(crate) enum WaitingKind {
    Offer,
    Ask,
}

pub(crate) fn waiting(
    state: &AppState,
    loupist_id: &str,
) -> Result<Vec<WaitingCard>, CommandError> {
    let id = super::home::home_thread_id(loupist_id);
    let Some(home) = state.store.get_chat(&id).map_err(map_store)? else {
        return Ok(Vec::new());
    };
    if home.loupist_id != loupist_id {
        return Ok(Vec::new());
    }
    let now = now_ms();
    let mut cards = Vec::new();
    for message in &home.messages {
        for offer in &message.offers {
            if offer.open(now) {
                cards.push(WaitingCard {
                    kind: WaitingKind::Offer,
                    id: offer.id.clone(),
                    title: format!("{} · {}", offer.asset_name, preset_name(offer.preset)),
                    at_ms: offer.created_at_ms,
                    results_date: offer.results_date.clone(),
                    chat_id: home.id.clone(),
                });
            }
        }
        if let Some(ask) = &message.ask {
            if ask.open(now) {
                cards.push(WaitingCard {
                    kind: WaitingKind::Ask,
                    id: ask.id.clone(),
                    title: ask.question.clone(),
                    at_ms: ask.created_at_ms,
                    results_date: None,
                    chat_id: home.id.clone(),
                });
            }
        }
    }
    Ok(cards)
}
