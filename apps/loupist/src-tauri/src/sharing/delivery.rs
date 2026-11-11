//! How text that came from a shared pack reaches the model. Every
//! model-facing field a pack supplied — accepted principles, perspective and
//! methods, their titles in the per-turn catalog and related-memory pointers,
//! lens and World search hits, and the shared names, relationships and
//! reference Sources in World reads — arrives inside `<<shared …>>` delimiters
//! that a locked SYSTEM rule explains.
//!
//! Delivery raises the turn's untrusted-text tier (a command asks the user in
//! a card for the rest of the turn) when the model receives a shared item that had a scan finding,
//! or a shared item's full text: a `lens_read` of a shared method or principle,
//! shared names, relationships or summaries from a World read or search, or a
//! pack Source's material. The per-turn context (principles, perspective,
//! catalog and related-memory pointers) of clean items the user accepted is
//! wrapped but pauses nothing, so a Loupist made from a pack stays usable. If
//! the shared lookup fails, every Memory delivery is treated as shared and
//! flagged: wrapped, and the tier raised.
use std::collections::BTreeMap;

use serde_json::Value;

use super::store::SharedWorld;
use crate::app::AppState;
use crate::custom_connections::taint::TurnTaint;

/// The `from` label when the shared lookup itself failed.
const UNKNOWN_PACK: &str = "a shared Loupist";
/// Fields of a delivered Memory object that carry a pack's own words.
const INLINE_FIELDS: [&str; 7] = [
    "name",
    "description",
    "proposition",
    "attribution",
    "title",
    "snippet",
    "summary",
];

fn source_label(pack: &str) -> String {
    pack.chars()
        .filter(|c| {
            !c.is_control() && !super::scan::hidden_character(*c) && !matches!(c, '"' | '<' | '>')
        })
        .take(120)
        .collect()
}

/// `<<shared id=… from="…">>` … `<<end shared id=…>>`. The id is unknown to
/// the pack author, so pack text cannot close the block early.
pub(crate) fn delimit(text: &str, pack: &str, id: &str) -> String {
    format!(
        "<<shared id={id} from=\"{}\">>\n{text}\n<<end shared id={id}>>",
        source_label(pack)
    )
}

/// The same delimiters around one short field (a name, title or snippet).
fn delimit_inline(text: &str, pack: &str, id: &str) -> String {
    format!(
        "<<shared id={id} from=\"{}\">>{text}<<end shared id={id}>>",
        source_label(pack)
    )
}

fn fresh_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_owned()
}

/// Derived from local record ids the author never sees; stable across turns.
fn stable_id(seed: &str) -> String {
    crate::util::sha256(seed.as_bytes())[..12].to_owned()
}

#[derive(Debug, Clone, Default)]
struct Known {
    /// Accepted Lens record → (pack, accepted despite a scan finding).
    lens: BTreeMap<String, (String, bool)>,
    world: SharedWorld,
}

/// Who supplied one delivered object, and whether its short fields alone
/// raise the tier (a flagged Lens item, or any map text).
struct Owner {
    pack: String,
    pauses: bool,
}

/// What a Loupist received from shared packs, for one delivery.
#[derive(Debug, Clone)]
pub(crate) struct Shared {
    /// None when the lookup failed: everything is treated as shared.
    known: Option<Known>,
}

impl Shared {
    pub(crate) fn load(state: &AppState, loupist_id: &str) -> Self {
        let known = (|| {
            let mut connection = state.store.lock_loupist(loupist_id).ok()?;
            let tx = connection.transaction().ok()?;
            let lens = super::store::shared_lens(&tx, loupist_id).ok()?;
            let world = super::store::shared_world(&tx, loupist_id).ok()?;
            tx.commit().ok()?;
            Some(Known { lens, world })
        })();
        Self { known }
    }

    #[cfg(test)]
    pub(crate) fn failed() -> Self {
        Self { known: None }
    }

    /// The pack an accepted Lens record (method, principle, perspective) came
    /// from; every record when the lookup failed.
    pub(crate) fn lens(&self, record_id: &str) -> Option<&str> {
        match &self.known {
            None => Some(UNKNOWN_PACK),
            Some(known) => known.lens.get(record_id).map(|(pack, _)| pack.as_str()),
        }
    }

    /// Whether a shared Lens record was accepted despite a scan finding (every
    /// record when the lookup failed).
    pub(crate) fn flagged(&self, record_id: &str) -> bool {
        match &self.known {
            None => true,
            Some(known) => known
                .lens
                .get(record_id)
                .is_some_and(|(_, flagged)| *flagged),
        }
    }

    /// Wraps each shared principle's line in the User principles block; true
    /// when a flagged one was delivered (the tier rises).
    pub(crate) fn wrap_principles(
        &self,
        block: String,
        principles: &[crate::knowledge::Principle],
    ) -> (String, bool) {
        if block.is_empty() {
            return (block, false);
        }
        let mut pauses = false;
        let mut lines: Vec<String> = block.split('\n').map(str::to_owned).collect();
        for principle in principles {
            let Some(pack) = self.lens(&principle.record_id) else {
                continue;
            };
            let prefix = format!("- [lens:{}] ", principle.record_id);
            if let Some(line) = lines.iter_mut().find(|line| line.starts_with(&prefix)) {
                let id = stable_id(&format!(
                    "{}:{}",
                    principle.record_id, principle.revision_id
                ));
                *line = delimit(line, pack, &id);
                pauses |= self.flagged(&principle.record_id);
            }
        }
        (lines.join("\n"), pauses)
    }

    /// The standing perspective body, wrapped when a shared pack supplied it;
    /// true when it was accepted despite a finding (the tier rises).
    pub(crate) fn wrap_charter(&self, record_id: Option<&str>, charter: String) -> (String, bool) {
        match record_id.and_then(|id| self.lens(id).map(|pack| (id, pack))) {
            Some((id, pack)) => {
                let tag = stable_id(&format!("{id}:{}", charter.len()));
                (delimit(&charter, pack, &tag), self.flagged(id))
            }
            None => (charter, false),
        }
    }

    /// One short field of a shared Lens record (a catalog title or summary, a
    /// pointer snippet), wrapped with an id stable for that record and field.
    /// It raises the tier only when the record is flagged (see [`Self::flagged`]).
    pub(crate) fn wrap_lens_field(
        &self,
        record_id: &str,
        field: &str,
        text: &str,
    ) -> Option<String> {
        self.lens(record_id).map(|pack| {
            delimit_inline(
                text,
                pack,
                &stable_id(&format!("{record_id}:{field}:{text}")),
            )
        })
    }

    /// The pack that owns one delivered object: a shared Lens record or
    /// reference Source by `recordId`, a shared name or relationship by its id,
    /// or a shared evidence span. Map text and pack Sources always pause; a
    /// Lens record's short fields pause only when it is flagged.
    fn owner(&self, map: &serde_json::Map<String, Value>) -> Option<Owner> {
        let text = |key: &str| map.get(key).and_then(Value::as_str);
        let Some(known) = &self.known else {
            let identified = [
                "recordId",
                "id",
                "adapter_id",
                "objectId",
                "spanId",
                "span_id",
            ]
            .iter()
            .any(|key| text(key).is_some());
            return identified.then(|| Owner {
                pack: UNKNOWN_PACK.to_owned(),
                pauses: true,
            });
        };
        let owner = |pack: &String, pauses: bool| Owner {
            pack: pack.clone(),
            pauses,
        };
        if let Some(record) = text("recordId") {
            if let Some((pack, flagged)) = known.lens.get(record) {
                return Some(owner(pack, *flagged));
            }
            if let Some(pack) = known.world.sources.get(record) {
                return Some(owner(pack, true));
            }
        }
        for key in ["id", "adapter_id", "objectId"] {
            if let Some(pack) = text(key).and_then(|id| known.world.subjects.get(id)) {
                return Some(owner(pack, true));
            }
        }
        for key in ["spanId", "span_id"] {
            if let Some(pack) = text(key).and_then(|id| known.world.spans.get(id)) {
                return Some(owner(pack, true));
            }
        }
        None
    }

    fn walk(&self, value: &mut Value, inherited: Option<&Owner>, delivery: &mut Delivery) {
        match value {
            Value::Array(items) => {
                for item in items {
                    self.walk(item, inherited, delivery);
                }
            }
            Value::Object(map) => {
                let own = self.owner(map);
                let owner = own.as_ref().or(inherited);
                let world_view = map.get("scope").and_then(Value::as_str) == Some("world");
                for (key, child) in map.iter_mut() {
                    match (key.as_str(), child) {
                        // A World read's view travels as JSON text: wrap inside it.
                        ("text", Value::String(text)) if world_view => {
                            if let Ok(mut view) = serde_json::from_str::<Value>(text) {
                                let mut inner = Delivery::default();
                                self.walk(&mut view, None, &mut inner);
                                if inner.wrapped {
                                    *text = view.to_string();
                                    delivery.wrapped = true;
                                    delivery.pauses |= inner.pauses;
                                }
                            }
                        }
                        // A full body (a shared method or principle, a pack Source's material).
                        ("text", Value::String(text)) => {
                            if let Some(owner) = owner {
                                *text = delimit(text, &owner.pack, &fresh_id());
                                delivery.wrapped = true;
                                delivery.pauses = true;
                            }
                        }
                        (field, Value::String(text)) if INLINE_FIELDS.contains(&field) => {
                            if let Some(owner) = owner {
                                wrap_inline(text, owner, delivery);
                            }
                        }
                        ("aliases", Value::Array(items)) => {
                            if let Some(owner) = owner {
                                for item in items {
                                    if let Value::String(text) = item {
                                        wrap_inline(text, owner, delivery);
                                    }
                                }
                            }
                        }
                        ("qualifiers", Value::Array(items)) => {
                            if let Some(owner) = owner {
                                for item in items {
                                    if let Some(Value::String(text)) = item.get_mut("value") {
                                        wrap_inline(text, owner, delivery);
                                    }
                                }
                            }
                        }
                        (_, child) => self.walk(child, owner, delivery),
                    }
                }
            }
            _ => {}
        }
    }

    /// Wraps every shared field of one Memory tool delivery (lens and World
    /// search and reads); true when the delivery raises the tier.
    pub(crate) fn mark_delivery(&self, delivered: &mut Value) -> bool {
        let mut delivery = Delivery::default();
        self.walk(delivered, None, &mut delivery);
        delivery.pauses
    }
}

#[derive(Default)]
struct Delivery {
    wrapped: bool,
    pauses: bool,
}

fn wrap_inline(text: &mut String, owner: &Owner, delivery: &mut Delivery) {
    *text = delimit_inline(text, &owner.pack, &fresh_id());
    delivery.wrapped = true;
    delivery.pauses |= owner.pauses;
}

/// The model-facing copy of a Memory tool result: shared text wrapped, and the
/// untrusted-text tier raised before the result is returned when it pauses.
pub(crate) fn deliver(
    state: &AppState,
    loupist_id: &str,
    delivered: &mut Value,
    taint: &TurnTaint,
) {
    if Shared::load(state, loupist_id).mark_delivery(delivered) {
        taint.mark_untrusted_text();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn principle(id: &str, statement: &str) -> crate::knowledge::Principle {
        crate::knowledge::Principle {
            record_id: id.into(),
            revision_id: format!("rev_{id}"),
            loupist_id: "loupist-a".into(),
            statement: statement.into(),
            why: None,
            example: None,
            origin: "shared".into(),
            origin_chat_id: None,
            origin_message_id: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            archived: false,
        }
    }

    #[test]
    fn old_stored_pack_labels_drop_hidden_characters_at_delivery() {
        let wrapped = delimit(
            "Shared text",
            "Chip\u{2028}\u{2029}\u{202E}\u{2060} basics",
            "fixture",
        );
        assert!(wrapped.contains("from=\"Chip basics\""));
        assert!(!wrapped.chars().any(super::super::scan::hidden_character));
    }

    #[test]
    fn shared_lines_are_delimited_and_a_failed_lookup_treats_everything_as_flagged() {
        let shared = Shared {
            known: Some(Known {
                lens: BTreeMap::from([
                    ("rec_shared".to_owned(), ("Chip basics".to_owned(), false)),
                    ("rec_flagged".to_owned(), ("Chip basics".to_owned(), true)),
                ]),
                world: SharedWorld::default(),
            }),
        };
        let principles = [
            principle("rec_own", "Mine"),
            principle("rec_shared", "Theirs"),
            principle("rec_flagged", "Flagged"),
        ];
        let block =
            "User principles\n- [lens:rec_own] Mine — \n- [lens:rec_shared] Theirs — ".to_owned();
        let (wrapped, pauses) = shared.wrap_principles(block.clone(), &principles[..2]);
        assert!(
            !pauses,
            "a clean accepted principle is wrapped but pauses nothing"
        );
        let lines: Vec<&str> = wrapped.split('\n').collect();
        assert_eq!(lines[1], "- [lens:rec_own] Mine — ");
        assert!(lines[2].starts_with("<<shared id="));
        assert!(lines[2].ends_with("from=\"Chip basics\">>"));
        assert_eq!(lines[3], "- [lens:rec_shared] Theirs — ");
        assert!(lines[4].starts_with("<<end shared id="));
        let flagged_block = format!("{block}\n- [lens:rec_flagged] Flagged — ");
        assert!(shared.wrap_principles(flagged_block, &principles).1);

        // A failed lookup cannot tell shared text apart: all of it is wrapped and pauses.
        let failed = Shared::failed();
        let (wrapped, pauses) = failed.wrap_principles(block, &principles[..2]);
        assert!(pauses);
        assert_eq!(wrapped.matches("<<end shared id=").count(), 2);
        let mut read = serde_json::json!({"scope": "lens", "recordId": "rec_own", "text": "Steps"});
        assert!(failed.mark_delivery(&mut read));
        assert!(read["text"].as_str().unwrap().starts_with("<<shared id="));
    }
}
