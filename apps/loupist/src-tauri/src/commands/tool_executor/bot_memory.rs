use super::*;
use crate::chat::bot_memory::{
    BotMemoryAction, BotMemoryKind, BotMemoryNote, StagedBotMemory, MAX_BASIS, MAX_TURN_CHANGES,
};

/// The pause after third-party text, worded for Bot memory and principles:
/// what a later reply would read as the user's own words must not come from a
/// page or a file.
pub(crate) const BOT_MEMORY_AFTER_UNTRUSTED_REFUSAL: &str = "This reply read third-party text, so it saves no instruction a later turn would receive: no new or changed Bot memory entry and no principle. Bot memory can still resolve and forget. Answer as usual and tell the user what you would save; a later reply can save it once the user confirms.";

/// A tool whose saved text is an instruction a later turn receives: a Lens
/// principle, or a Bot memory entry (preference entries also travel as
/// `standing_preferences`). Notes, Journal, page filings, the research focus
/// and a thesis are not this class.
pub(super) fn writes_later_turn_instruction(method: crate::broker::ToolMethod) -> bool {
    use crate::broker::tool_class::PersistentWrite;
    use crate::broker::ToolMethod;
    if method.safety().write != PersistentWrite::Text {
        return false;
    }
    match method {
        ToolMethod::BotMemory | ToolMethod::PrincipleSave => true,
        ToolMethod::NoteSave
        | ToolMethod::JournalSave
        | ToolMethod::JournalReview
        | ToolMethod::LensFile
        | ToolMethod::WorldSchemaUpdate
        | ToolMethod::LoupistSetup
        | ToolMethod::ThesisUpdate => false,
        _ => unreachable!("a PersistentWrite::Text tool needs a later-instruction decision"),
    }
}

/// Refuse an instruction a later turn would receive once this reply has read
/// third-party text. Bot memory may still resolve and forget.
pub(super) fn refuse_later_instruction(
    method: crate::broker::ToolMethod,
    taint: &crate::custom_connections::taint::TurnTaint,
) -> Result<(), crate::broker::BrokerError> {
    if !writes_later_turn_instruction(method) || !taint.has_untrusted_content() {
        return Ok(());
    }
    Err(crate::broker::BrokerError::UntrustedTextPaused
        .with_next(BOT_MEMORY_AFTER_UNTRUSTED_REFUSAL))
}

/// Active Lens principles whose statement folds to one of `texts`. Read only.
fn lens_principle_twins(
    state: &crate::app::AppState,
    loupist_id: &str,
    texts: &[String],
) -> Vec<Value> {
    if texts.is_empty() {
        return Vec::new();
    }
    let Ok(principles) = crate::commands::knowledge::principles::list(
        state,
        crate::commands::knowledge::principles::PrincipleListRequest {
            loupist_id: loupist_id.to_owned(),
            include_archived: Some(false),
        },
    ) else {
        return Vec::new();
    };
    let mut seen = BTreeSet::new();
    principles
        .into_iter()
        .filter(|principle| {
            seen.insert(principle.record_id.clone())
                && texts.iter().any(|text| {
                    crate::knowledge::principles::same_folded_text(&principle.statement, text)
                })
        })
        .map(|principle| {
            json!({
                "recordId": principle.record_id,
                "title": crate::knowledge::principles::principle_title(&principle.statement),
            })
        })
        .collect()
}

/// The `bot_memory` calls of one reply. A call reserves its changes under
/// this lock when it is checked, so parallel calls see each other; an
/// undelivered call frees its share, and only delivered calls commit.
#[derive(Default)]
pub(in crate::commands) struct BotMemoryReservations {
    calls: Vec<(String, bool, Vec<StagedBotMemory>)>,
}

impl BotMemoryReservations {
    pub(super) fn has_delivered(&self) -> bool {
        self.calls
            .iter()
            .any(|(_, delivered, changes)| *delivered && !changes.is_empty())
    }

    fn reserved(&self) -> Vec<StagedBotMemory> {
        self.calls
            .iter()
            .flat_map(|(_, _, changes)| changes.iter().cloned())
            .collect()
    }

    pub(in crate::commands) fn delivered(&self) -> Vec<StagedBotMemory> {
        self.calls
            .iter()
            .filter(|(_, delivered, _)| *delivered)
            .flat_map(|(_, _, changes)| changes.iter().cloned())
            .collect()
    }

    pub(super) fn deliver(&mut self, delivery_id: &str) {
        for (id, delivered, _) in &mut self.calls {
            if id == delivery_id {
                *delivered = true;
            }
        }
    }

    pub(super) fn discard(&mut self, delivery_id: &str) {
        self.calls
            .retain(|(id, delivered, _)| id != delivery_id || *delivered);
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    changes: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeInput {
    action: String,
    id: Option<String>,
    kind: Option<String>,
    text: Option<String>,
    #[serde(default)]
    basis: Vec<String>,
    #[serde(rename = "match")]
    words: Option<String>,
}

fn invalid(field: impl Into<String>, expected: &str) -> BrokerError {
    BrokerError::InvalidArguments {
        field: field.into(),
        expected: expected.into(),
    }
}

fn parse(params: Value, current_message: &str) -> Result<Vec<StagedBotMemory>, BrokerError> {
    let input: Input = serde_json::from_value(params)
        .map_err(|_| invalid("bot_memory", "changes as the tool describes"))?;
    if input.changes.is_empty() || input.changes.len() > MAX_TURN_CHANGES {
        return Err(invalid("changes", "1 to 30 changes"));
    }
    let mut staged = Vec::with_capacity(input.changes.len());
    for (index, value) in input.changes.into_iter().enumerate() {
        let at = |key: &str| format!("changes[{index}]{key}");
        let change: ChangeInput = serde_json::from_value(value)
            .map_err(|_| invalid(at(""), "{action, id?, kind?, text?, basis?}"))?;
        let text = || {
            change
                .text
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| invalid(at(".text"), "1–300 characters"))
        };
        let mut basis = Vec::new();
        for id in &change.basis {
            if !basis.contains(id) {
                basis.push(id.clone());
            }
        }
        if basis.len() > MAX_BASIS {
            return Err(invalid(at(".basis"), "at most 4 message ids"));
        }
        if basis.is_empty() {
            basis.push(current_message.to_owned());
        }
        let id = || {
            change
                .id
                .clone()
                .ok_or_else(|| invalid(at(".id"), "an entry id from bot_memory.entries"))
        };
        let (entry_id, action) = match change.action.as_str() {
            "remember" => {
                if change.id.is_some() {
                    return Err(invalid(at(".id"), "no id: remember starts a new entry"));
                }
                let kind = change
                    .kind
                    .as_deref()
                    .and_then(BotMemoryKind::parse)
                    .ok_or_else(|| invalid(at(".kind"), "preference, decision, open or context"))?;
                let id = format!("bm-{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
                let note = BotMemoryNote {
                    kind,
                    text: text()?,
                    basis,
                };
                (id, BotMemoryAction::Remember(note))
            }
            "update" => (
                id()?,
                BotMemoryAction::Update {
                    text: text()?,
                    basis,
                },
            ),
            "resolve" => (id()?, BotMemoryAction::Resolve),
            "forget" => match (&change.id, change.words.as_deref().map(str::trim)) {
                (Some(id), None) => (id.clone(), BotMemoryAction::Forget),
                (None, Some(words)) if !words.is_empty() && words.chars().count() <= 100 => (
                    String::new(),
                    BotMemoryAction::ForgetMatching(words.to_owned()),
                ),
                _ => {
                    return Err(invalid(
                        at(""),
                        "forget takes an id or match (1–100 characters), not both",
                    ))
                }
            },
            _ => {
                return Err(invalid(
                    at(".action"),
                    "remember, update, resolve or forget",
                ))
            }
        };
        if !matches!(
            action,
            BotMemoryAction::Remember(_) | BotMemoryAction::Update { .. }
        ) && (change.kind.is_some() || change.text.is_some() || !change.basis.is_empty())
        {
            return Err(invalid(at(""), "only an id for resolve and forget"));
        }
        if change.words.is_some() && !matches!(action, BotMemoryAction::ForgetMatching(_)) {
            return Err(invalid(at(".match"), "match only with forget"));
        }
        if matches!(action, BotMemoryAction::Update { .. }) && change.kind.is_some() {
            return Err(invalid(
                at(".kind"),
                "no kind: an entry keeps its kind; resolve it and remember a new one",
            ));
        }
        staged.push(StagedBotMemory { entry_id, action });
    }
    Ok(staged)
}

impl AppToolExecutor {
    pub(super) async fn stage_bot_memory(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if !self
            .capture
            .surface
            .as_ref()
            .is_some_and(crate::chat::surface::ChatSurface::is_bot)
        {
            return Err(BrokerError::Execution(format!(
                "bot_memory is unavailable in a {} turn: only the Bot keeps memory of its conversation",
                self.capture
                    .surface
                    .as_ref()
                    .map_or("host", crate::chat::surface::ChatSurface::name)
            )));
        }
        if !policy.use_memory {
            return Err(BrokerError::MemoryDisabled);
        }
        if !policy.propose_memory_updates {
            return Err(BrokerError::ProposalDisabled);
        }
        let (Some(reply_id), Some(current_message)) = (
            self.capture.source_message_id.as_deref(),
            self.capture.principle_user_message_id.as_deref(),
        ) else {
            return Err(BrokerError::invalid("bot_memory", "This reply has no current Bot message to save from. Continue without saving and tell the user it was not saved."));
        };
        let staged = parse(params, current_message)?;
        let saves = staged.iter().any(|change| {
            matches!(
                change.action,
                BotMemoryAction::Remember(_) | BotMemoryAction::Update { .. }
            )
        });
        if saves {
            refuse_later_instruction(
                crate::broker::ToolMethod::BotMemory,
                &self.capture.turn_taint,
            )?;
        }
        let chat = self
            .state
            .store
            .get_chat(&policy.session_id)
            .map_err(store_error)?
            .filter(|chat| chat.loupist_id == self.loupist_id && crate::chat::home::is_home(chat))
            .ok_or_else(|| BrokerError::invalid("bot_memory", "Not the bound Bot Chat for this Loupist. Continue without saving and tell the user it was not saved."))?;
        // Check against every call this reply has reserved, delivered or not,
        // and reserve under the same lock.
        let mut reservations = self.capture.staged_bot_memory.lock().await;
        let mut memory = chat.bot_memory.clone();
        let earlier = reservations.reserved();
        if earlier.len() + staged.len() > MAX_TURN_CHANGES {
            return Err(invalid("changes", "at most 30 changes in one reply"));
        }
        let at_ms = crate::commands::now_ms();
        memory
            .apply(&chat.messages, reply_id, at_ms, &earlier)
            .map_err(BrokerError::Execution)?;
        // Counts only: what a match finds may be resolved, on another version
        // or later than an as-of date, so its words never return to the model.
        // The forgotten lines are matched to Lens principles before the apply
        // drops them. The principles are not deleted.
        let shown = memory.entries(|change| {
            change.reply_id == reply_id
                || chat
                    .messages
                    .iter()
                    .any(|message| message.id == change.reply_id)
        });
        let forgotten = staged
            .iter()
            .map(|change| match &change.action {
                BotMemoryAction::ForgetMatching(words) => {
                    let ids = memory.matching(words);
                    let texts = shown
                        .iter()
                        .filter(|entry| ids.contains(&entry.id))
                        .map(|entry| entry.text.clone())
                        .collect::<Vec<_>>();
                    (Some(ids.len()), texts)
                }
                BotMemoryAction::Forget => (
                    None,
                    shown
                        .iter()
                        .filter(|entry| entry.id == change.entry_id)
                        .map(|entry| entry.text.clone())
                        .collect(),
                ),
                _ => (None, Vec::new()),
            })
            .collect::<Vec<_>>();
        memory
            .apply(&chat.messages, reply_id, at_ms, &staged)
            .map_err(BrokerError::Execution)?;
        let entries = memory.entries(|change| {
            change.reply_id == reply_id
                || chat
                    .messages
                    .iter()
                    .any(|message| message.id == change.reply_id)
        });
        let result = json!({
            "status": "staged",
            "durable": false,
            "changed": staged.iter().zip(&forgotten).map(|(change, forgotten)| {
                let (matched, texts) = forgotten;
                let action = match &change.action {
                    BotMemoryAction::Remember(_) => "remember",
                    BotMemoryAction::Update { .. } => "update",
                    BotMemoryAction::Resolve => "resolve",
                    BotMemoryAction::Forget | BotMemoryAction::ForgetMatching(_) => "forget",
                };
                let mut value = if let Some(count) = matched {
                    json!({"action": action, "forgotten": count})
                } else {
                    json!({"id": change.entry_id, "action": action})
                };
                if matches!(
                    change.action,
                    BotMemoryAction::Forget | BotMemoryAction::ForgetMatching(_)
                ) {
                    let twins = lens_principle_twins(&self.state, &self.loupist_id, texts);
                    if !twins.is_empty() {
                        value["principles"] = json!(twins);
                    }
                }
                value
            }).collect::<Vec<_>>(),
            "kept": entries.len(),
            "room": crate::chat::bot_memory::MAX_ENTRIES.saturating_sub(entries.len()),
            "notice": "Applies when this reply finishes. Tell the user plainly what you will remember or forget when they asked for it; do not mention entry ids.",
        });
        reservations
            .calls
            .push((delivery_id.to_owned(), false, staged));
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_defaults_the_basis_to_the_current_message_and_rejects_mixed_fields() {
        let staged = parse(
            json!({"changes": [
                {"action": "remember", "kind": "preference", "text": " Answers in KRW "},
                {"action": "update", "id": "bm-a", "text": "Answers in USD", "basis": ["m1", "m1"]},
                {"action": "forget", "id": "bm-b"},
            ]}),
            "m-current",
        )
        .unwrap();
        let BotMemoryAction::Remember(note) = &staged[0].action else {
            panic!("{staged:?}");
        };
        assert!(staged[0].entry_id.starts_with("bm-"));
        assert_eq!(note.text, "Answers in KRW");
        assert_eq!(note.basis, ["m-current"]);
        assert_eq!(
            staged[1].action,
            BotMemoryAction::Update {
                text: "Answers in USD".into(),
                basis: vec!["m1".into()],
            }
        );
        assert_eq!(staged[2].action, BotMemoryAction::Forget);

        for (bad, field) in [
            (json!({"changes": []}), "changes"),
            (
                json!({"changes": [{"action": "remember", "text": "x"}]}),
                "changes[0].kind",
            ),
            (
                json!({"changes": [{"action": "remember", "kind": "preference", "text": "  "}]}),
                "changes[0].text",
            ),
            (
                json!({"changes": [{"action": "remember", "id": "bm-a", "kind": "open", "text": "x"}]}),
                "changes[0].id",
            ),
            (
                json!({"changes": [{"action": "resolve", "id": "bm-a", "text": "x"}]}),
                "changes[0]",
            ),
            (
                json!({"changes": [{"action": "update", "id": "bm-a", "kind": "open", "text": "x"}]}),
                "changes[0].kind",
            ),
            (
                json!({"changes": [{"action": "erase", "id": "bm-a"}]}),
                "changes[0].action",
            ),
            (
                json!({"changes": [{"action": "remember", "kind": "open", "text": "x", "basis": ["a", "b", "c", "d", "e"]}]}),
                "changes[0].basis",
            ),
        ] {
            let error = parse(bad, "m-current").unwrap_err();
            assert!(
                matches!(&error, BrokerError::InvalidArguments { field: got, .. } if got == field),
                "{error:?}"
            );
        }
    }
}
