//! Seeds that name each other: retained Sources by fixture id, Lens pages
//! whose sections cite those Sources and the World claims their scripts state.
use super::*;
use crate::knowledge::lens_page::{
    Basis, BasisRef, FilingOrigin, LensOp, PageDelta, PageKind, PageTarget, SectionKey,
};
use std::collections::BTreeSet;

/// A retained Source the scenario names by its fixture id.
pub(super) type Retained = BTreeMap<String, RetainedSource>;

/// Retain one seeded Source at `at_ms`.
pub(super) fn retain(
    write: &KnowledgeWrite<'_>,
    loupist_id: &str,
    source: &SourceSeed,
    at_ms: i64,
) -> KnowledgeResult<RetainedSource> {
    // The Source is its text, read and cited as the user would see it. A
    // stored Source has no field for company, form or language, so those
    // seed keys stay fixture metadata.
    let (media_type, bytes) = ("text/plain".to_owned(), source.text.as_bytes().to_vec());
    let publication_time = source
        .publication_time
        .as_deref()
        .map(world::time_point)
        .transpose()
        .map_err(|_| KnowledgeError::Invalid("seed publication_time is invalid"))?;
    write.retain_new_source(RetainNewSource {
        origin: None,
        loupist_id: loupist_id.into(),
        at_ms,
        title: source.title.clone(),
        source_class: SourceClass::PublisherDocument,
        origin_locator: source.origin.clone(),
        provider_identity: "eval-fixture".into(),
        authorship: Authorship::Publisher,
        coverage: Coverage::IncludedFull,
        origin_chat_id: None,
        request_ref: None,
        declared_digest: None,
        publication_time,
        material: Some(RetainMaterial {
            media_type,
            role: RepresentationRole::Original,
            document_id: source
                .id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            parser_version: None,
            parent_representation_id: None,
            input_span_ids: vec![],
            selector: Selector::ByteRange {
                start: 0,
                end: bytes.len() as u64,
            },
            bytes,
            context_note: "Fictional eval source".into(),
        }),
    })
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LensPageSeed {
    title: String,
    #[serde(default)]
    summary: String,
    kind: PageKind,
    #[serde(default)]
    sections: Vec<LensPageSection>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LensPageSection {
    key: String,
    heading: String,
    markdown: String,
    #[serde(default)]
    refs: Vec<SectionRef>,
}

/// A section's evidence: a seeded Source's span and, optionally, the World
/// claim a scripted fact of that Source became.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SectionRef {
    source: String,
    #[serde(default)]
    fact: Option<String>,
}

impl LensPageSeed {
    /// The Sources and scripted facts the page's sections cite.
    pub(super) fn refs(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.sections
            .iter()
            .flat_map(|section| &section.refs)
            .map(|reference| (reference.source.as_str(), reference.fact.as_deref()))
    }
}

/// File the scenario's research pages after the seed's World publications, so
/// a section can cite a claim a later publication ends.
pub(super) fn lens_pages(
    state: &AppState,
    loupist_id: &str,
    pages: &[LensPageSeed],
    retained: &Retained,
    world: Option<&world::script::Staging>,
) -> Result<Vec<Value>> {
    let store = KnowledgeStore::for_loupist(state.store.clone(), loupist_id)?;
    let mut filed = Vec::new();
    for page in pages {
        let mut ops = Vec::new();
        if !page.summary.trim().is_empty() {
            ops.push(LensOp::Summary {
                text: page.summary.clone(),
            });
        }
        for section in &page.sections {
            let mut refs = Vec::new();
            for reference in &section.refs {
                let source = retained.get(&reference.source).ok_or_else(|| {
                    format!(
                        "lens_pages ref names no seeded Source: {}",
                        reference.source
                    )
                })?;
                if let Some(fact) = &reference.fact {
                    let claim = world.and_then(|world| world.claim(fact)).ok_or_else(|| {
                        format!("lens_pages ref names a fact no seed publication staged: {fact}")
                    })?;
                    refs.push(BasisRef::Claim {
                        claim_id: claim.to_owned(),
                    });
                }
                refs.push(BasisRef::Span {
                    record_id: source.record_id.clone(),
                    revision_digest: source.revision_digest.clone(),
                    span_id: source
                        .span_id
                        .clone()
                        .ok_or("a seeded Source has no span")?,
                });
            }
            ops.push(LensOp::Put {
                key: SectionKey::new(&section.key)?,
                heading: section.heading.clone(),
                markdown: section.markdown.clone(),
                basis: Basis {
                    refs,
                    uncertainty: None,
                },
                expected: None,
            });
        }
        let receipt = store.transact(|write| {
            write.file(
                loupist_id,
                &FilingOrigin::User {
                    chat_id: None,
                    message_id: None,
                },
                vec![PageDelta {
                    target: PageTarget::Create {
                        kind: page.kind,
                        title: page.title.clone(),
                        anchors: Vec::new(),
                    },
                    ops,
                }],
                crate::commands::now_ms(),
            )
        })?;
        let [page] = receipt.as_slice() else {
            return Err("a lens_pages seed must file exactly one page".into());
        };
        if !page.created {
            return Err(format!("lens_pages seed was not created: {}", page.title).into());
        }
        filed.push(json!({"title": page.title, "record_id": page.record_id}));
    }
    Ok(filed)
}

/// A followed asset and the World company it is, named by a seed publication.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AssetLink {
    /// A followed asset's name, as `watchlist` gives it.
    asset: String,
    /// The World entity a scripted publication at seed names.
    company: String,
}

impl AssetLink {
    pub(super) fn company(&self) -> &str {
        &self.company
    }
}

/// Link each followed asset to its World company on its asset page, as the
/// product's own match does, as soon as a publication has introduced that
/// company. `linked` keeps the links already made.
pub(super) fn asset_links(
    state: &AppState,
    loupist_id: &str,
    links: &[AssetLink],
    linked: &mut BTreeSet<usize>,
) -> Result<()> {
    if links.len() == linked.len() {
        return Ok(());
    }
    let store = KnowledgeStore::for_loupist(state.store.clone(), loupist_id)?;
    let Some(world) = world::script::head(&store, loupist_id)? else {
        return Ok(());
    };
    let rows = state.store.assets_list(loupist_id, false)?;
    for (index, link) in links.iter().enumerate() {
        if linked.contains(&index) {
            continue;
        }
        let Some(node) = world.nodes.iter().find(|node| node.name == link.company) else {
            continue;
        };
        let matches = crate::store::user_context::matching(&rows, &link.asset);
        let [asset] = matches.as_slice() else {
            return Err(format!("an asset link names one followed asset: {}", link.asset).into());
        };
        crate::commands::assets::with_assets(state, loupist_id, true, |tx| {
            let page = crate::knowledge::assets::ensure_asset_page(
                tx,
                loupist_id,
                asset,
                crate::commands::now_ms(),
            )?;
            crate::knowledge::assets::set_entity(tx, loupist_id, &page.asset_id, Some(&node.id))
        })?;
        linked.insert(index);
    }
    Ok(())
}
