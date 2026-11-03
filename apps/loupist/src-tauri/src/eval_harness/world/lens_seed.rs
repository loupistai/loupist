//! Import exact learned methods into disposable evaluation Loupists. Only declared
//! Source record/revision identities may change; normal Lens authority still applies.

use super::{AppState, Result, Source};
use crate::knowledge::{
    CreateRecord, DependencyRole, EvidenceRevision, KnowledgeError, KnowledgeStore,
    LensChangeInput, LensDocument, RecordKind, RetainedSource, ReviseLens,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SeedLens {
    title: String,
    document: LensDocument,
    original_document_sha256: String,
    #[serde(default)]
    source_links: Vec<SourceLink>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceLink {
    original_record_id: String,
    original_revision_id: String,
    fixture_id: String,
    methodological_dependency: bool,
}

#[derive(Serialize)]
struct MappedSource {
    original_record_id: String,
    original_revision_id: String,
    fixture_id: String,
    record_id: String,
    revision_id: String,
    revision_digest: String,
    methodological_dependency: bool,
}

struct PreparedLens {
    document: LensDocument,
    document_sha256: String,
    roundtrip_document_sha256: String,
    source_links_sha256: String,
    mapped_sources: Vec<MappedSource>,
    dependencies: Vec<EvidenceRevision>,
}

fn document_sha256(document: &LensDocument) -> Result<String> {
    Ok(crate::util::sha256(document.delivery_text()?.as_bytes()))
}

// The sidecar hash uses sorted object keys and preserves the declared array order.
fn source_links_sha256(links: &[SourceLink]) -> Result<String> {
    let rows: Vec<_> = links
        .iter()
        .map(|link| {
            BTreeMap::from([
                ("fixture_id", json!(link.fixture_id)),
                (
                    "methodological_dependency",
                    json!(link.methodological_dependency),
                ),
                ("original_record_id", json!(link.original_record_id)),
                ("original_revision_id", json!(link.original_revision_id)),
            ])
        })
        .collect();
    Ok(crate::util::sha256(&serde_json::to_vec(&rows)?))
}

fn valid_id(value: &str, prefix: &str) -> bool {
    value.strip_prefix(prefix).is_some_and(|suffix| {
        suffix.len() == 32
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

fn id_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
}

fn replacement<'a>(token: &'a str, mapping: &'a BTreeMap<String, String>) -> Result<&'a str> {
    let Some((prefix, suffix)) = token.split_once('_') else {
        return Ok(token);
    };
    if matches!(prefix, "rec" | "rev") {
        if !valid_id(token, &format!("{prefix}_")) {
            return Err("seed_lens_malformed_reference".into());
        }
        return mapping
            .get(token)
            .map(String::as_str)
            .ok_or_else(|| "seed_lens_unknown_reference".into());
    }
    let reserved = matches!(
        prefix,
        "src"
            | "spn"
            | "cap"
            | "rep"
            | "rcp"
            | "dlv"
            | "rsh"
            | "jnl"
            | "snp"
            | "nod"
            | "clm"
            | "gen"
            | "job"
            | "atm"
            | "chg"
            | "dft"
    );
    let other_canonical = prefix.len() == 3
        && prefix.bytes().all(|byte| byte.is_ascii_lowercase())
        && suffix.len() == 32
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase());
    if reserved || other_canonical {
        return Err(format!("seed_lens_unsupported_reference_kind: {prefix}_").into());
    }
    Ok(token)
}

fn remap_text(text: &str, mapping: &BTreeMap<String, String>) -> Result<String> {
    let mut rewritten = String::with_capacity(text.len());
    let mut copied = 0;
    let mut token_start = None;
    for (index, character) in text
        .char_indices()
        .chain(std::iter::once((text.len(), '\0')))
    {
        if id_character(character) {
            token_start.get_or_insert(index);
        } else if let Some(start) = token_start.take() {
            rewritten.push_str(&text[copied..start]);
            rewritten.push_str(replacement(&text[start..index], mapping)?);
            copied = index;
        }
    }
    rewritten.push_str(&text[copied..]);
    Ok(rewritten)
}

fn remap_document(
    document: &LensDocument,
    mapping: &BTreeMap<String, String>,
) -> Result<LensDocument> {
    Ok(LensDocument {
        markdown: remap_text(&document.markdown, mapping)?,
        applicability: remap_text(&document.applicability, mapping)?,
        assumptions: remap_text(&document.assumptions, mapping)?,
        procedure: remap_text(&document.procedure, mapping)?,
        principles: remap_text(&document.principles, mapping)?,
        limitations: remap_text(&document.limitations, mapping)?,
    })
}

fn validate_lens(seed: &SeedLens, fixtures: &BTreeSet<&str>) -> Result<()> {
    if document_sha256(&seed.document)? != seed.original_document_sha256 {
        return Err("seed_lens_original_document_sha256_mismatch".into());
    }
    // Titles are copied verbatim, so canonical identities there are unsupported.
    remap_text(&seed.title, &BTreeMap::new())
        .map_err(|error| format!("seed_lens_title_reference_unsupported: {error}"))?;
    let mut records = BTreeSet::new();
    let mut revisions = BTreeSet::new();
    let mut linked_fixtures = BTreeSet::new();
    let mut identities = BTreeMap::new();
    for link in &seed.source_links {
        if !valid_id(&link.original_record_id, "rec_")
            || !valid_id(&link.original_revision_id, "rev_")
        {
            return Err("seed_lens_source_link_identity_invalid".into());
        }
        if !fixtures.contains(link.fixture_id.as_str()) {
            return Err("seed_lens_source_fixture_missing".into());
        }
        if !records.insert(&link.original_record_id)
            || !revisions.insert(&link.original_revision_id)
            || !linked_fixtures.insert(&link.fixture_id)
        {
            return Err("seed_lens_source_link_ambiguous".into());
        }
        for identity in [&link.original_record_id, &link.original_revision_id] {
            identities.insert(identity.clone(), identity.clone());
        }
    }
    remap_document(&seed.document, &identities)?;
    Ok(())
}

pub(super) fn validate(lenses: &[SeedLens], sources: &[Source]) -> Result<()> {
    let fixtures = sources.iter().map(|source| source.id.as_str()).collect();
    for lens in lenses {
        validate_lens(lens, &fixtures)?;
    }
    Ok(())
}

fn source_map(ingestion: &Value) -> Result<BTreeMap<String, RetainedSource>> {
    let mut sources = BTreeMap::new();
    for item in ingestion
        .as_array()
        .ok_or("seed_lens_source_ingestion_invalid")?
    {
        let fixture = item["fixture_id"]
            .as_str()
            .ok_or("seed_lens_source_ingestion_invalid")?;
        let saved: RetainedSource = serde_json::from_value(item["saved"].clone())?;
        if sources
            .insert(fixture.to_string(), saved.clone())
            .is_some_and(|previous| previous != saved)
        {
            return Err("seed_lens_source_ingestion_ambiguous".into());
        }
    }
    Ok(sources)
}

fn prepare(seed: &SeedLens, sources: &BTreeMap<String, RetainedSource>) -> Result<PreparedLens> {
    validate_lens(seed, &sources.keys().map(String::as_str).collect())?;
    let mut forward = BTreeMap::new();
    let mut reverse = BTreeMap::new();
    let mut mapped_sources = Vec::new();
    let mut dependencies = Vec::new();
    for link in &seed.source_links {
        let saved = sources
            .get(&link.fixture_id)
            .ok_or("seed_lens_source_fixture_missing")?;
        for (original, current) in [
            (&link.original_record_id, &saved.record_id),
            (&link.original_revision_id, &saved.revision_id),
        ] {
            forward.insert(original.clone(), current.clone());
            if reverse.insert(current.clone(), original.clone()).is_some() {
                return Err("seed_lens_source_mapping_not_invertible".into());
            }
        }
        if link.methodological_dependency {
            dependencies.push(EvidenceRevision {
                record_id: saved.record_id.clone(),
                revision_digest: saved.revision_digest.clone(),
                role: DependencyRole::MethodologicalSource,
            });
        }
        mapped_sources.push(MappedSource {
            original_record_id: link.original_record_id.clone(),
            original_revision_id: link.original_revision_id.clone(),
            fixture_id: link.fixture_id.clone(),
            record_id: saved.record_id.clone(),
            revision_id: saved.revision_id.clone(),
            revision_digest: saved.revision_digest.clone(),
            methodological_dependency: link.methodological_dependency,
        });
    }
    let document = remap_document(&seed.document, &forward)?;
    let roundtrip = remap_document(&document, &reverse)?;
    if roundtrip != seed.document {
        return Err("seed_lens_document_roundtrip_mismatch".into());
    }
    Ok(PreparedLens {
        document_sha256: document_sha256(&document)?,
        roundtrip_document_sha256: document_sha256(&roundtrip)?,
        source_links_sha256: source_links_sha256(&seed.source_links)?,
        document,
        mapped_sources,
        dependencies,
    })
}

pub(super) fn seed(
    state: &AppState,
    loupist_id: &str,
    lenses: &[SeedLens],
    source_ingestion: &Value,
) -> Result<Value> {
    let sources = source_map(source_ingestion)?;
    let prepared = lenses
        .iter()
        .map(|lens| prepare(lens, &sources))
        .collect::<Result<Vec<_>>>()?;
    let store = KnowledgeStore::for_loupist(state.store.clone(), loupist_id)?;
    let at_ms = chrono::Utc::now().timestamp_millis();
    Ok(store.transact(|write| {
        let selection_before = write.load_standing_selection(loupist_id)?;
        let standing_before = selection_before
            .as_ref()
            .map(|(record, _)| write.read_lens(loupist_id, record))
            .transpose()?;
        let mut receipts = Vec::new();
        for (original, prepared) in lenses.iter().zip(&prepared) {
            for source in &prepared.mapped_sources {
                let record = write.owned_record(loupist_id, &source.record_id)?;
                let revision = write.get_revision(loupist_id, &source.revision_id)?;
                if record.kind != RecordKind::Source
                    || record.revision_id.as_deref() != Some(source.revision_id.as_str())
                    || revision.record_id != source.record_id
                    || revision.revision_digest != source.revision_digest
                {
                    return Err(KnowledgeError::Invalid(
                        "seed_lens_source_identity_changed",
                    ));
                }
            }
            let created = write.create_record(CreateRecord {
                loupist_id: loupist_id.into(),
                kind: RecordKind::Lens,
                title: original.title.clone(),
                at_ms,
            })?;
            let saved = write.revise_lens(ReviseLens {
                loupist_id: loupist_id.into(),
                record_id: created.record_id.clone(),
                expected_revision_id: None,
                at_ms,
                document: prepared.document.clone(),
                dependencies: prepared.dependencies.clone(),
                change: LensChangeInput {
                    rationale: "Evaluation import of a learned method with declared Source identity remapping".into(),
                    references: Vec::new(),
                    feedback: None,
                },
            })?;
            let stored = write.read_lens(loupist_id, &saved.record_id)?;
            if stored.document != prepared.document || created.title != original.title {
                return Err(KnowledgeError::Invalid("seed_lens_stored_document_changed"));
            }
            receipts.push(json!({
                "title":original.title,"original_document":original.document,
                "original_document_sha256":original.original_document_sha256,
                "original_source_links":original.source_links,"original_source_links_sha256":prepared.source_links_sha256,
                "remapped_document":prepared.document,"remapped_document_sha256":prepared.document_sha256,
                "roundtrip_document_sha256":prepared.roundtrip_document_sha256,"roundtrip_equal":true,
                "unchanged_bytes_outside_declared_source_ids":true,"mapped_sources":prepared.mapped_sources,
                "created":{"record_id":saved.record_id,"revision_id":saved.revision_id,
                    "revision_digest":saved.revision_digest,"dependencies":saved.dependencies},
                "remap_fields":["markdown","applicability","assumptions","procedure","principles","limitations"]
            }));
        }
        let selection_after = write.load_standing_selection(loupist_id)?;
        let standing_after = selection_after
            .as_ref()
            .map(|(record, _)| write.read_lens(loupist_id, record))
            .transpose()?;
        if selection_before != selection_after || standing_before != standing_after {
            return Err(KnowledgeError::Invalid("seed_lens_standing_lens_changed"));
        }
        Ok(json!({"status":"completed","at_ms":at_ms,"lenses":receipts,
            "standing_selection_before":selection_before,"standing_selection_after":selection_after,
            "standing_lens_unchanged":true,"paid_inference_calls":0,
            "fidelity_scope":"original title, all six document fields, and declared actual MethodologicalSource dependency membership",
            "not_imported":["authoring change history","Chat records"],
            "document_hash_contract":"sha256(LensDocument.delivery_text UTF-8); sorted six-field canonical JSON",
            "source_links_hash_contract":"sha256(sorted-key canonical JSON array); declared array order preserved"}))
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{Authorship, GraphProcessingSettings, LensView};

    const ORIGINAL_RECORD: &str = "rec_11111111111111111111111111111111";
    const ORIGINAL_REVISION: &str = "rev_22222222222222222222222222222222";

    fn source() -> Source {
        Source {
            id: "method-source".into(),
            title: "Method source".into(),
            text: "Record the original scope before comparing observations.\n".into(),
            origin: "https://example.test/method".into(),
            publication_time: None,
            acquired_at: None,
            authorship: Authorship::Publisher,
        }
    }

    fn learned() -> SeedLens {
        let document = LensDocument {
            markdown: format!("# 저장된 방법\r\n[원문](source:{ORIGINAL_RECORD})\n  공백 유지  \n"),
            applicability: format!("범위가 일치할 때.\t{ORIGINAL_REVISION}"),
            assumptions: "입력 0007.50, ≤ 3; 문자열 그대로.".into(),
            procedure: format!("1. {ORIGINAL_RECORD}\n2. {ORIGINAL_REVISION}"),
            principles: "\"관측\"과 판단을 분리한다.".into(),
            limitations: "추가 자료가 없으면 한계를 남긴다.\n".into(),
        };
        SeedLens {
            title: "Exact learned method".into(),
            original_document_sha256: document_sha256(&document).unwrap(),
            document,
            source_links: vec![SourceLink {
                original_record_id: ORIGINAL_RECORD.into(),
                original_revision_id: ORIGINAL_REVISION.into(),
                fixture_id: "method-source".into(),
                methodological_dependency: true,
            }],
        }
    }

    fn rehash(seed: &mut SeedLens) {
        seed.original_document_sha256 = document_sha256(&seed.document).unwrap();
    }

    async fn fixture() -> (tempfile::TempDir, AppState, String, Value, LensView) {
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_eval(temporary.path().join("app"));
        let workspace = temporary.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        let config = GraphProcessingSettings {
            enrichment_enabled: false,
            ..GraphProcessingSettings::default()
        };
        let ingestion = super::super::ingest(&state, &loupist.id, &config, &[source()]).unwrap();
        let store = KnowledgeStore::for_loupist(state.store.clone(), &loupist.id).unwrap();
        let standing = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: loupist.id.clone(),
                    kind: RecordKind::Lens,
                    title: "Existing perspective".into(),
                    at_ms: 10,
                })?;
                let standing = write.revise_lens(ReviseLens {
                    loupist_id: loupist.id.clone(),
                    record_id: record.record_id,
                    expected_revision_id: None,
                    at_ms: 10,
                    document: LensDocument {
                        markdown: "Keep the existing perspective.".into(),
                        applicability: String::new(),
                        assumptions: String::new(),
                        procedure: String::new(),
                        principles: String::new(),
                        limitations: String::new(),
                    },
                    dependencies: Vec::new(),
                    change: LensChangeInput {
                        rationale: "Existing fixture perspective".into(),
                        references: Vec::new(),
                        feedback: None,
                    },
                })?;
                write.select_standing_lens(
                    &loupist.id,
                    &standing.record_id,
                    &standing.revision_id,
                    10,
                )?;
                Ok(standing)
            })
            .unwrap();
        (temporary, state, loupist.id, ingestion, standing)
    }

    #[test]
    fn seed_lenses_hash_the_exact_six_field_delivery() {
        let document = LensDocument {
            markdown: "m".into(),
            applicability: "a".into(),
            assumptions: "s".into(),
            procedure: "p".into(),
            principles: "r".into(),
            limitations: "l".into(),
        };
        let canonical = br#"{"applicability":"a","assumptions":"s","limitations":"l","markdown":"m","principles":"r","procedure":"p"}"#;
        assert_eq!(
            document_sha256(&document).unwrap(),
            crate::util::sha256(canonical)
        );
        let seed = learned();
        let canonical_links = format!(
            "[{{\"fixture_id\":\"method-source\",\"methodological_dependency\":true,\"original_record_id\":\"{ORIGINAL_RECORD}\",\"original_revision_id\":\"{ORIGINAL_REVISION}\"}}]"
        );
        assert_eq!(
            source_links_sha256(&seed.source_links).unwrap(),
            crate::util::sha256(canonical_links.as_bytes())
        );
        let mut mapping_only = seed.clone();
        mapping_only.source_links[0].methodological_dependency = false;
        assert_ne!(
            source_links_sha256(&seed.source_links).unwrap(),
            source_links_sha256(&mapping_only.source_links).unwrap()
        );
    }

    #[test]
    fn seed_lenses_are_optional_and_part_of_serialized_execution_identity() {
        let base = json!({"id":"fixture","title":"Fixture","condition":"clean",
            "world_enabled":false,"seed_sources":[source()],
            "steps":[{"id":"probe","user":"Compare fixtures."}]});
        let empty: super::super::Scenario = serde_json::from_value(base.clone()).unwrap();
        assert!(empty.seed_lenses.is_empty());
        let empty_bytes = serde_json::to_vec(&empty).unwrap();
        let mut seeded = base;
        seeded["seed_lenses"] = json!([learned()]);
        let parsed: super::super::Scenario = serde_json::from_value(seeded).unwrap();
        assert_ne!(
            crate::util::sha256(&empty_bytes),
            crate::util::sha256(&serde_json::to_vec(&parsed).unwrap())
        );
        assert_eq!(
            serde_json::to_value(&parsed).unwrap()["seed_lenses"][0]["original_document_sha256"],
            learned().original_document_sha256
        );
    }

    #[test]
    fn seed_lenses_reject_unverifiable_or_unsupported_references() {
        type Edit = fn(&mut SeedLens);
        let rejected: &[(&str, Edit)] = &[
            ("bad hash", |seed| {
                seed.original_document_sha256 = "0".repeat(64)
            }),
            ("missing fixture", |seed| {
                seed.source_links[0].fixture_id = "later-step-source".into()
            }),
            ("undeclared record", |seed| {
                seed.document.limitations = "rec_33333333333333333333333333333333".into();
                rehash(seed);
            }),
            ("ambiguous link", |seed| {
                let link = seed.source_links[0].clone();
                seed.source_links.push(link);
            }),
            ("reference in title", |seed| {
                seed.title = format!("Method from {ORIGINAL_RECORD}")
            }),
            ("span reference", |seed| {
                seed.document.procedure =
                    "[reference](evidence:spn_33333333333333333333333333333333)".into();
                rehash(seed);
            }),
        ];
        for (name, edit) in rejected {
            let mut seed = learned();
            edit(&mut seed);
            assert!(validate(&[seed], &[source()]).is_err(), "{name}");
        }
        // A fixture cannot ask to change the standing selection or bind a raw Source id.
        for field in ["standingLens", "select"] {
            let mut value = serde_json::to_value(learned()).unwrap();
            value[field] = json!(true);
            assert!(serde_json::from_value::<SeedLens>(value).is_err());
        }
        let mut value = serde_json::to_value(learned()).unwrap();
        value["source_links"][0]["source_id"] = json!("src_33333333333333333333333333333333");
        assert!(serde_json::from_value::<SeedLens>(value).is_err());
    }

    async fn assert_commit_exact_method(has_dependency: bool) {
        let (_temporary, state, loupist, ingestion, standing) = fixture().await;
        let store = KnowledgeStore::for_loupist(state.store.clone(), &loupist).unwrap();
        let saved_source: RetainedSource =
            serde_json::from_value(ingestion[0]["saved"].clone()).unwrap();
        let before_source = serde_json::to_value(
            store
                .get_revision(&loupist, &saved_source.revision_id)
                .unwrap(),
        )
        .unwrap();
        let mut original = learned();
        original.source_links[0].methodological_dependency = has_dependency;
        let receipt = seed(&state, &loupist, &[original.clone()], &ingestion).unwrap();
        assert_eq!(receipt["status"], "completed");
        assert_eq!(receipt["lenses"].as_array().unwrap().len(), 1);
        let imported = &receipt["lenses"][0];
        let view = store
            .read_lens(&loupist, imported["created"]["record_id"].as_str().unwrap())
            .unwrap();
        let mut expected = original.document.clone();
        for text in [
            &mut expected.markdown,
            &mut expected.applicability,
            &mut expected.assumptions,
            &mut expected.procedure,
            &mut expected.principles,
            &mut expected.limitations,
        ] {
            *text = text
                .replace(ORIGINAL_RECORD, &saved_source.record_id)
                .replace(ORIGINAL_REVISION, &saved_source.revision_id);
        }
        assert_eq!(view.document, expected);
        let expected_dependencies = if has_dependency {
            vec![EvidenceRevision {
                record_id: saved_source.record_id.clone(),
                revision_digest: saved_source.revision_digest.clone(),
                role: DependencyRole::MethodologicalSource,
            }]
        } else {
            Vec::new()
        };
        assert_eq!(view.dependencies, expected_dependencies);
        assert_eq!(imported["roundtrip_equal"], true);
        assert_eq!(
            store.read_lens(&loupist, &standing.record_id).unwrap(),
            standing
        );
        assert_eq!(receipt["standing_lens_unchanged"], true);
        assert_eq!(
            serde_json::to_value(
                store
                    .get_revision(&loupist, &saved_source.revision_id)
                    .unwrap()
            )
            .unwrap(),
            before_source
        );
        assert!(store.graph_jobs(&loupist).unwrap().is_empty());
        assert_eq!(receipt["paid_inference_calls"], 0);
        state.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn seed_lenses_commit_exact_methods_with_or_without_source_dependencies() {
        for has_dependency in [true, false] {
            assert_commit_exact_method(has_dependency).await;
        }
    }

    #[tokio::test]
    async fn seed_lenses_roll_back_the_whole_import_when_product_validation_rejects_a_method() {
        let (_temporary, state, loupist, ingestion, standing) = fixture().await;
        let store = KnowledgeStore::for_loupist(state.store.clone(), &loupist).unwrap();
        let before = serde_json::to_value(store.list_records(&loupist).unwrap()).unwrap();
        let mut invalid = learned();
        invalid.title = "Invalid second method".into();
        invalid.document.markdown.clear();
        rehash(&mut invalid);
        assert!(seed(&state, &loupist, &[learned(), invalid], &ingestion).is_err());
        assert_eq!(
            serde_json::to_value(store.list_records(&loupist).unwrap()).unwrap(),
            before
        );
        assert_eq!(
            store.read_lens(&loupist, &standing.record_id).unwrap(),
            standing
        );
        assert_eq!(
            store
                .read(|write| write.load_standing_selection(&loupist))
                .unwrap(),
            Some((standing.record_id, standing.revision_id))
        );
        state.shutdown().await.unwrap();
    }
}
