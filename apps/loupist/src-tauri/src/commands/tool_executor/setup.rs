use rusqlite::Transaction;

use super::*;
use crate::{
    chat::surface::ChatSurface,
    domain::{LoupistTemplate, MapKind, SetupChange},
    knowledge::UNKNOWN_MARKET,
    store::user_context::{identity, name, WatchlistEntry},
};

const MAX_ASSETS: usize = 10;
const MAX_FOCUS_CHARS: usize = 280;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::commands) struct SetupAsset {
    pub symbol: String,
    pub market: Option<String>,
    pub name: Option<String>,
}

/// One checked `loupist_setup` call, applied when the reply commits.
#[derive(Clone, Debug, Default)]
pub(in crate::commands) struct StagedSetup {
    pub follow: Vec<SetupAsset>,
    pub unfollow: Vec<SetupAsset>,
    pub focus: Option<String>,
    pub preset: Option<LoupistTemplate>,
    pub user_message_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    follow: Vec<Value>,
    #[serde(default)]
    unfollow: Vec<Value>,
    focus: Option<Value>,
    preset: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssetInput {
    symbol: String,
    market: Option<String>,
    name: Option<String>,
}

fn invalid(field: impl Into<String>, expected: &str) -> BrokerError {
    BrokerError::InvalidArguments {
        field: field.into(),
        expected: expected.into(),
    }
}

fn assets(list: &str, items: Vec<Value>, named: bool) -> Result<Vec<SetupAsset>, BrokerError> {
    if items.len() > MAX_ASSETS {
        return Err(invalid(list, "at most 10 assets"));
    }
    let shape = if named {
        "{symbol, market?, name?}"
    } else {
        "{symbol, market?}"
    };
    let mut parsed: Vec<SetupAsset> = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        let at = |key: &str| format!("{list}[{index}]{key}");
        let input: AssetInput = serde_json::from_value(item).map_err(|_| invalid(at(""), shape))?;
        if !named && input.name.is_some() {
            return Err(invalid(at(""), shape));
        }
        let code = |key: &str, value: &str| {
            identity(value).map_err(|_| invalid(at(key), "1–64 ASCII characters without spaces"))
        };
        let symbol = code(".symbol", &input.symbol)?;
        let market = input
            .market
            .map(|market| code(".market", &market))
            .transpose()?
            .filter(|market| market != UNKNOWN_MARKET);
        let display = name(input.name.as_deref())
            .map_err(|_| invalid(at(".name"), "at most 256 characters"))?;
        if parsed
            .iter()
            .any(|asset| asset.symbol == symbol && asset.market == market)
        {
            return Err(invalid(at(""), "each asset once"));
        }
        parsed.push(SetupAsset {
            symbol,
            market,
            name: display,
        });
    }
    Ok(parsed)
}

fn parse(params: Value) -> Result<StagedSetup, BrokerError> {
    let request: Input = serde_json::from_value(params).map_err(|_| {
        invalid(
            "loupist_setup",
            "follow, unfollow, focus or preset as the tool describes",
        )
    })?;
    let follow = assets("follow", request.follow, true)?;
    let unfollow = assets("unfollow", request.unfollow, false)?;
    if let Some(index) = unfollow.iter().position(|asset| {
        follow
            .iter()
            .any(|followed| followed.symbol == asset.symbol)
    }) {
        return Err(invalid(
            format!("unfollow[{index}].symbol"),
            "an asset this call does not also follow",
        ));
    }
    let focus = match request.focus {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => {
            let text = text.trim();
            if text.is_empty() || text.chars().count() > MAX_FOCUS_CHARS || text.contains('\0') {
                return Err(invalid("focus", "1–280 characters"));
            }
            Some(text.to_owned())
        }
        Some(_) => return Err(invalid("focus", "1–280 characters")),
    };
    let preset =
        match request.preset {
            None | Some(Value::Null) => None,
            Some(value) => Some(value.as_str().and_then(LoupistTemplate::parse).ok_or_else(
                || {
                    let ids = LoupistTemplate::ALL.map(LoupistTemplate::as_str).join(", ");
                    invalid("preset", &format!("one of {ids}"))
                },
            )?),
        };
    if follow.is_empty() && unfollow.is_empty() && focus.is_none() && preset.is_none() {
        return Err(invalid(
            "loupist_setup",
            "at least one of follow, unfollow, focus or preset",
        ));
    }
    Ok(StagedSetup {
        follow,
        unfollow,
        focus,
        preset,
        user_message_id: None,
    })
}

impl AppToolExecutor {
    pub(super) async fn stage_loupist_setup(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if !self
            .capture
            .surface
            .as_ref()
            .is_some_and(ChatSurface::interactive)
        {
            return Err(BrokerError::Execution(format!(
                "loupist_setup is unavailable in a {} turn: only the user, in the Bot or a research session, sets up this Loupist",
                self.capture.surface.as_ref().map_or("host", ChatSurface::name)
            )));
        }
        if !policy.propose_memory_updates || !policy.use_memory {
            return Err(BrokerError::MemoryDisabled);
        }
        let mut staged = parse(params)?;
        staged.user_message_id = self.capture.principle_user_message_id.clone();
        let result = json!({
            "status": "staged",
            "durable": false,
            "follow": staged.follow.iter().map(|asset| &asset.symbol).collect::<Vec<_>>(),
            "unfollow": staged.unfollow.iter().map(|asset| &asset.symbol).collect::<Vec<_>>(),
            "focus": staged.focus,
            "preset": staged.preset.map(LoupistTemplate::as_str),
            "notice": "Applies when this reply finishes. Tell the user in plain words what this Loupist now follows and researches; never send them to settings.",
        });
        self.capture
            .stage(delivery_id, |pending| pending.setup = Some(staged))
            .await;
        Ok(result)
    }
}

fn shown_market(market: &str) -> Option<String> {
    (market != UNKNOWN_MARKET).then(|| market.to_owned())
}

/// The watchlist lives in the app database, outside the reply's transaction.
/// It is applied once the reply is committing, through the watchlist commands'
/// own functions, and only the rows that changed are reported.
pub(crate) fn apply_watchlist(
    state: &AppState,
    loupist_id: &str,
    setup: &StagedSetup,
) -> Vec<SetupChange> {
    let mut changes = Vec::new();
    let Ok(mut current) = state.store.watchlist_list() else {
        eprintln!("loupist_setup could not read the watchlist");
        return changes;
    };
    for asset in &setup.unfollow {
        let matching: Vec<WatchlistEntry> = current
            .iter()
            .filter(|entry| {
                entry.symbol == asset.symbol
                    && asset
                        .market
                        .as_ref()
                        .is_none_or(|market| &entry.market == market)
            })
            .cloned()
            .collect();
        for entry in matching {
            if state
                .store
                .watchlist_remove(&entry.symbol, &entry.market)
                .is_err()
            {
                eprintln!("loupist_setup could not stop following an asset");
                continue;
            }
            current.retain(|row| row != &entry);
            changes.push(SetupChange::Unfollowed {
                symbol: entry.symbol,
                market: shown_market(&entry.market),
            });
        }
    }
    for asset in &setup.follow {
        let followed = current.iter().any(|entry| {
            entry.symbol == asset.symbol
                && asset
                    .market
                    .as_ref()
                    .is_none_or(|market| &entry.market == market)
        });
        if followed {
            continue;
        }
        let market = asset.market.as_deref().unwrap_or(UNKNOWN_MARKET);
        match crate::commands::user_context::follow(
            state,
            &asset.symbol,
            market,
            asset.name.as_deref(),
            Some(loupist_id),
        ) {
            Ok(entry) => {
                current.push(entry);
                changes.push(SetupChange::Followed {
                    symbol: asset.symbol.clone(),
                    market: asset.market.clone(),
                    name: asset.name.clone(),
                });
            }
            Err(_) => eprintln!("loupist_setup could not follow an asset"),
        }
    }
    changes
}

/// Focus and preset kinds, inside the reply's transaction. The profile and
/// the World settings take the same focus in one write so they cannot drift;
/// the caller resyncs the folder registration after commit. A preset's
/// default focus fills only an empty focus.
pub(crate) fn commit_turn(
    tx: &Transaction<'_>,
    loupist_id: &str,
    chat_id: &str,
    at_ms: i64,
    setup: &StagedSetup,
) -> Result<(Vec<SetupChange>, Vec<MapKind>), crate::store::StoreError> {
    let knowledge = |error: crate::knowledge::KnowledgeError| {
        crate::store::StoreError::Invalid(error.to_string())
    };
    let mut changes = Vec::new();
    let mut skipped = Vec::new();
    let current = crate::store::loupist_profile_in(tx, loupist_id)?.focus;
    let focus = match (
        &setup.focus,
        setup.preset.and_then(LoupistTemplate::default_focus),
    ) {
        (Some(focus), _) => Some(focus.as_str()),
        (None, Some(default)) if current.trim().is_empty() => Some(default),
        _ => None,
    };
    if let Some(focus) = focus {
        let mut settings = crate::knowledge::load_settings(tx, loupist_id).map_err(knowledge)?;
        if current != focus || settings.research_focus != focus {
            crate::store::update_loupist_fields_in(tx, loupist_id, at_ms, |profile| {
                profile.focus = focus.to_owned();
            })?;
            settings.research_focus = focus.to_owned();
            crate::knowledge::store_settings(tx, loupist_id, &settings).map_err(knowledge)?;
            changes.push(SetupChange::Focus {
                focus: focus.to_owned(),
            });
        }
    }
    if let Some(preset) = setup.preset {
        let message = setup.user_message_id.as_deref().unwrap_or_default();
        let origin = (!message.is_empty()).then_some((chat_id, message));
        match crate::knowledge::apply_preset(tx, loupist_id, preset.as_str(), origin, at_ms) {
            Ok(Some(change)) => changes.push(SetupChange::Tracks {
                kinds: crate::knowledge::added_labels(&change)
                    .into_iter()
                    .map(|kind| MapKind {
                        name: kind.name,
                        label: kind.labels.en,
                        label_ko: kind.labels.ko,
                    })
                    .collect(),
            }),
            Ok(None) => {}
            Err(crate::knowledge::KnowledgeError::Invalid(_)) => {
                let (types, relations) =
                    crate::knowledge::missing_preset_kinds(tx, loupist_id, preset.as_str())
                        .map_err(knowledge)?;
                let types = types.into_iter().map(|item| (item.name, item.labels));
                let relations = relations.into_iter().map(|item| (item.name, item.labels));
                skipped.extend(types.chain(relations).map(|(name, labels)| MapKind {
                    name,
                    label: labels.en,
                    label_ko: labels.ko,
                }));
            }
            Err(error) => return Err(knowledge(error)),
        }
    }
    Ok((changes, skipped))
}
