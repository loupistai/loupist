//! Builds a pack from canonical records. Exclusions are decided from records
//! (privacy of every supporting Source), not by filtering text, and the
//! private-string scan runs over every string that would leave.
use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use super::pack::valid_token as token;
use super::pack::{
    self, ClaimRecord, Counts, EndpointRecord, EntityRecord, Files, Filing, FilingSystem,
    ItemOrigin, License, Manifest, MethodDoc, PackKind, PrincipleDoc, Profile, QualifierRecord,
    SourceRecord,
};
use super::scan::{scan_private, PrivateClass, PrivateContext};
use crate::knowledge::shared_pack::{node_identifiers, ExportSnapshot, SpanSource};
use crate::knowledge::{GraphClaim, GraphNode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Scope {
    Loupist,
    Item(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct Sections {
    pub profile: bool,
    pub perspective: bool,
    pub methods: bool,
    pub principles: bool,
    pub map: bool,
}

impl Sections {
    pub(crate) const ALL: Self = Self {
        profile: true,
        perspective: true,
        methods: true,
        principles: true,
        map: true,
    };
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProfileInfo {
    pub name: String,
    pub focus: String,
    pub tone: String,
    pub avatar: String,
}

/// Everything gathered from the Loupist for one export or summary.
#[derive(Debug, Clone, Default)]
pub(crate) struct ExportInput {
    pub profile: ProfileInfo,
    pub snapshot: ExportSnapshot,
    /// Accepted shared Lens items, by record id: they keep their origin in frontmatter.
    pub lens_origins: BTreeMap<String, ItemOrigin>,
    /// Imported records keyed as `source:<id>`, `node:<id>`, `claim:<id>` or `profile`.
    pub world_origins: BTreeMap<String, ItemOrigin>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub(crate) enum ItemKind {
    Profile,
    Perspective,
    Method,
    Principle,
    Entity,
    Claim,
    Source,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct Finding {
    pub id: String,
    pub class: PrivateClass,
    pub item_key: String,
    pub item_kind: ItemKind,
    pub item_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub symbol: Option<String>,
    pub blocking: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct SummaryCounts {
    pub perspective: u32,
    pub methods: u32,
    pub principles: u32,
    pub names: u32,
    pub links: u32,
    pub sources: u32,
    pub left_out: u32,
    pub left_out_license: u32,
}

/// Every candidate record, keyed, before sections and exclusions apply.
#[derive(Debug, Clone)]
pub(crate) struct Prepared {
    pub profile: Profile,
    pub include_profile: bool,
    pub perspective: Option<MethodDoc>,
    pub methods: Vec<MethodDoc>,
    pub principles: Vec<PrincipleDoc>,
    pub entities: Vec<EntityRecord>,
    pub claims: Vec<ClaimRecord>,
    pub sources: Vec<SourceRecord>,
    /// Relationships every support of which is private.
    pub left_out: u32,
    pub left_out_license: u32,
    /// Licenses of shared material, by item key.
    pub shared_origins: BTreeMap<String, ItemOrigin>,
    /// Lens record id by item key, for item scopes.
    pub record_keys: BTreeMap<String, String>,
}

const RESERVED_ID_PREFIXES: [&str; 10] = [
    "rec_", "rev_", "spn_", "snp_", "jnl_", "src_", "cap_", "rep_", "chg_", "gev_",
];

fn id_at(text: &str, index: usize) -> Option<usize> {
    let prefix = RESERVED_ID_PREFIXES
        .iter()
        .find(|prefix| text[index..].starts_with(*prefix))?;
    let start = index + prefix.len();
    let length = text[start..]
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric())
        .count();
    let boundary = text[..index]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
    (boundary && length >= 16).then_some(start + length)
}

/// Record IDs never leave: Lens links lose their target and keep their label.
pub(crate) fn strip_record_ids(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        let rest = &text[index..];
        if rest.starts_with("](lens:") {
            if let Some(close) = rest.find(')') {
                // Drop the opening bracket already copied, keep the label.
                if let Some(open) = out.rfind('[') {
                    out.remove(open);
                }
                index += close + 1;
                continue;
            }
        }
        for scheme in ["lens:", "world://", "journal://"] {
            if rest.starts_with(scheme) && id_at(text, index + scheme.len()).is_some() {
                index += scheme.len();
                break;
            }
        }
        if let Some(end) = id_at(text, index) {
            index = end;
            continue;
        }
        let character = text[index..].chars().next().expect("character");
        out.push(character);
        index += character.len_utf8();
    }
    out
}

fn clean_document(document: &pack::PackDoc) -> pack::PackDoc {
    pack::PackDoc {
        summary: strip_record_ids(&document.summary),
        sections: document
            .sections
            .iter()
            .map(|section| pack::PackSection {
                key: section.key.clone(),
                heading: strip_record_ids(&section.heading),
                markdown: strip_record_ids(&section.markdown),
            })
            .collect(),
    }
}

/// Provider policy and authorship are independent. A user-owned upload or account
/// capture is not a user-authored statement. Web pages only export reference metadata.
pub(crate) fn source_is_private(source: &SpanSource) -> bool {
    use super::provider_policy::{classify, DataPolicy};
    if source.custom_origin {
        return true;
    }
    let provider = source.provider_identity.to_ascii_lowercase();
    if source.authorship == "user" && source.source_class == "user_statement" && provider == "user"
    {
        return false;
    }
    if source.authorship == "imported_contributor" && provider.starts_with("shared_pack:") {
        return false;
    }
    if source.authorship != "publisher" {
        return true;
    }
    if source.source_class == "web_page" {
        return !(classify(&provider) == DataPolicy::PublicRecord
            || provider == "native-web:web_fetch"
            || provider == "web-research"
            || provider == "web-research:web_fetch"
            || provider == "web_fetch");
    }
    !matches!(
        source.source_class.as_str(),
        "publisher_document" | "historical_document" | "tool_result" | "dataset"
    ) || classify(&provider) != DataPolicy::PublicRecord
}

fn printable(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}

/// At most `max_bytes` of `text` without control characters, cut at a
/// character boundary (line breaks kept).
fn clip(text: &str, max_bytes: usize) -> String {
    let text = printable(text);
    let mut end = text.len().min(max_bytes);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// One line of at most `max_bytes` without control characters.
fn line(text: &str, max_bytes: usize) -> String {
    pack::fit(&printable(text), max_bytes)
}

/// Whether a World relationship can travel in the pack format and load into
/// the receiver's map: two different, known names in a directed role pair.
fn loadable(claim: &GraphClaim, nodes: &BTreeMap<&str, &GraphNode>) -> bool {
    let [first, second] = claim.endpoints.as_slice() else {
        return false;
    };
    pack::ENDPOINT_ROLES
        .iter()
        .any(|roles| first.role == roles[0] && second.role == roles[1])
        && first.node_id != second.node_id
        && claim.endpoints.iter().all(|endpoint| {
            nodes
                .get(endpoint.node_id.as_str())
                .is_some_and(|node| token(&node.kind, 128))
        })
        && token(&claim.relation, 64)
}

fn http_url(locator: &str) -> Option<String> {
    let normalized = reqwest::Url::parse(locator.trim()).ok()?.to_string();
    pack::http_url(&normalized).map(|url| url.to_string())
}

fn filing(url: &reqwest::Url) -> Option<Filing> {
    let host = url.host_str()?.to_ascii_lowercase();
    if host.ends_with("dart.fss.or.kr") {
        let id = url
            .query_pairs()
            .find(|(key, _)| key.eq_ignore_ascii_case("rcpNo") || key == "rcept_no")?
            .1
            .to_string();
        return id.bytes().all(|b| b.is_ascii_digit()).then_some(Filing {
            system: FilingSystem::Dart,
            id,
        });
    }
    if host.ends_with("sec.gov") {
        let parts: Vec<&str> = url.path().split('/').collect();
        let position = parts.iter().position(|part| *part == "data")?;
        let accession = parts.get(position + 2)?;
        return (accession.len() >= 10 && accession.bytes().all(|b| b.is_ascii_digit())).then(
            || Filing {
                system: FilingSystem::Edgar,
                id: (*accession).to_owned(),
            },
        );
    }
    None
}

fn source_record(key: String, source: &SpanSource) -> SourceRecord {
    let url = http_url(&source.origin_locator);
    let parsed = url.as_deref().and_then(|url| reqwest::Url::parse(url).ok());
    let doi = parsed.as_ref().and_then(|url| {
        (url.host_str() == Some("doi.org")).then(|| url.path().trim_start_matches('/').to_owned())
    });
    let publisher = parsed
        .as_ref()
        .and_then(|url| {
            url.host_str()
                .map(|host| host.trim_start_matches("www.").to_owned())
        })
        .unwrap_or_default();
    let title = line(&source.title, 200);
    SourceRecord {
        key,
        title: if title.is_empty() {
            "Source".into()
        } else {
            title
        },
        publisher: line(&publisher, 200),
        filing: parsed.as_ref().and_then(filing),
        doi: doi.map(|doi| line(&doi, 200)).filter(|doi| !doi.is_empty()),
        url,
        published_at: source.publication_time.clone(),
    }
}

fn local_name(alias: &str) -> bool {
    alias
        .chars()
        .any(|c| c as u32 > 0x024F && c.is_alphabetic())
}

/// One name, fitted to the pack's byte bounds (a 200-character Korean name
/// is longer than 200 bytes).
fn entity_record(key: String, node: &GraphNode) -> EntityRecord {
    let name = line(&node.name, 200);
    let mut seen = BTreeSet::new();
    let (local, aliases): (Vec<String>, Vec<String>) = node
        .aliases
        .iter()
        .map(|alias| line(alias, 200))
        .filter(|alias| !alias.is_empty() && *alias != name && seen.insert(alias.clone()))
        .take(16)
        .partition(|alias| local_name(alias));
    let identifiers = node_identifiers(node);
    let fitted = |value: Option<String>| {
        value
            .map(|value| line(&value, 40))
            .filter(|value| !value.is_empty())
    };
    EntityRecord {
        key,
        name: if name.is_empty() {
            "Unnamed".into()
        } else {
            name
        },
        kind: node.kind.clone(),
        aliases,
        local_names: local,
        identifiers: pack::Identifiers {
            ticker: fitted(identifiers.ticker),
            exchange: fitted(identifiers.exchange),
            cik: fitted(identifiers.cik),
            dart_corp_code: fitted(identifiers.dart_corp_code),
            isin: fitted(identifiers.isin),
        },
        summary: clip(&node.description, 4_000),
    }
}

fn claim_record(
    key: String,
    claim: &GraphClaim,
    entity_keys: &BTreeMap<String, String>,
    sources: Vec<String>,
    fallback_attribution: &str,
) -> ClaimRecord {
    let attribution = line(&claim.attribution, 500);
    let proposition = clip(&claim.proposition, 8_000);
    ClaimRecord {
        key,
        relation: claim.relation.clone(),
        endpoints: claim
            .endpoints
            .iter()
            .map(|endpoint| EndpointRecord {
                role: endpoint.role.clone(),
                entity: entity_keys[&endpoint.node_id].clone(),
            })
            .collect(),
        proposition: if proposition.trim().is_empty() {
            fallback_attribution.to_owned()
        } else {
            proposition
        },
        attribution: if attribution.is_empty() {
            fallback_attribution.to_owned()
        } else {
            attribution
        },
        qualifiers: claim
            .qualifiers
            .iter()
            .filter(|qualifier| token(&qualifier.key, 64))
            .map(|qualifier| QualifierRecord {
                key: qualifier.key.clone(),
                value: line(&qualifier.value, 512),
            })
            .filter(|qualifier| !qualifier.value.is_empty())
            .take(16)
            .collect(),
        published_at: claim.publication_time.clone(),
        effective: claim.effective_time.clone(),
        valid_at: claim.valid_at.clone(),
        invalid_at: claim.invalid_at.clone(),
        sources,
        quote: None,
    }
}

/// Keys every candidate and decides which relationships may leave at all.
pub(crate) fn prepare(input: &ExportInput, pack_id: &str) -> Prepared {
    let snapshot = &input.snapshot;
    let mut shared_origins = BTreeMap::new();
    if let Some(origin) = input.world_origins.get("profile") {
        shared_origins.insert("profile".into(), origin.clone());
    }
    let mut record_keys = BTreeMap::new();
    let method = |record: &crate::knowledge::shared_pack::LensRecord, key: String| {
        let origin = input.lens_origins.get(&record.record_id).cloned();
        MethodDoc {
            key,
            title: strip_record_ids(&record.title),
            origin,
            document: clean_document(&record.document),
        }
    };
    let perspective = snapshot.perspective.as_ref().map(|record| {
        record_keys.insert("perspective".to_owned(), record.record_id.clone());
        method(record, "perspective".into())
    });
    let methods: Vec<MethodDoc> = snapshot
        .methods
        .iter()
        .map(|record| {
            let key = pack::item_key("m", pack_id, &record.record_id);
            record_keys.insert(key.clone(), record.record_id.clone());
            method(record, key)
        })
        .collect();
    let principles: Vec<PrincipleDoc> = snapshot
        .principles
        .iter()
        .map(|principle| {
            let key = pack::item_key("p", pack_id, &principle.record_id);
            record_keys.insert(key.clone(), principle.record_id.clone());
            PrincipleDoc {
                key,
                statement: strip_record_ids(&principle.statement),
                why: principle.why.as_deref().map(strip_record_ids),
                example: principle.example.as_deref().map(strip_record_ids),
                origin: input.lens_origins.get(&principle.record_id).cloned(),
            }
        })
        .collect();
    for doc in perspective.iter().chain(&methods) {
        if let Some(origin) = &doc.origin {
            shared_origins.insert(doc.key.clone(), origin.clone());
        }
    }
    for doc in &principles {
        if let Some(origin) = &doc.origin {
            shared_origins.insert(doc.key.clone(), origin.clone());
        }
    }
    let nodes: BTreeMap<&str, &GraphNode> = snapshot
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let mut left_out = 0;
    let mut kept = Vec::new();
    let mut used_sources = BTreeMap::<String, &SpanSource>::new();
    for claim in &snapshot.claims {
        if !loadable(claim, &nodes) {
            continue;
        }
        // A relationship leaves only with at least one public Source behind it.
        let public: Vec<&SpanSource> = claim
            .evidence
            .iter()
            .filter_map(|binding| snapshot.spans.get(&binding.span_id))
            .filter(|source| !source_is_private(source))
            .filter(|source| {
                source.authorship != "imported_contributor"
                    || input
                        .world_origins
                        .contains_key(&format!("source:{}", source.record_id))
            })
            .collect();
        if public.is_empty() {
            left_out += 1;
            continue;
        }
        let mut keys = BTreeSet::new();
        for source in public {
            let key = pack::item_key("s", pack_id, &source.record_id);
            used_sources.entry(key.clone()).or_insert(source);
            keys.insert(key);
        }
        kept.push((claim, keys.into_iter().take(32).collect::<Vec<_>>()));
    }
    let mut entity_keys = BTreeMap::new();
    let mut entities = Vec::new();
    for (claim, _) in &kept {
        for endpoint in &claim.endpoints {
            if entity_keys.contains_key(&endpoint.node_id) {
                continue;
            }
            let key = pack::item_key("e", pack_id, &endpoint.node_id);
            entity_keys.insert(endpoint.node_id.clone(), key.clone());
            if let Some(origin) = input
                .world_origins
                .get(&format!("node:{}", endpoint.node_id))
            {
                shared_origins.insert(key.clone(), origin.clone());
            }
            entities.push(entity_record(key, nodes[endpoint.node_id.as_str()]));
        }
    }
    let mut sources: Vec<SourceRecord> = used_sources
        .into_iter()
        .map(|(key, source)| {
            if let Some(origin) = input
                .world_origins
                .get(&format!("source:{}", source.record_id))
            {
                shared_origins.insert(key.clone(), origin.clone());
            }
            source_record(key, source)
        })
        .collect();
    let titles: BTreeMap<&str, &str> = sources
        .iter()
        .map(|source| (source.key.as_str(), source.title.as_str()))
        .collect();
    let mut claims: Vec<ClaimRecord> = kept
        .into_iter()
        .map(|(claim, keys)| {
            let fallback = titles.get(keys[0].as_str()).copied().unwrap_or("Source");
            let key = pack::item_key("c", pack_id, &claim.id);
            if let Some(origin) = input.world_origins.get(&format!("claim:{}", claim.id)) {
                shared_origins.insert(key.clone(), origin.clone());
            }
            claim_record(key, claim, &entity_keys, keys, fallback)
        })
        .collect();
    entities.sort_by(|a, b| a.key.cmp(&b.key));
    claims.sort_by(|a, b| a.key.cmp(&b.key));
    sources.sort_by(|a, b| a.key.cmp(&b.key));
    Prepared {
        include_profile: true,
        profile: Profile {
            name: input.profile.name.clone(),
            focus: strip_record_ids(&input.profile.focus),
            tone: input.profile.tone.clone(),
            language: language(input),
            avatar: input.profile.avatar.clone(),
            description: String::new(),
        },
        perspective,
        methods,
        principles,
        entities,
        claims,
        sources,
        left_out,
        left_out_license: 0,
        shared_origins,
        record_keys,
    }
}

fn language(input: &ExportInput) -> String {
    let text: String = std::iter::once(input.profile.name.as_str())
        .chain(std::iter::once(input.profile.focus.as_str()))
        .chain(input.snapshot.methods.iter().map(|m| m.title.as_str()))
        .chain(
            input
                .snapshot
                .principles
                .iter()
                .map(|p| p.statement.as_str()),
        )
        .collect();
    let letters = text.chars().filter(|c| c.is_alphabetic()).count();
    let hangul = text
        .chars()
        .filter(|c| ('\u{AC00}'..='\u{D7A3}').contains(c) || ('\u{1100}'..='\u{11FF}').contains(c))
        .count();
    if letters > 0 && hangul * 10 >= letters * 3 {
        "ko".into()
    } else {
        "en".into()
    }
}

fn finding_id(class: PrivateClass, key: &str, symbol: Option<&str>) -> String {
    pack::sha256_hex(format!("{class:?}\u{0}{key}\u{0}{}", symbol.unwrap_or("")).as_bytes())[..16]
        .to_owned()
}

fn label(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() > 80 {
        format!("{}…", line.chars().take(79).collect::<String>())
    } else {
        line
    }
}

/// The item's label as a finding names it: cut before the first word that
/// could carry the matched value (digits, an address, a long token).
fn finding_label(text: &str) -> String {
    let mut words = Vec::new();
    let mut cut = false;
    for word in text.split_whitespace() {
        let risky = word
            .chars()
            .any(|c| c.is_ascii_digit() || matches!(c, '@' | '/' | ':'))
            || word.chars().count() > 24;
        if risky || words.len() == 8 {
            cut = true;
            break;
        }
        words.push(word);
    }
    let mut out = words.join(" ");
    if cut || out.chars().count() > 80 {
        out = out.chars().take(79).collect();
        out.push('…');
    }
    out
}

fn scan_item(
    findings: &mut Vec<Finding>,
    context: &PrivateContext,
    key: &str,
    kind: ItemKind,
    item_label: &str,
    texts: &[&str],
) {
    let mut hits = BTreeSet::new();
    for text in texts {
        hits.extend(scan_private(text, context));
    }
    for hit in hits {
        findings.push(Finding {
            id: finding_id(hit.class, key, hit.symbol.as_deref()),
            class: hit.class,
            item_key: key.to_owned(),
            item_kind: kind,
            item_label: finding_label(item_label),
            symbol: hit.symbol,
            blocking: hit.class.blocking(),
        });
    }
}

fn document_texts(document: &pack::PackDoc) -> Vec<&str> {
    document.texts()
}

/// The private-string scan over every string of every candidate.
pub(crate) fn findings(prepared: &Prepared, context: &PrivateContext) -> Vec<Finding> {
    let mut findings = Vec::new();
    let profile = &prepared.profile;
    scan_item(
        &mut findings,
        context,
        "profile",
        ItemKind::Profile,
        &profile.name,
        &[&profile.name, &profile.focus, &profile.description],
    );
    if let Some(doc) = &prepared.perspective {
        let mut texts = vec![doc.title.as_str()];
        texts.extend(document_texts(&doc.document));
        scan_item(
            &mut findings,
            context,
            &doc.key,
            ItemKind::Perspective,
            &doc.title,
            &texts,
        );
    }
    for doc in &prepared.methods {
        let mut texts = vec![doc.title.as_str()];
        texts.extend(document_texts(&doc.document));
        scan_item(
            &mut findings,
            context,
            &doc.key,
            ItemKind::Method,
            &doc.title,
            &texts,
        );
    }
    for doc in &prepared.principles {
        scan_item(
            &mut findings,
            context,
            &doc.key,
            ItemKind::Principle,
            &doc.statement,
            &[
                &doc.statement,
                doc.why.as_deref().unwrap_or(""),
                doc.example.as_deref().unwrap_or(""),
            ],
        );
    }
    for entity in &prepared.entities {
        let identifiers = &entity.identifiers;
        let mut texts: Vec<&str> = vec![&entity.name, &entity.summary];
        texts.extend(entity.aliases.iter().map(String::as_str));
        texts.extend(entity.local_names.iter().map(String::as_str));
        texts.extend(
            [
                &identifiers.ticker,
                &identifiers.exchange,
                &identifiers.cik,
                &identifiers.dart_corp_code,
                &identifiers.isin,
            ]
            .into_iter()
            .flatten()
            .map(String::as_str),
        );
        scan_item(
            &mut findings,
            context,
            &entity.key,
            ItemKind::Entity,
            &entity.name,
            &texts,
        );
    }
    for claim in &prepared.claims {
        let mut texts: Vec<&str> = vec![&claim.proposition, &claim.attribution];
        texts.extend(claim.qualifiers.iter().map(|q| q.value.as_str()));
        scan_item(
            &mut findings,
            context,
            &claim.key,
            ItemKind::Claim,
            &claim.proposition,
            &texts,
        );
    }
    for source in &prepared.sources {
        let mut texts: Vec<&str> = vec![&source.title, &source.publisher];
        texts.extend(source.url.as_deref());
        texts.extend(source.doi.as_deref());
        scan_item(
            &mut findings,
            context,
            &source.key,
            ItemKind::Source,
            &source.title,
            &texts,
        );
    }
    let labels: BTreeMap<&str, (ItemKind, &str)> = prepared
        .perspective
        .iter()
        .map(|item| {
            (
                item.key.as_str(),
                (ItemKind::Perspective, item.title.as_str()),
            )
        })
        .chain(
            prepared
                .methods
                .iter()
                .map(|item| (item.key.as_str(), (ItemKind::Method, item.title.as_str()))),
        )
        .chain(prepared.principles.iter().map(|item| {
            (
                item.key.as_str(),
                (ItemKind::Principle, item.statement.as_str()),
            )
        }))
        .chain(
            prepared
                .sources
                .iter()
                .map(|item| (item.key.as_str(), (ItemKind::Source, item.title.as_str()))),
        )
        .chain(
            prepared
                .entities
                .iter()
                .map(|item| (item.key.as_str(), (ItemKind::Entity, item.name.as_str()))),
        )
        .chain(prepared.claims.iter().map(|item| {
            (
                item.key.as_str(),
                (ItemKind::Claim, item.proposition.as_str()),
            )
        }))
        .chain(std::iter::once((
            "profile",
            (ItemKind::Profile, prepared.profile.name.as_str()),
        )))
        .collect();
    for (key, origin) in &prepared.shared_origins {
        let entries = origin.entries();
        let texts: Vec<&str> = entries
            .iter()
            .flat_map(|entry| {
                std::iter::once(entry.name.as_str())
                    .chain(entry.author.as_deref())
                    .chain(entry.url.as_deref())
            })
            .collect();
        let (kind, title) = labels
            .get(key.as_str())
            .copied()
            .unwrap_or((ItemKind::Source, origin.name.as_str()));
        scan_item(&mut findings, context, key, kind, title, &texts);
    }
    let mut ids = BTreeSet::new();
    findings.retain(|finding| ids.insert(finding.id.clone()));
    findings
}

/// What the person chose in the share sheet.
#[derive(Debug, Clone)]
pub(crate) struct Choice {
    pub scope: Scope,
    pub sections: Sections,
    pub excluded: BTreeSet<String>,
    pub acknowledged: BTreeSet<String>,
}

/// Applies scope, sections and "Leave it out", cascading: a left-out name
/// takes its relationships, a relationship without a public Source leaves.
pub(crate) fn select(prepared: &Prepared, choice: &Choice) -> Prepared {
    let mut excluded = choice.excluded.clone();
    let blocked: BTreeSet<String> = prepared
        .shared_origins
        .iter()
        .filter(|(_, origin)| !origin.redistributable())
        .map(|(key, _)| key.clone())
        .collect();
    excluded.extend(blocked.iter().cloned());
    let mut selected = prepared.clone();
    let sections = choice.sections;
    selected.include_profile =
        choice.scope == Scope::Loupist && sections.profile && !excluded.contains("profile");
    selected.left_out_license = blocked
        .iter()
        .filter(|key| {
            (sections.profile && key.as_str() == "profile")
                || (sections.perspective
                    && prepared
                        .perspective
                        .as_ref()
                        .is_some_and(|doc| doc.key == **key))
                || (sections.methods && prepared.methods.iter().any(|doc| doc.key == **key))
                || (sections.principles && prepared.principles.iter().any(|doc| doc.key == **key))
                || (sections.map
                    && (prepared.sources.iter().any(|source| source.key == **key)
                        || prepared.entities.iter().any(|entity| entity.key == **key)
                        || prepared.claims.iter().any(|claim| claim.key == **key)))
        })
        .count() as u32;
    if let Scope::Item(record_id) = &choice.scope {
        let key = prepared
            .record_keys
            .iter()
            .find(|(_, id)| *id == record_id)
            .map(|(key, _)| key.clone());
        let keep =
            |item_key: &str| Some(item_key) == key.as_deref() && !excluded.contains(item_key);
        selected.perspective = None;
        selected.methods.retain(|doc| keep(&doc.key));
        selected.principles.retain(|doc| keep(&doc.key));
        selected.left_out = 0;
        selected.left_out_license = u32::from(key.is_some_and(|key| blocked.contains(&key)));
        selected.entities.clear();
        selected.claims.clear();
        selected.sources.clear();
        return selected;
    }
    let sections = choice.sections;
    if !sections.perspective {
        selected.perspective = None;
    }
    selected.perspective = selected
        .perspective
        .filter(|doc| !excluded.contains(&doc.key));
    if !sections.methods {
        selected.methods.clear();
    }
    selected.methods.retain(|doc| !excluded.contains(&doc.key));
    if !sections.principles {
        selected.principles.clear();
    }
    selected
        .principles
        .retain(|doc| !excluded.contains(&doc.key));
    if !sections.map {
        selected.left_out = 0;
        selected.entities.clear();
        selected.claims.clear();
        selected.sources.clear();
        return selected;
    }
    let left_entities: BTreeSet<&str> = selected
        .entities
        .iter()
        .filter(|entity| excluded.contains(&entity.key))
        .map(|entity| entity.key.as_str())
        .collect();
    let mut claims = Vec::new();
    for claim in &prepared.claims {
        if excluded.contains(&claim.key) {
            continue;
        }
        if claim
            .endpoints
            .iter()
            .any(|endpoint| left_entities.contains(endpoint.entity.as_str()))
        {
            if claim
                .endpoints
                .iter()
                .any(|endpoint| blocked.contains(&endpoint.entity))
            {
                selected.left_out_license += 1;
            }
            continue;
        }
        let blocked_supports = claim.sources.iter().all(|key| blocked.contains(key));
        let mut claim = claim.clone();
        let had_sources = !claim.sources.is_empty();
        claim.sources.retain(|source| !excluded.contains(source));
        if had_sources && claim.sources.is_empty() {
            if blocked_supports {
                selected.left_out_license += 1;
            }
            continue;
        }
        claims.push(claim);
    }
    let used_entities: BTreeSet<&str> = claims
        .iter()
        .flat_map(|claim| claim.endpoints.iter().map(|e| e.entity.as_str()))
        .collect();
    let used_sources: BTreeSet<&str> = claims
        .iter()
        .flat_map(|claim| claim.sources.iter().map(String::as_str))
        .collect();
    selected.entities = prepared
        .entities
        .iter()
        .filter(|entity| used_entities.contains(entity.key.as_str()))
        .cloned()
        .collect();
    selected.sources = prepared
        .sources
        .iter()
        .filter(|source| used_sources.contains(source.key.as_str()))
        .cloned()
        .collect();
    selected.claims = claims;
    selected
}

/// Labels for selected records whose encoded form exceeds a pack item limit.
pub(crate) fn oversized_items(selected: &Prepared) -> Vec<String> {
    let mut items = Vec::new();
    let mut note = |key: &str, title: &str| items.push(format!("{} ({key})", pack::fit(title, 80)));
    if selected.include_profile
        && pack::render_profile(&selected.profile).len() > pack::MAX_MARKDOWN
    {
        note("profile", &selected.profile.name);
    }
    if let Some(item) = &selected.perspective {
        if pack::render_method(item).len() > pack::MAX_MARKDOWN {
            note(&item.key, &item.title);
        }
    }
    for (index, item) in selected.methods.iter().enumerate() {
        if index >= pack::MAX_METHODS || pack::render_method(item).len() > pack::MAX_MARKDOWN {
            note(&item.key, &item.title);
        }
    }
    for (index, item) in selected.principles.iter().enumerate() {
        if index >= pack::MAX_PRINCIPLES || pack::render_principle(item).len() > pack::MAX_MARKDOWN
        {
            note(&item.key, &item.statement);
        }
    }
    for item in &selected.entities {
        if serde_json::to_vec(item).expect("entity serializes").len() > pack::MAX_JSONL_LINE {
            note(&item.key, &item.name);
        }
    }
    for item in &selected.claims {
        if serde_json::to_vec(item).expect("claim serializes").len() > pack::MAX_JSONL_LINE {
            note(&item.key, &item.proposition);
        }
    }
    for item in &selected.sources {
        if serde_json::to_vec(item).expect("source serializes").len() > pack::MAX_JSONL_LINE {
            note(&item.key, &item.title);
        }
    }
    items
}

pub(crate) fn counts(prepared: &Prepared) -> SummaryCounts {
    SummaryCounts {
        perspective: u32::from(prepared.perspective.is_some()),
        methods: prepared.methods.len() as u32,
        principles: prepared.principles.len() as u32,
        names: prepared.entities.len() as u32,
        links: prepared.claims.len() as u32,
        sources: prepared.sources.len() as u32,
        left_out: prepared.left_out,
        left_out_license: prepared.left_out_license,
    }
}

fn included_keys(selected: &Prepared, profile: bool) -> BTreeSet<String> {
    let mut keys: BTreeSet<String> = selected
        .perspective
        .iter()
        .chain(&selected.methods)
        .map(|doc| doc.key.clone())
        .chain(selected.principles.iter().map(|doc| doc.key.clone()))
        .chain(selected.entities.iter().map(|e| e.key.clone()))
        .chain(selected.claims.iter().map(|c| c.key.clone()))
        .chain(selected.sources.iter().map(|s| s.key.clone()))
        .collect();
    if profile && selected.include_profile {
        keys.insert("profile".into());
    }
    keys
}

/// Blocking findings and unacknowledged warnings left in what would be exported.
pub(crate) fn unresolved<'a>(
    findings: &'a [Finding],
    selected: &Prepared,
    choice: &Choice,
) -> Vec<&'a Finding> {
    let profile = choice.scope == Scope::Loupist && choice.sections.profile;
    let included = included_keys(selected, profile);
    findings
        .iter()
        .filter(|finding| included.contains(&finding.item_key))
        .filter(|finding| finding.blocking || !choice.acknowledged.contains(&finding.id))
        .collect()
}

/// One pure compatibility function supplies both the summary and export gate.
pub(crate) fn license_options(selected: &Prepared) -> Result<Vec<License>, pack::LicenseConflict> {
    pack::compatible_licenses(&licensed_items(selected))
}

pub(crate) fn licensed_items(selected: &Prepared) -> Vec<pack::LicensedItem> {
    let included = included_keys(selected, selected.include_profile);
    selected
        .shared_origins
        .iter()
        .filter(|(key, _)| included.contains(*key))
        .flat_map(|(key, origin)| {
            let name = selected
                .perspective
                .iter()
                .chain(&selected.methods)
                .find(|item| item.key == *key)
                .map(|item| item.title.clone())
                .or_else(|| {
                    selected
                        .principles
                        .iter()
                        .find(|item| item.key == *key)
                        .map(|item| label(&item.statement))
                })
                .or_else(|| {
                    selected
                        .sources
                        .iter()
                        .find(|item| item.key == *key)
                        .map(|item| item.title.clone())
                })
                .or_else(|| {
                    selected
                        .entities
                        .iter()
                        .find(|item| item.key == *key)
                        .map(|item| item.name.clone())
                })
                .or_else(|| {
                    selected
                        .claims
                        .iter()
                        .find(|item| item.key == *key)
                        .map(|item| item.proposition.clone())
                })
                .or_else(|| (key == "profile").then(|| selected.profile.name.clone()))
                .unwrap_or_else(|| origin.name.clone());
            origin.entries().into_iter().filter_map(move |entry| {
                entry.license.map(|license| pack::LicensedItem {
                    key: key.clone(),
                    name: name.clone(),
                    license,
                })
            })
        })
        .collect()
}

fn credits(selected: &Prepared) -> Vec<pack::OriginEntry> {
    let included = included_keys(selected, selected.include_profile);
    let mut credits = BTreeMap::new();
    for (_, origin) in selected
        .shared_origins
        .iter()
        .filter(|(key, _)| included.contains(*key))
    {
        for entry in origin.entries() {
            if entry
                .license
                .is_some_and(|license| license != License::Cc0 && license.redistributable())
            {
                credits.insert((entry.id.clone(), entry.version, entry.license), entry);
            }
        }
    }
    credits.into_values().collect()
}

fn asset_classes(selected: &Prepared) -> Vec<String> {
    let mut classes = BTreeSet::new();
    for entity in &selected.entities {
        let kind = entity.kind.to_ascii_lowercase();
        let text = format!("{} {}", entity.name, entity.summary).to_ascii_lowercase();
        if kind == "organization" && entity.identifiers.ticker.is_some() {
            classes.insert("stocks");
        }
        if text.contains(" etf") || text.contains("fund") {
            classes.insert("etfs");
        }
        if text.contains("bond") || text.contains("treasury") {
            classes.insert("bonds");
        }
        if text.contains("bitcoin") || text.contains("crypto") || text.contains("ethereum") {
            classes.insert("crypto");
        }
        if matches!(kind.as_str(), "policy" | "metric") && text.contains("rate") {
            classes.insert("economy");
        }
    }
    pack::ASSET_CLASSES
        .iter()
        .filter(|class| classes.contains(*class))
        .map(|class| (*class).to_owned())
        .collect()
}

fn unique_path(paths: &mut BTreeSet<String>, prefix: &str, slug: &str) -> String {
    let mut candidate = format!("{prefix}{slug}.md");
    let mut n = 2;
    while !paths.insert(candidate.clone()) {
        candidate = format!("{prefix}{slug}-{n}.md");
        n += 1;
    }
    candidate
}

/// Identity a pack carries; version and time come from the share identity.
#[derive(Debug, Clone)]
pub(crate) struct PackMeta {
    pub pack_id: String,
    pub version: u32,
    pub created_at: String,
    pub license: License,
    pub terms_url: Option<String>,
    pub byline: Option<String>,
    pub notice: String,
    pub open_url: Option<String>,
    pub app: String,
    /// The shared Loupist this one was started from, then its own ancestors (W19.5).
    pub origin: Vec<pack::OriginEntry>,
}

/// The pack files for a selection. The README and manifest are generated last.
pub(crate) fn build(selected: &Prepared, choice: &Choice, meta: &PackMeta) -> (Manifest, Files) {
    let mut files = Files::new();
    let mut paths = BTreeSet::new();
    let line = |value: &str| {
        let mut text = value.to_owned();
        text.push('\n');
        text
    };
    let kind = match &choice.scope {
        Scope::Loupist => PackKind::Loupist,
        Scope::Item(_) if selected.principles.len() == 1 => PackKind::Principle,
        Scope::Item(_) => PackKind::Method,
    };
    // With Profile off the Loupist's name stays home: the manifest, README and
    // folder use a neutral title.
    let neutral = || super::neutral_title(&selected.profile.language).to_owned();
    let name = match kind {
        PackKind::Loupist if selected.include_profile => selected.profile.name.clone(),
        PackKind::Loupist => neutral(),
        PackKind::Method => selected
            .methods
            .first()
            .map(|doc| doc.title.clone())
            .unwrap_or_else(neutral),
        PackKind::Principle => selected
            .principles
            .first()
            .map(|doc| label(&doc.statement))
            .unwrap_or_else(neutral),
    };
    match kind {
        PackKind::Loupist => {
            if selected.include_profile {
                files.insert(
                    "LOUPIST.md".into(),
                    pack::render_profile(&selected.profile).into_bytes(),
                );
            }
            files.insert(
                "LICENSE".into(),
                pack::render_license(
                    meta.license,
                    meta.byline.as_deref(),
                    meta.terms_url.as_deref(),
                )
                .into_bytes(),
            );
            if let Some(doc) = &selected.perspective {
                files.insert(
                    "lens/perspective.md".into(),
                    pack::render_method(doc).into_bytes(),
                );
            }
            for doc in &selected.methods {
                let path = unique_path(
                    &mut paths,
                    "lens/methods/",
                    &pack::slug(&doc.title, &doc.key),
                );
                files.insert(path, pack::render_method(doc).into_bytes());
            }
            for doc in &selected.principles {
                let words: String = doc
                    .statement
                    .split_whitespace()
                    .take(8)
                    .collect::<Vec<_>>()
                    .join(" ");
                let path = unique_path(
                    &mut paths,
                    "lens/principles/",
                    &pack::slug(&words, &doc.key),
                );
                files.insert(path, pack::render_principle(doc).into_bytes());
            }
            if !selected.claims.is_empty() {
                let jsonl = |lines: Vec<String>| lines.concat().into_bytes();
                files.insert(
                    "world/entities.jsonl".into(),
                    jsonl(
                        selected
                            .entities
                            .iter()
                            .map(|r| line(&serde_json::to_string(r).expect("entity")))
                            .collect(),
                    ),
                );
                files.insert(
                    "world/claims.jsonl".into(),
                    jsonl(
                        selected
                            .claims
                            .iter()
                            .map(|r| line(&serde_json::to_string(r).expect("claim")))
                            .collect(),
                    ),
                );
                files.insert(
                    "world/sources.jsonl".into(),
                    jsonl(
                        selected
                            .sources
                            .iter()
                            .map(|r| line(&serde_json::to_string(r).expect("source")))
                            .collect(),
                    ),
                );
            }
        }
        PackKind::Method => {
            if let Some(doc) = selected.methods.first() {
                files.insert(
                    format!("method-{}.md", pack::slug(&doc.title, &doc.key)),
                    pack::render_method(doc).into_bytes(),
                );
            }
        }
        PackKind::Principle => {
            if let Some(doc) = selected.principles.first() {
                let words: String = doc
                    .statement
                    .split_whitespace()
                    .take(8)
                    .collect::<Vec<_>>()
                    .join(" ");
                files.insert(
                    format!("principle-{}.md", pack::slug(&words, &doc.key)),
                    pack::render_principle(doc).into_bytes(),
                );
            }
        }
    }
    files.insert(
        "README.md".into(),
        pack::render_readme(
            &name,
            &meta.notice,
            meta.license,
            meta.open_url.as_deref(),
            meta.terms_url.as_deref(),
            &credits(selected),
        )
        .into_bytes(),
    );
    let world = kind == PackKind::Loupist && !selected.claims.is_empty();
    let manifest = Manifest {
        format: pack::FORMAT.into(),
        id: meta.pack_id.clone(),
        kind,
        name: pack::fit(&name, pack::MAX_NAME),
        version: meta.version,
        created_at: meta.created_at.clone(),
        language: selected.profile.language.clone(),
        license: meta.license,
        terms_url: meta.terms_url.clone(),
        credits: credits(selected),
        author: meta
            .byline
            .as_ref()
            .map(|byline| pack::fit(byline, 80))
            .filter(|byline| !byline.is_empty())
            .map(|display| pack::Author { display }),
        origin: if kind == PackKind::Loupist {
            meta.origin.iter().take(pack::MAX_ORIGIN).cloned().collect()
        } else {
            Vec::new()
        },
        asset_classes: if world {
            asset_classes(selected)
        } else {
            Vec::new()
        },
        counts: Counts {
            perspective: u32::from(kind == PackKind::Loupist && selected.perspective.is_some()),
            methods: match kind {
                PackKind::Principle => 0,
                _ => selected.methods.len().min(if kind == PackKind::Method {
                    1
                } else {
                    usize::MAX
                }) as u32,
            },
            principles: match kind {
                PackKind::Method => 0,
                _ => selected
                    .principles
                    .len()
                    .min(if kind == PackKind::Principle {
                        1
                    } else {
                        usize::MAX
                    }) as u32,
            },
            entities: if world {
                selected.entities.len() as u32
            } else {
                0
            },
            claims: if world {
                selected.claims.len() as u32
            } else {
                0
            },
            sources: if world {
                selected.sources.len() as u32
            } else {
                0
            },
        },
        files: BTreeMap::new(),
        app: meta.app.clone(),
    };
    pack::seal(manifest, files)
}

/// The version-independent hash of what a choice would export.
pub(crate) fn content_hash_for(selected: &Prepared, choice: &Choice, meta: &PackMeta) -> String {
    let (_, files) = build(selected, choice, meta);
    pack::content_hash(&files)
}

#[cfg(test)]
mod url_tests {
    use super::*;
    #[test]
    fn stored_source_urls_are_normalized_before_strict_validation() {
        assert_eq!(
            http_url(" \nhttps://example.org/a b\t"),
            Some("https://example.org/a%20b".into())
        );
        assert!(http_url(" ftp://example.org/file ").is_none());
        assert!(http_url(" https://user:secret@example.org/file ").is_none());
    }
}
