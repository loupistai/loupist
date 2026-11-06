//! Import exact learned method pages into disposable evaluation Loupists. Only
//! declared Source record/revision identities may change; normal Lens
//! authority still applies.

use super::{AppState, Result, Source};
use crate::knowledge::lens_page::{
    Basis, FilingOrigin, LensOp, PageDelta, PageKind, PageTarget, SectionKey,
};
use crate::knowledge::{KnowledgeError, KnowledgeStore, RecordKind, RetainedSource};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct SeedSection {
    key: String,
    heading: String,
    markdown: String,
}

/// A method page as a scenario declares it: summary plus keyed sections.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct SeedDoc {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    sections: Vec<SeedSection>,
}

impl SeedDoc {
    fn remap(&self, mapping: &BTreeMap<String, String>) -> Result<Self> {
        Ok(Self {
            summary: remap_text(&self.summary, mapping)?,
            sections: self
                .sections
                .iter()
                .map(|section| {
                    Ok(SeedSection {
                        key: section.key.clone(),
                        heading: remap_text(&section.heading, mapping)?,
                        markdown: remap_text(&section.markdown, mapping)?,
                    })
                })
                .collect::<Result<Vec<_>>>()?,
        })
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SeedLens {
    title: String,
    document: SeedDoc,
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
}

#[derive(Serialize)]
struct MappedSource {
    original_record_id: String,
    original_revision_id: String,
    fixture_id: String,
    record_id: String,
    revision_id: String,
    revision_digest: String,
}

struct PreparedLens {
    document: SeedDoc,
    document_sha256: String,
    roundtrip_document_sha256: String,
    source_links_sha256: String,
    mapped_sources: Vec<MappedSource>,
}

/// sha256 over the sorted-key canonical JSON of the declared page.
fn document_sha256(document: &SeedDoc) -> Result<String> {
    Ok(crate::util::sha256(
        crate::knowledge::canonicalize_json_text(&serde_json::to_string(document)?)?.as_bytes(),
    ))
}

// The sidecar hash uses sorted object keys and preserves the declared array order.
fn source_links_sha256(links: &[SourceLink]) -> Result<String> {
    let rows: Vec<_> = links
        .iter()
        .map(|link| {
            BTreeMap::from([
                ("fixture_id", json!(link.fixture_id)),
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
            | "lop"
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
    seed.document.remap(&identities)?;
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
        mapped_sources.push(MappedSource {
            original_record_id: link.original_record_id.clone(),
            original_revision_id: link.original_revision_id.clone(),
            fixture_id: link.fixture_id.clone(),
            record_id: saved.record_id.clone(),
            revision_id: saved.revision_id.clone(),
            revision_digest: saved.revision_digest.clone(),
        });
    }
    let document = seed.document.remap(&forward)?;
    let roundtrip = document.remap(&reverse)?;
    if roundtrip != seed.document {
        return Err("seed_lens_document_roundtrip_mismatch".into());
    }
    Ok(PreparedLens {
        document_sha256: document_sha256(&document)?,
        roundtrip_document_sha256: document_sha256(&roundtrip)?,
        source_links_sha256: source_links_sha256(&seed.source_links)?,
        document,
        mapped_sources,
    })
}

fn page_ops(document: &SeedDoc) -> crate::knowledge::KnowledgeResult<Vec<LensOp>> {
    let mut ops = Vec::new();
    if !document.summary.trim().is_empty() {
        ops.push(LensOp::Summary {
            text: document.summary.clone(),
        });
    }
    for section in &document.sections {
        ops.push(LensOp::Put {
            key: SectionKey::new(&section.key)?,
            heading: section.heading.clone(),
            markdown: section.markdown.clone(),
            basis: Basis::default(),
            expected: None,
        });
    }
    Ok(ops)
}

fn stored_doc(page: &crate::knowledge::PageRead) -> SeedDoc {
    SeedDoc {
        summary: page.body.summary().to_owned(),
        sections: page
            .body
            .sections()
            .iter()
            .map(|section| SeedSection {
                key: section.key.as_str().to_owned(),
                heading: section.heading.clone(),
                markdown: section.markdown.clone(),
            })
            .collect(),
    }
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
        let perspective_before = write
            .active_perspective(loupist_id)?
            .map(|record| write.read_page(loupist_id, &record))
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
            let filed = write.file(
                loupist_id,
                &FilingOrigin::Import {
                    pack: json!({"kind": "eval_seed"}),
                },
                vec![PageDelta {
                    target: PageTarget::Create {
                        kind: PageKind::Method,
                        title: original.title.clone(),
                        anchors: Vec::new(),
                    },
                    ops: page_ops(&prepared.document)?,
                }],
                at_ms,
            )?;
            let page = filed.into_iter().next().ok_or(KnowledgeError::NotFound)?;
            if !page.created || !page.outcomes.iter().all(|op| op.outcome.changed()) {
                return Err(KnowledgeError::Invalid("seed_lens_stored_document_changed"));
            }
            let stored = write.read_page(loupist_id, &page.record_id)?;
            if stored_doc(&stored) != prepared.document || stored.title != original.title {
                return Err(KnowledgeError::Invalid("seed_lens_stored_document_changed"));
            }
            receipts.push(json!({
                "title":original.title,"original_document":original.document,
                "original_document_sha256":original.original_document_sha256,
                "original_source_links":original.source_links,"original_source_links_sha256":prepared.source_links_sha256,
                "remapped_document":prepared.document,"remapped_document_sha256":prepared.document_sha256,
                "roundtrip_document_sha256":prepared.roundtrip_document_sha256,"roundtrip_equal":true,
                "unchanged_bytes_outside_declared_source_ids":true,"mapped_sources":prepared.mapped_sources,
                "created":{"record_id":stored.record_id,"revision_id":stored.revision_id,
                    "revision_digest":stored.revision_digest},
                "remap_fields":["summary","sections[].heading","sections[].markdown"]
            }));
        }
        let perspective_after = write
            .active_perspective(loupist_id)?
            .map(|record| write.read_page(loupist_id, &record))
            .transpose()?;
        if perspective_before != perspective_after {
            return Err(KnowledgeError::Invalid("seed_lens_perspective_changed"));
        }
        Ok(json!({"status":"completed","at_ms":at_ms,"lenses":receipts,
            "perspective_unchanged":true,"paid_inference_calls":0,
            "fidelity_scope":"original title, summary and every section's key, heading and markdown",
            "not_imported":["authoring change history","Chat records","section basis"],
            "document_hash_contract":"sha256(sorted-key canonical JSON of {summary, sections[]})",
            "source_links_hash_contract":"sha256(sorted-key canonical JSON array); declared array order preserved"}))
    })?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{Authorship, GraphProcessingSettings, PageRead};

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

    fn section(key: &str, markdown: String) -> SeedSection {
        SeedSection {
            key: key.into(),
            heading: key.to_uppercase(),
            markdown,
        }
    }

    fn learned() -> SeedLens {
        let document = SeedDoc {
            summary: "저장된 방법".into(),
            sections: vec![
                section(
                    "use-when",
                    format!(
                        "범위가 일치할 때.\t{ORIGINAL_REVISION} [원문](source:{ORIGINAL_RECORD})"
                    ),
                ),
                section(
                    "steps",
                    format!("1. {ORIGINAL_RECORD}\n2. {ORIGINAL_REVISION}\n  공백 유지  "),
                ),
                section("limits", "추가 자료가 없으면 한계를 남긴다.".into()),
            ],
        };
        SeedLens {
            title: "Exact learned method".into(),
            original_document_sha256: document_sha256(&document).unwrap(),
            document,
            source_links: vec![SourceLink {
                original_record_id: ORIGINAL_RECORD.into(),
                original_revision_id: ORIGINAL_REVISION.into(),
                fixture_id: "method-source".into(),
            }],
        }
    }

    fn rehash(seed: &mut SeedLens) {
        seed.original_document_sha256 = document_sha256(&seed.document).unwrap();
    }

    async fn fixture() -> (tempfile::TempDir, AppState, String, Value, PageRead) {
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
        let perspective = store
            .transact(|write| {
                let filed = write.file(
                    &loupist.id,
                    &FilingOrigin::User {
                        chat_id: None,
                        message_id: None,
                    },
                    vec![PageDelta {
                        target: PageTarget::Create {
                            kind: PageKind::Perspective,
                            title: "Existing perspective".into(),
                            anchors: Vec::new(),
                        },
                        ops: vec![LensOp::Put {
                            key: SectionKey::new("beliefs").unwrap(),
                            heading: "Beliefs".into(),
                            markdown: "Keep the existing perspective.".into(),
                            basis: Basis::default(),
                            expected: None,
                        }],
                    }],
                    10,
                )?;
                write.read_page(&loupist.id, &filed[0].record_id)
            })
            .unwrap();
        (temporary, state, loupist.id, ingestion, perspective)
    }

    #[test]
    fn seed_lenses_hash_the_declared_page() {
        let document = SeedDoc {
            summary: "m".into(),
            sections: vec![section("steps", "p".into())],
        };
        let canonical =
            br#"{"sections":[{"heading":"STEPS","key":"steps","markdown":"p"}],"summary":"m"}"#;
        assert_eq!(
            document_sha256(&document).unwrap(),
            crate::util::sha256(canonical)
        );
        let seed = learned();
        let canonical_links = format!(
            "[{{\"fixture_id\":\"method-source\",\"original_record_id\":\"{ORIGINAL_RECORD}\",\"original_revision_id\":\"{ORIGINAL_REVISION}\"}}]"
        );
        assert_eq!(
            source_links_sha256(&seed.source_links).unwrap(),
            crate::util::sha256(canonical_links.as_bytes())
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
                seed.document.sections[2].markdown = "rec_33333333333333333333333333333333".into();
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
                seed.document.sections[1].markdown =
                    "[reference](evidence:spn_33333333333333333333333333333333)".into();
                rehash(seed);
            }),
        ];
        for (name, edit) in rejected {
            let mut seed = learned();
            edit(&mut seed);
            assert!(validate(&[seed], &[source()]).is_err(), "{name}");
        }
        // A fixture cannot ask to change the perspective or bind a raw Source id.
        for field in ["perspective", "select"] {
            let mut value = serde_json::to_value(learned()).unwrap();
            value[field] = json!(true);
            assert!(serde_json::from_value::<SeedLens>(value).is_err());
        }
        let mut value = serde_json::to_value(learned()).unwrap();
        value["source_links"][0]["source_id"] = json!("src_33333333333333333333333333333333");
        assert!(serde_json::from_value::<SeedLens>(value).is_err());
    }

    #[tokio::test]
    async fn seed_lenses_commit_exact_method_pages_with_remapped_source_ids() {
        let (_temporary, state, loupist, ingestion, perspective) = fixture().await;
        let store = KnowledgeStore::for_loupist(state.store.clone(), &loupist).unwrap();
        let saved_source: RetainedSource =
            serde_json::from_value(ingestion[0]["saved"].clone()).unwrap();
        let before_source = serde_json::to_value(
            store
                .get_revision(&loupist, &saved_source.revision_id)
                .unwrap(),
        )
        .unwrap();
        let original = learned();
        let receipt = seed(&state, &loupist, &[original.clone()], &ingestion).unwrap();
        assert_eq!(receipt["status"], "completed");
        assert_eq!(receipt["lenses"].as_array().unwrap().len(), 1);
        let imported = &receipt["lenses"][0];
        let page = store
            .read(|read| {
                read.read_page(&loupist, imported["created"]["record_id"].as_str().unwrap())
            })
            .unwrap();
        let mut expected = original.document.clone();
        for section in &mut expected.sections {
            section.markdown = section
                .markdown
                .replace(ORIGINAL_RECORD, &saved_source.record_id)
                .replace(ORIGINAL_REVISION, &saved_source.revision_id);
        }
        assert_eq!(stored_doc(&page), expected);
        assert_eq!(page.kind, crate::knowledge::lens_page::LensKind::Method);
        assert_eq!(page.history[0].origin, "import");
        assert_eq!(imported["roundtrip_equal"], true);
        assert_eq!(
            store
                .read(|read| read.read_page(&loupist, &perspective.record_id))
                .unwrap(),
            perspective
        );
        assert_eq!(receipt["perspective_unchanged"], true);
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
    async fn seed_lenses_roll_back_the_whole_import_when_product_validation_rejects_a_method() {
        let (_temporary, state, loupist, ingestion, perspective) = fixture().await;
        let store = KnowledgeStore::for_loupist(state.store.clone(), &loupist).unwrap();
        let before = serde_json::to_value(store.list_records(&loupist).unwrap()).unwrap();
        let mut invalid = learned();
        invalid.title = "Invalid second method".into();
        invalid.document.sections[0].markdown.clear();
        rehash(&mut invalid);
        assert!(seed(&state, &loupist, &[learned(), invalid], &ingestion).is_err());
        assert_eq!(
            serde_json::to_value(store.list_records(&loupist).unwrap()).unwrap(),
            before
        );
        assert_eq!(
            store
                .read(|read| read.read_page(&loupist, &perspective.record_id))
                .unwrap(),
            perspective
        );
        state.shutdown().await.unwrap();
    }
}
