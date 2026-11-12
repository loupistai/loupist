use rusqlite::Transaction;

use super::*;
use crate::knowledge::{
    ExtensionCategory, ExtensionField, ExtensionLabels, ExtensionRelation, ExtensionType,
};

#[derive(Clone, Debug)]
pub(in crate::commands) struct StagedVocabulary {
    pub types: Vec<ExtensionType>,
    pub relations: Vec<ExtensionRelation>,
    pub user_message_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolLabels {
    en: String,
    ko: Option<String>,
}

impl From<ToolLabels> for ExtensionLabels {
    fn from(labels: ToolLabels) -> Self {
        Self {
            en: labels.en,
            ko: labels.ko,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolType {
    name: String,
    description: String,
    category: Option<ExtensionCategory>,
    labels: ToolLabels,
    blurbs: Option<ToolLabels>,
    #[serde(default)]
    fields: Vec<ExtensionField>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolRelation {
    name: String,
    description: String,
    signatures: Vec<(String, String)>,
    labels: ToolLabels,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    types: Vec<ToolType>,
    #[serde(default)]
    relations: Vec<ToolRelation>,
}

impl AppToolExecutor {
    pub(super) async fn stage_world_schema_update(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if !policy.propose_memory_updates || !policy.use_memory {
            return Err(BrokerError::MemoryDisabled);
        }
        let invalid = || BrokerError::InvalidArguments {
            field: "world_schema_update".into(),
            expected: "types and relations as the tool describes".into(),
        };
        let request: Input = serde_json::from_value(params).map_err(|_| invalid())?;
        if request.types.is_empty() && request.relations.is_empty() {
            return Err(invalid());
        }
        let types: Vec<ExtensionType> = request
            .types
            .into_iter()
            .map(|item| ExtensionType {
                category: item.category.unwrap_or(ExtensionCategory::Other),
                blurbs: item
                    .blurbs
                    .map_or_else(|| item.labels.clone_labels(), ExtensionLabels::from),
                labels: item.labels.into(),
                name: item.name,
                description: item.description,
                fields: item.fields,
            })
            .collect();
        let relations: Vec<ExtensionRelation> = request
            .relations
            .into_iter()
            .map(|item| ExtensionRelation {
                name: item.name,
                description: item.description,
                signatures: item.signatures,
                labels: item.labels.into(),
            })
            .collect();
        let live = self
            .live_extension(&policy.loupist_id)
            .map_err(knowledge_error)?;
        let staged = self.capture.staged_vocabulary.lock().await;
        let mut preview = live;
        for earlier in staged.iter() {
            preview.types.extend(earlier.types.iter().cloned());
            preview.relations.extend(earlier.relations.iter().cloned());
        }
        if types
            .iter()
            .any(|item| crate::knowledge::type_already_known(&item.name, &preview))
            || relations
                .iter()
                .any(|item| crate::knowledge::relation_already_known(&item.name, &preview))
        {
            return Err(BrokerError::Execution(
                "The map already tracks that name. Pick a different one.".into(),
            ));
        }
        preview.types.extend(types.iter().cloned());
        preview.relations.extend(relations.iter().cloned());
        crate::knowledge::validate_extension(&preview).map_err(knowledge_error)?;
        drop(staged);
        let names: Vec<String> = types
            .iter()
            .map(|item| item.name.clone())
            .chain(relations.iter().map(|item| item.name.clone()))
            .collect();
        let record = StagedVocabulary {
            types,
            relations,
            user_message_id: self.capture.principle_user_message_id.clone(),
        };
        self.capture
            .stage(delivery_id, |pending| pending.vocabulary.push(record))
            .await;
        Ok(json!({"status": "staged", "kinds": names, "durable": false}))
    }

    fn live_extension(
        &self,
        loupist_id: &str,
    ) -> Result<crate::knowledge::WorldExtension, crate::knowledge::KnowledgeError> {
        let connection = self
            .state
            .store
            .lock_loupist(loupist_id)
            .map_err(|_| crate::knowledge::KnowledgeError::LockPoisoned)?;
        crate::knowledge::live_extension(&connection, loupist_id)
    }
}

impl ToolLabels {
    fn clone_labels(&self) -> ExtensionLabels {
        ExtensionLabels {
            en: self.en.clone(),
            ko: self.ko.clone(),
        }
    }
}

pub(crate) fn commit_turn(
    tx: &Transaction<'_>,
    loupist_id: &str,
    chat_id: &str,
    at_ms: i64,
    staged: &[StagedVocabulary],
) -> Result<Vec<crate::domain::MapKind>, crate::knowledge::KnowledgeError> {
    let mut skipped = Vec::new();
    for record in staged {
        let message = record.user_message_id.as_deref().unwrap_or_default();
        let mut provenance = crate::knowledge::Provenance::new(
            crate::knowledge::Producer::Chat,
            json!({ "said": message }),
        );
        provenance.origin = (!message.is_empty()).then_some((chat_id, message));
        match crate::knowledge::add_kinds(
            tx,
            loupist_id,
            record.types.clone(),
            record.relations.clone(),
            provenance,
            at_ms,
        ) {
            Ok(_) => {}
            Err(
                crate::knowledge::KnowledgeError::KindAlreadyKnown
                | crate::knowledge::KnowledgeError::Invalid(_),
            ) => {
                let types = record.types.iter().map(|item| (&item.name, &item.labels));
                let relations = record
                    .relations
                    .iter()
                    .map(|item| (&item.name, &item.labels));
                skipped.extend(types.chain(relations).map(|(name, labels)| {
                    crate::domain::MapKind {
                        name: name.clone(),
                        label: labels.en.clone(),
                        label_ko: labels.ko.clone(),
                    }
                }));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(skipped)
}
