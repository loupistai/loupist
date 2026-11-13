//! `connection_setup`: the model names a bundled connection and the reply shows
//! a card that sets it up. Rust resolves the name, reports the connection's
//! state for this Loupist, and never touches a key: the user enters one in the
//! card, and the binding change reaches the next turn's sealed authority.
use serde::Deserialize;
use serde_json::{json, Value};

use super::*;
use crate::{
    chat::surface::ChatSurface,
    marketplace::{
        entry_lookup::{resolve_setup_entry, SetupCandidate},
        MarketplaceConnectorReadiness, MarketplaceEntryDto,
    },
};

const MAX_ID_CHARS: usize = 96;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    id: String,
}

/// What the card will offer, computed from connector readiness and this
/// Loupist's binding at the moment of the call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SetupState {
    Unavailable,
    NeedsSetup,
    OffHere,
    Ready,
}

impl SetupState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::NeedsSetup => "needs_setup",
            Self::OffHere => "off_here",
            Self::Ready => "ready",
        }
    }

    /// `in_run` says whether this reply's sealed capabilities include the
    /// entry; `loadable` whether one capability loads its tools.
    fn notice(self, entry_id: &str, in_run: bool, loadable: bool) -> String {
        if self == Self::Ready && in_run {
            return if loadable {
                format!("This connection is set up and on. Load it now with capability_load {{id: \"{entry_id}\"}} and use it in this reply. Never ask for or repeat a key in chat.")
            } else {
                "This connection is set up and on in this reply; use its tools where the capability index lists them. Never ask for or repeat a key in chat.".into()
            };
        }
        match self {
            Self::Unavailable => "This connection cannot run in this Loupist build, so the card in this reply only says it is unavailable here. Tell the user that plainly; do not ask for a key or promise that setup will make it work.",
            Self::NeedsSetup => "The card in this reply asks the user for this connection's setup. Any key goes in the card, never in chat: do not ask for one and never repeat one. The connection works from the user's next message, so finish this reply without it and say the card is ready. The card changes nothing else: answer from what the user gave you and, with saving on, keep the facts they stated with their origin exactly as you would without the card.",
            Self::OffHere => "This connection is set up but off for this Loupist; the card in this reply offers to turn it on. It works from the user's next message, so finish this reply without it. The card changes nothing else: answer from what the user gave you and, with saving on, keep the facts they stated with their origin exactly as you would without the card. Never ask for or repeat a key in chat.",
            Self::Ready => "This connection is set up and on for this Loupist, but this reply started before it was on, so it works from the user's next message. Finish this reply without it. The card changes nothing else: answer from what the user gave you and, with saving on, keep the facts they stated with their origin exactly as you would without the card. Never ask for or repeat a key in chat.",
        }
        .into()
    }
}

fn invalid(field: &str, expected: String) -> BrokerError {
    BrokerError::InvalidArguments {
        field: field.into(),
        expected,
    }
}

fn candidates_text(candidates: &[SetupCandidate]) -> String {
    candidates
        .iter()
        .map(|candidate| format!("{} ({})", candidate.id, candidate.name))
        .collect::<Vec<_>>()
        .join(", ")
}

fn unavailable<E>(_: E) -> BrokerError {
    BrokerError::Execution("connection status is unavailable".into())
}

fn setup_state(
    state: &AppState,
    loupist_id: &str,
    entry: &MarketplaceEntryDto,
) -> Result<SetupState, BrokerError> {
    let readiness = crate::marketplace::with_connector_lifecycle(state, || {
        crate::marketplace::connector_readiness(entry, state)
    })
    .map_err(unavailable)?;
    match readiness {
        MarketplaceConnectorReadiness::Ready => {}
        MarketplaceConnectorReadiness::RuntimeUnavailable => return Ok(SetupState::Unavailable),
        _ => return Ok(SetupState::NeedsSetup),
    }
    let enabled = state
        .store
        .get_loupist_capability(loupist_id, &entry.id)
        .map_err(unavailable)?
        .is_some_and(|binding| {
            binding.enabled
                && binding
                    .granted_permissions
                    .iter()
                    .any(|permission| permission == "execute")
        });
    Ok(if enabled {
        SetupState::Ready
    } else {
        SetupState::OffHere
    })
}

impl AppToolExecutor {
    pub(super) fn connection_setup(&self, params: Value) -> Result<Value, BrokerError> {
        if !self
            .capture
            .surface
            .as_ref()
            .is_some_and(ChatSurface::interactive)
        {
            return Err(BrokerError::Execution(format!(
                "connection_setup is unavailable in a {} turn: only the user, in the Bot or a research session, sets up a connection",
                self.capture.surface.as_ref().map_or("host", ChatSurface::name)
            )));
        }
        let request: Input = serde_json::from_value(params)
            .map_err(|_| invalid("connection_setup", "{id: a connection id or name}".into()))?;
        let id = request.id.trim();
        if id.is_empty() || id.chars().count() > MAX_ID_CHARS {
            return Err(invalid("id", "1–96 characters".into()));
        }
        if crate::util::credential_text::contains_credential_text(id) {
            return Err(invalid(
                "id",
                "a connection id or name, never a key; the user enters any key in the card".into(),
            ));
        }
        let entry = resolve_setup_entry(id).map_err(|candidates| {
            invalid(
                "id",
                format!("one of these connections: {}", candidates_text(&candidates)),
            )
        })?;
        let state = setup_state(&self.state, &self.loupist_id, entry)?;
        Ok(json!({
            "status": "card_shown",
            "id": entry.id,
            "name": entry.name,
            "state": state.as_str(),
            "notice": state.notice(
                &entry.id,
                self.capability_ids.contains(&entry.id),
                crate::marketplace::bundled_plugins()
                    .ok()
                    .and_then(|market| crate::agent_harness::serving_component_id(entry, &market))
                    .is_some(),
            ),
        }))
    }
}
