use rusqlite::Transaction;

use super::*;
use crate::{
    chat::surface::ChatSurface,
    commands::assets::wire::{AssetReferenceFields, AssetReferenceOrigin, AssetRow},
    domain::{LoupistTemplate, MapKind, SetupChange},
    store::user_context,
};

const MAX_ASSETS: usize = 10;
const MAX_FOCUS_CHARS: usize = 280;

/// One checked `loupist_setup` call, applied when the reply commits.
#[derive(Clone, Debug, Default)]
pub(in crate::commands) struct StagedSetup {
    pub follow: Vec<AssetRow>,
    pub unfollow: Vec<AssetRow>,
    pub focus: Option<String>,
    pub preset: Option<LoupistTemplate>,
    pub user_message_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    follow: Vec<FollowInput>,
    #[serde(default)]
    unfollow: Vec<UnfollowInput>,
    focus: Option<Value>,
    preset: Option<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FollowInput {
    name: String,
    reference: Option<AssetReferenceFields>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UnfollowInput {
    asset: String,
}

fn invalid(field: impl Into<String>, expected: &str) -> BrokerError {
    BrokerError::InvalidArguments {
        field: field.into(),
        expected: expected.into(),
    }
}

fn parse(params: Value, current: &[AssetRow]) -> Result<StagedSetup, BrokerError> {
    let request: Input = serde_json::from_value(params)
        .map_err(|_| invalid("loupist_setup", "Use follow, unfollow, focus or preset."))?;
    if request.follow.len() > MAX_ASSETS || request.unfollow.len() > MAX_ASSETS {
        return Err(invalid("assets", "Use at most 10 assets per list."));
    }
    let mut planned = current.to_vec();
    let mut follow = Vec::new();
    for requested in request.follow {
        let asset = user_context::plan_follow(
            &planned,
            &requested.name,
            requested.reference,
            AssetReferenceOrigin::Agent,
            crate::commands::now_ms(),
        )
        .map_err(store_error)?;
        if follow
            .iter()
            .any(|row: &AssetRow| row.asset_id == asset.asset_id)
        {
            return Err(invalid("follow", "List each asset once."));
        }
        if !planned.iter().any(|row| row.asset_id == asset.asset_id) {
            planned.push(asset.clone());
        }
        follow.push(asset);
    }
    let current = current
        .iter()
        .filter(|row| row.followed_at_ms.is_some())
        .cloned()
        .collect::<Vec<_>>();
    let mut unfollow = Vec::new();
    for requested in request.unfollow {
        user_context::text(&requested.asset, 256).map_err(store_error)?;
        match user_context::matching(&current, &requested.asset).as_slice() {
            [] => {}
            [row] => {
                if follow.iter().any(|asset| asset.asset_id == row.asset_id) {
                    return Err(invalid("unfollow", "This call also follows the asset."));
                }
                if !unfollow
                    .iter()
                    .any(|asset: &AssetRow| asset.asset_id == row.asset_id)
                {
                    unfollow.push((*row).clone());
                }
            }
            candidates => {
                return Err(invalid(
                    "unfollow",
                    &format!("Several assets match: {}", json!(candidates)),
                ))
            }
        }
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
        if !policy.use_memory {
            return Err(BrokerError::MemoryDisabled);
        }
        if !policy.propose_memory_updates {
            return Err(BrokerError::ProposalDisabled);
        }
        // Follow and unfollow act on this Loupist's own list.
        let mut staged = parse(
            params,
            &self
                .state
                .store
                .assets_list(&self.loupist_id, false)
                .map_err(store_error)?,
        )?;
        staged.user_message_id = self.capture.principle_user_message_id.clone();
        let result = json!({
            "status": "staged",
            "durable": false,
            "follow": staged.follow,
            "unfollow": staged.unfollow,
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

pub(crate) fn apply_watchlist(
    state: &AppState,
    loupist_id: &str,
    setup: &StagedSetup,
) -> Vec<(SetupChange, String)> {
    let mut changes = Vec::new();
    for asset in &setup.unfollow {
        if state
            .store
            .set_asset_followed(loupist_id, &asset.asset_id, None)
            .is_ok_and(|asset| asset.is_some())
        {
            let item = user_context::MapContextItem::from(asset);
            changes.push((
                SetupChange::Unfollowed {
                    symbol: label(&item, &asset.name),
                    market: (!item.market.is_empty()).then_some(item.market),
                },
                watchlist_label(asset),
            ));
        }
    }
    for staged in &setup.follow {
        if let Ok(Some(asset)) =
            state
                .store
                .apply_asset_follow(loupist_id, staged, crate::commands::now_ms())
        {
            let item = user_context::MapContextItem::from(&asset);
            changes.push((
                SetupChange::Followed {
                    symbol: label(&item, &asset.name),
                    market: (!item.market.is_empty()).then_some(item.market),
                    name: Some(asset.name),
                },
                watchlist_label(staged),
            ));
        }
    }
    if !changes.is_empty() {
        crate::commands::assets::follows_changed(state);
    }
    changes
}

pub(super) fn watchlist_label(asset: &AssetRow) -> String {
    label(&user_context::MapContextItem::from(asset), &asset.name)
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .trim()
        .to_owned()
}

fn label(item: &user_context::MapContextItem, name: &str) -> String {
    let text = if item.symbol.is_empty() {
        name
    } else {
        &item.symbol
    };
    text.chars().take(64).collect()
}

/// Focus and preset kinds, inside the reply's transaction. The profile and the
/// World settings take the same focus in one write so they cannot drift; the
/// focus files no Lens page. The caller resyncs the folder registration after
/// commit. A preset's default focus fills only an empty focus.
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
