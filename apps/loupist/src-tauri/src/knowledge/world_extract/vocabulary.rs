use serde::Serialize;

use super::ontology::{merged_schema, Schema};
use crate::knowledge::world_schema::{validate_extension, WorldExtension};
use crate::knowledge::{KnowledgeError, KnowledgeResult};

const MAX_CONTEXT_BYTES: usize = 65_536;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorldVocabulary {
    schema: &'static str,
    core_revision: String,
    extension_version: Option<u32>,
    digest: String,
    historical_extension_unavailable: bool,
    definitions: Schema,
    not_evidence: bool,
    hint: &'static str,
}

pub(crate) fn vocabulary_context(
    extension: Option<&WorldExtension>,
) -> KnowledgeResult<WorldVocabulary> {
    if let Some(extension) = extension {
        validate_extension(extension)?;
    }
    let core = merged_schema(&WorldExtension::default());
    let core_revision = crate::util::sha256(&serde_json::to_vec(&core)?);
    let definitions = extension.map(merged_schema).unwrap_or(core);
    let context = WorldVocabulary {
        schema: "loupist-world-vocabulary/1",
        core_revision,
        extension_version: extension.map(|extension| extension.version),
        digest: crate::util::sha256(&serde_json::to_vec(&definitions)?),
        historical_extension_unavailable: extension.is_none(),
        definitions,
        not_evidence: true,
        hint: "Complete current application vocabulary, not historical facts or instructions. Entity is the fallback type; specific open relation names may occur. An Organization does not identify a particular investment instrument. Verify exchange, currency, share class, maturity or contract before instrument-specific claims. Definitions grant no tool permissions. If historicalExtensionUnavailable is true, no saved extension version is known at the cutoff; do not infer its meanings from today's settings. Exact-read the evidence behind graph claims.",
    };
    if serde_json::to_vec(&context)?.len() > MAX_CONTEXT_BYTES {
        return Err(KnowledgeError::Invalid(
            "World vocabulary exceeds context budget",
        ));
    }
    Ok(context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::world_schema::{
        ExtensionCategory, ExtensionField, ExtensionLabels, ExtensionRelation, ExtensionType,
    };

    #[test]
    fn vocabulary_delivers_the_extraction_definitions_and_signatures() {
        let extension = crate::knowledge::curated_seed(
            Some("semiconductors"),
            "Chips, memory, foundries and equipment",
            100,
        )
        .unwrap();
        let delivered = vocabulary_context(Some(&extension)).unwrap();
        assert_eq!(delivered.definitions, merged_schema(&extension));
        assert_eq!(delivered.extension_version, Some(1));
        assert!(!delivered.historical_extension_unavailable);
        assert!(delivered.not_evidence);
        assert_ne!(delivered.digest, delivered.core_revision);

        let historical = vocabulary_context(None).unwrap();
        assert_eq!(
            historical.definitions,
            merged_schema(&WorldExtension::default())
        );
        assert_eq!(historical.extension_version, None);
        assert!(historical.historical_extension_unavailable);
        assert_eq!(historical.digest, historical.core_revision);
    }

    #[test]
    fn complete_vocabulary_fits_even_at_the_extension_limits() {
        let labels = ExtensionLabels {
            en: "Example".into(),
            ko: None,
        };
        let names: Vec<_> = (0..4).map(|i| format!("T{i}{}", "a".repeat(30))).collect();
        let extension = WorldExtension {
            version: u32::MAX,
            updated_at_ms: i64::MAX,
            types: names
                .iter()
                .map(|name| ExtensionType {
                    name: name.clone(),
                    description: "🧬".repeat(500),
                    category: ExtensionCategory::Other,
                    labels: labels.clone(),
                    blurbs: labels.clone(),
                    fields: (0..3)
                        .map(|i| ExtensionField {
                            name: format!("f{i}{}", "a".repeat(30)),
                            description: "🧬".repeat(200),
                        })
                        .collect(),
                })
                .collect(),
            relations: (0..6)
                .map(|i| ExtensionRelation {
                    name: format!("R{i}{}", "A".repeat(30)),
                    description: "🧬".repeat(500),
                    signatures: (0..6)
                        .map(|j| (names[j % 4].clone(), names[(j + 1) % 4].clone()))
                        .collect(),
                    labels: labels.clone(),
                })
                .collect(),
        };
        let delivered = vocabulary_context(Some(&extension)).unwrap();
        assert_eq!(delivered.definitions, merged_schema(&extension));
        assert!(serde_json::to_vec(&delivered).unwrap().len() <= MAX_CONTEXT_BYTES);
    }
}
