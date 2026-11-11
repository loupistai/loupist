//! The `loupist-pack/1` format: manifest, records, Markdown items, keys,
//! hashing and the structural validator. The importer trusts nothing outside
//! the manifest, and every rejection is typed.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::knowledge::TimePoint;

pub(crate) const FORMAT: &str = "loupist-pack/1";
pub(crate) const MANIFEST: &str = "loupist-pack.json";
/// The Skill importer's bounds, shared by exporter and importer.
pub(crate) const MAX_FILES: usize = 512;
pub(crate) const MAX_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_DEPTH: usize = 8;
pub(crate) const MAX_MARKDOWN: usize = 64 * 1024;
pub(crate) const MAX_JSONL_LINE: usize = 8 * 1024;
pub(crate) const MAX_PREVIEW: usize = 1024 * 1024;
pub(crate) const MAX_METHODS: usize = 200;
pub(crate) const MAX_PRINCIPLES: usize = 100;
pub(crate) const MAX_ENTITIES: usize = 10_000;
pub(crate) const MAX_CLAIMS: usize = 8_000;
pub(crate) const MAX_SOURCES: usize = 5_000;
pub(crate) const MAX_ORIGIN: usize = 8;
pub(crate) const MAX_QUOTE: usize = 300;
/// Names, in bytes: a 60-character Loupist name in any script fits.
pub(crate) const MAX_NAME: usize = 240;
/// A profile focus, in characters: the same cap as creating a Loupist.
pub(crate) const MAX_FOCUS: usize = 280;
pub(crate) const ASSET_CLASSES: [&str; 5] = ["stocks", "etfs", "bonds", "crypto", "economy"];

pub(crate) type Files = BTreeMap<String, Vec<u8>>;

/// Why a pack cannot be added. The renderer maps each to its own sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) enum Rejection {
    NotPack,
    Newer,
    TooLarge,
    Unsafe,
    Damaged,
    Spoof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub(crate) enum PackKind {
    Loupist,
    Method,
    Principle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) enum License {
    #[serde(rename = "CC-BY-4.0")]
    CcBy,
    #[serde(rename = "CC-BY-SA-4.0")]
    CcBySa,
    #[serde(rename = "CC0-1.0")]
    Cc0,
    #[serde(rename = "CC-BY-NC-4.0")]
    CcByNc,
    #[default]
    #[serde(rename = "CC-BY-NC-SA-4.0")]
    CcByNcSa,
    #[serde(rename = "LicenseRef-Loupist-Personal")]
    Personal,
}

impl License {
    pub(crate) const OPEN: [Self; 5] = [
        Self::CcByNcSa,
        Self::CcBy,
        Self::CcBySa,
        Self::Cc0,
        Self::CcByNc,
    ];
    pub(crate) fn id(self) -> &'static str {
        match self {
            Self::CcBy => "CC-BY-4.0",
            Self::CcBySa => "CC-BY-SA-4.0",
            Self::Cc0 => "CC0-1.0",
            Self::CcByNc => "CC-BY-NC-4.0",
            Self::CcByNcSa => "CC-BY-NC-SA-4.0",
            Self::Personal => "LicenseRef-Loupist-Personal",
        }
    }
    pub(crate) fn from_id(id: &str) -> Option<Self> {
        Self::OPEN
            .into_iter()
            .chain([Self::Personal])
            .find(|license| license.id() == id)
    }
    /// Openly published pack license; NC is not an OSI/open-definition license.
    pub(crate) fn open(self) -> bool {
        self != Self::Personal
    }
    pub(crate) fn redistributable(self) -> bool {
        self.open()
    }
    pub(crate) fn url(self, terms_url: Option<&str>) -> &str {
        match self {
            Self::CcBy => "https://creativecommons.org/licenses/by/4.0/legalcode",
            Self::CcBySa => "https://creativecommons.org/licenses/by-sa/4.0/legalcode",
            Self::Cc0 => "https://creativecommons.org/publicdomain/zero/1.0/legalcode",
            Self::CcByNc => "https://creativecommons.org/licenses/by-nc/4.0/legalcode",
            Self::CcByNcSa => "https://creativecommons.org/licenses/by-nc-sa/4.0/legalcode",
            Self::Personal => terms_url.unwrap_or(""),
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::CcBy => "Creative Commons Attribution 4.0 International (CC BY 4.0)",
            Self::CcBySa => "Creative Commons Attribution-ShareAlike 4.0 International (CC BY-SA 4.0)",
            Self::Cc0 => "CC0 1.0 Universal (public domain dedication)",
            Self::CcByNc => "Creative Commons Attribution-NonCommercial 4.0 International (CC BY-NC 4.0)",
            Self::CcByNcSa => "Creative Commons Attribution-NonCommercial-ShareAlike 4.0 International (CC BY-NC-SA 4.0)",
            Self::Personal => "Loupist Personal (see terms)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct LicensedItem {
    pub key: String,
    pub name: String,
    pub license: License,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct LicenseConflict {
    pub code: &'static str,
    pub items: Vec<LicensedItem>,
}

/// Intersection over every included item's rights. Personal items never participate:
/// the exporter excludes them first. Original user material is represented as CC0.
pub(crate) fn compatible_licenses(items: &[LicensedItem]) -> Result<Vec<License>, LicenseConflict> {
    let allowed: Vec<_> = License::OPEN
        .into_iter()
        .filter(|target| {
            items.iter().all(|item| match item.license {
                License::Cc0 | License::CcBy | License::Personal => true,
                License::CcBySa => *target == License::CcBySa,
                License::CcByNc => matches!(target, License::CcByNc | License::CcByNcSa),
                License::CcByNcSa => *target == License::CcByNcSa,
            })
        })
        .collect();
    if allowed.is_empty() {
        Err(LicenseConflict {
            code: "license_conflict",
            items: items
                .iter()
                .filter(|item| {
                    matches!(
                        item.license,
                        License::CcBySa | License::CcByNc | License::CcByNcSa
                    )
                })
                .cloned()
                .collect(),
        })
    } else {
        Ok(allowed)
    }
}

pub(crate) fn valid_terms_url(url: &str) -> bool {
    valid_url(url) && url.starts_with("https://")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Author {
    pub display: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OriginEntry {
    pub id: String,
    pub name: String,
    pub version: u32,
    pub url: Option<String>,
    pub commit: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub license: Option<License>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Counts {
    pub perspective: u32,
    pub methods: u32,
    pub principles: u32,
    pub entities: u32,
    pub claims: u32,
    pub sources: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileEntry {
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Manifest {
    pub format: String,
    pub id: String,
    pub kind: PackKind,
    pub name: String,
    pub version: u32,
    pub created_at: String,
    pub language: String,
    pub license: License,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terms_url: Option<String>,
    #[serde(default)]
    pub credits: Vec<OriginEntry>,
    pub author: Option<Author>,
    pub origin: Vec<OriginEntry>,
    pub asset_classes: Vec<String>,
    pub counts: Counts,
    pub files: BTreeMap<String, FileEntry>,
    pub app: String,
}
const MANIFEST_FIELDS: [&str; 16] = [
    "format",
    "id",
    "kind",
    "name",
    "version",
    "createdAt",
    "language",
    "license",
    "termsUrl",
    "credits",
    "author",
    "origin",
    "assetClasses",
    "counts",
    "files",
    "app",
];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Identifiers {
    pub ticker: Option<String>,
    pub exchange: Option<String>,
    pub cik: Option<String>,
    pub dart_corp_code: Option<String>,
    pub isin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct EntityRecord {
    pub key: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub aliases: Vec<String>,
    pub local_names: Vec<String>,
    pub identifiers: Identifiers,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EndpointRecord {
    pub role: String,
    pub entity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QualifierRecord {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ClaimRecord {
    pub key: String,
    pub relation: String,
    pub endpoints: Vec<EndpointRecord>,
    pub proposition: String,
    pub attribution: String,
    pub qualifiers: Vec<QualifierRecord>,
    pub published_at: Option<TimePoint>,
    pub effective: Option<TimePoint>,
    pub valid_at: Option<TimePoint>,
    pub invalid_at: Option<TimePoint>,
    pub sources: Vec<String>,
    pub quote: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum FilingSystem {
    Edgar,
    Dart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Filing {
    pub system: FilingSystem,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SourceRecord {
    pub key: String,
    pub title: String,
    pub publisher: String,
    pub url: Option<String>,
    pub filing: Option<Filing>,
    pub doi: Option<String>,
    pub published_at: Option<TimePoint>,
}

/// `origin` in a shared item's frontmatter: who shared it first, under which license.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ItemOrigin {
    pub pack_id: String,
    pub name: String,
    pub author: Option<String>,
    pub license: License,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub chain: Vec<OriginEntry>,
}

impl ItemOrigin {
    pub(crate) fn entries(&self) -> Vec<OriginEntry> {
        std::iter::once(OriginEntry {
            id: self.pack_id.clone(),
            name: self.name.clone(),
            version: self.version,
            url: self.url.clone(),
            commit: None,
            author: self.author.clone(),
            license: Some(self.license),
        })
        .chain(self.chain.clone())
        .collect()
    }
    pub(crate) fn redistributable(&self) -> bool {
        self.license.redistributable()
            && self
                .chain
                .iter()
                .all(|entry| entry.license.is_none_or(License::redistributable))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Profile {
    pub name: String,
    pub focus: String,
    pub tone: String,
    pub language: String,
    pub avatar: String,
    #[serde(skip)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MethodFront {
    key: String,
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin: Option<ItemOrigin>,
    /// Section keys and headings in page order; the body carries their text.
    #[serde(default)]
    sections: Vec<SectionFront>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SectionFront {
    key: String,
    heading: String,
}

/// One keyed section of a shared page. Basis never travels: a pack carries
/// text, and an accepted section starts with an empty basis and shared origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PackSection {
    pub key: String,
    pub heading: String,
    pub markdown: String,
}

/// A method or perspective page as a pack carries it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PackDoc {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub sections: Vec<PackSection>,
}

impl PackDoc {
    pub(crate) fn texts(&self) -> Vec<&str> {
        std::iter::once(self.summary.as_str())
            .chain(
                self.sections
                    .iter()
                    .flat_map(|section| [section.heading.as_str(), section.markdown.as_str()]),
            )
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PrincipleFront {
    key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin: Option<ItemOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MethodDoc {
    pub key: String,
    pub title: String,
    pub origin: Option<ItemOrigin>,
    pub document: PackDoc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PrincipleDoc {
    pub key: String,
    pub statement: String,
    pub why: Option<String>,
    pub example: Option<String>,
    pub origin: Option<ItemOrigin>,
}

/// A validated pack: every byte was listed and hashed by the manifest.
#[derive(Debug, Clone)]
pub(crate) struct ParsedPack {
    pub manifest: Manifest,
    pub profile: Option<Profile>,
    pub perspective: Option<MethodDoc>,
    pub methods: Vec<MethodDoc>,
    pub principles: Vec<PrincipleDoc>,
    pub entities: Vec<EntityRecord>,
    pub claims: Vec<ClaimRecord>,
    pub sources: Vec<SourceRecord>,
    pub extension: Option<serde_json::Value>,
}

/// One line of at most `max_bytes`, cut at a character boundary.
pub(crate) fn fit(text: &str, max_bytes: usize) -> String {
    let mut out = String::new();
    for character in text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
    {
        if out.len() + character.len_utf8() > max_bytes {
            break;
        }
        out.push(character);
    }
    out
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let inner_key: Vec<u8> = block.iter().map(|byte| byte ^ 0x36).collect();
    let outer_key: Vec<u8> = block.iter().map(|byte| byte ^ 0x5c).collect();
    let inner = Sha256::new()
        .chain_update(&inner_key)
        .chain_update(message)
        .finalize();
    Sha256::new()
        .chain_update(&outer_key)
        .chain_update(&inner[..])
        .finalize()
        .to_vec()
}

/// A pack-scoped key: versions diff by key without exposing record IDs.
pub(crate) fn item_key(prefix: &str, pack_id: &str, record_id: &str) -> String {
    let mac = hmac_sha256(pack_id.as_bytes(), record_id.as_bytes());
    format!("{prefix}-{}", &hex::encode(mac)[..16])
}

fn valid_key(key: &str, prefix: char) -> bool {
    let mut chars = key.chars();
    chars.next() == Some(prefix)
        && chars.next() == Some('-')
        && key.len() == 18
        && key[2..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// File-name slug: ASCII letters and digits joined by single dashes, at most 48.
pub(crate) fn slug(text: &str, fallback: &str) -> String {
    let mut out = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            out.push(character.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= 48 {
            break;
        }
    }
    let out = out.trim_matches('-').to_owned();
    if out.is_empty() {
        fallback.to_owned()
    } else {
        out
    }
}

fn valid_slug(slug: &str) -> bool {
    (1..=64).contains(&slug.len())
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !slug.starts_with('-')
}

pub(crate) fn file_entry(bytes: &[u8]) -> FileEntry {
    FileEntry {
        sha256: sha256_hex(bytes),
        bytes: bytes.len() as u64,
    }
}

/// The version-independent identity of the content: generated files and the
/// manifest are left out, so an unchanged Loupist keeps its version.
pub(crate) fn content_hash(files: &Files) -> String {
    let mut hasher = Sha256::new();
    for (path, bytes) in files {
        if path == MANIFEST || path == "README.md" {
            continue;
        }
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(sha256_hex(bytes).as_bytes());
        hasher.update([0]);
    }
    if let Some(bytes) = files.get(MANIFEST) {
        if let Ok(manifest) = serde_json::from_slice::<Manifest>(bytes) {
            hasher.update(
                serde_json::to_vec(&(
                    manifest.license,
                    manifest.terms_url,
                    manifest.author,
                    manifest.credits,
                ))
                .expect("license metadata"),
            );
        }
    }
    hex::encode(hasher.finalize())
}

fn is_heading_line(line: &str) -> bool {
    line.trim_start_matches('\\').starts_with("## ")
}

/// One more backslash before any line that looks like a section heading, so a
/// field can never close itself; [`unescape`] reverses it exactly.
fn escape(text: &str) -> String {
    text.split('\n')
        .map(|line| {
            if is_heading_line(line) {
                format!("\\{line}")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn unescape(text: &str) -> String {
    text.split('\n')
        .map(|line| {
            if line.starts_with('\\') && is_heading_line(line) {
                line[1..].to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn frontmatter<T: Serialize>(front: &T) -> String {
    serde_yaml_ng::to_string(front).expect("frontmatter serializes")
}

/// Every string and key of a decoded JSON or YAML value: escapes such as
/// `\u003c` only become the characters they spell after decoding.
fn decoded_spoof(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(text) => super::scan::imitates_marker(text),
        serde_json::Value::Array(items) => items.iter().any(decoded_spoof),
        serde_json::Value::Object(map) => map
            .iter()
            .any(|(key, value)| super::scan::imitates_marker(key) || decoded_spoof(value)),
        _ => false,
    }
}

/// Frontmatter decoded once, checked for marker spoofs in its decoded strings,
/// then read into its typed shape.
fn front<T: serde::de::DeserializeOwned>(yaml: &str) -> Result<T, Rejection> {
    let value: serde_json::Value = serde_yaml_ng::from_str(yaml).map_err(|_| Rejection::Damaged)?;
    if decoded_spoof(&value) {
        return Err(Rejection::Spoof);
    }
    serde_json::from_value(value).map_err(|_| Rejection::Damaged)
}

fn split_frontmatter(text: &str) -> Result<(&str, &str), Rejection> {
    let rest = text.strip_prefix("---\n").ok_or(Rejection::Damaged)?;
    let end = rest.find("\n---\n").ok_or(Rejection::Damaged)?;
    let body = &rest[end + 5..];
    Ok((&rest[..end + 1], body))
}

fn sections(body: &str, headings: &[&str]) -> Result<Vec<String>, Rejection> {
    let mut rest = body
        .strip_prefix('\n')
        .and_then(|body| body.strip_suffix('\n'))
        .ok_or(Rejection::Damaged)?;
    let mut fields = Vec::new();
    for heading in headings {
        let separator = format!("\n\n## {heading}\n\n");
        let (field, after) = rest.split_once(&separator).ok_or(Rejection::Damaged)?;
        fields.push(unescape(field));
        rest = after;
    }
    fields.push(unescape(rest));
    Ok(fields)
}

pub(crate) fn render_method(method: &MethodDoc) -> String {
    let document = &method.document;
    let mut text = format!(
        "---\n{}---\n\n{}",
        frontmatter(&MethodFront {
            key: method.key.clone(),
            title: method.title.clone(),
            origin: method.origin.clone(),
            sections: document
                .sections
                .iter()
                .map(|section| SectionFront {
                    key: section.key.clone(),
                    heading: section.heading.clone(),
                })
                .collect(),
        }),
        escape(&document.summary),
    );
    for section in &document.sections {
        text.push_str(&format!(
            "\n\n## {}\n\n{}",
            section.heading,
            escape(&section.markdown)
        ));
    }
    text.push('\n');
    text
}

pub(crate) fn parse_method(text: &str) -> Result<MethodDoc, Rejection> {
    let (yaml, body) = split_frontmatter(text)?;
    let front: MethodFront = front(yaml)?;
    let headings: Vec<&str> = front
        .sections
        .iter()
        .map(|section| section.heading.as_str())
        .collect();
    let mut fields = sections(body, &headings)?.into_iter();
    let summary = fields.next().ok_or(Rejection::Damaged)?;
    let sections = front
        .sections
        .iter()
        .zip(fields)
        .map(|(section, markdown)| PackSection {
            key: section.key.clone(),
            heading: section.heading.clone(),
            markdown,
        })
        .collect();
    Ok(MethodDoc {
        key: front.key,
        title: front.title,
        origin: front.origin,
        document: PackDoc { summary, sections },
    })
}

pub(crate) fn render_principle(principle: &PrincipleDoc) -> String {
    format!(
        "---\n{}---\n\n{}\n\n## Why\n\n{}\n\n## Example\n\n{}\n",
        frontmatter(&PrincipleFront {
            key: principle.key.clone(),
            origin: principle.origin.clone(),
        }),
        escape(&principle.statement),
        escape(principle.why.as_deref().unwrap_or("")),
        escape(principle.example.as_deref().unwrap_or("")),
    )
}

pub(crate) fn parse_principle(text: &str) -> Result<PrincipleDoc, Rejection> {
    let (yaml, body) = split_frontmatter(text)?;
    let front: PrincipleFront = front(yaml)?;
    let fields = sections(body, &["Why", "Example"])?;
    let optional = |text: &String| (!text.is_empty()).then(|| text.clone());
    Ok(PrincipleDoc {
        key: front.key,
        statement: fields[0].clone(),
        why: optional(&fields[1]),
        example: optional(&fields[2]),
        origin: front.origin,
    })
}

pub(crate) fn render_profile(profile: &Profile) -> String {
    format!(
        "---\n{}---\n\n{}\n",
        frontmatter(profile),
        escape(&profile.description)
    )
}

pub(crate) fn parse_profile(text: &str) -> Result<Profile, Rejection> {
    let (yaml, body) = split_frontmatter(text)?;
    let mut profile: Profile = front(yaml)?;
    profile.description = unescape(
        body.strip_prefix('\n')
            .and_then(|body| body.strip_suffix('\n'))
            .ok_or(Rejection::Damaged)?,
    );
    Ok(profile)
}

pub(crate) fn render_readme(
    name: &str,
    notice: &str,
    license: License,
    open_url: Option<&str>,
    terms_url: Option<&str>,
    credits: &[OriginEntry],
) -> String {
    let mut readme = format!("# {name}\n\n{notice}\n");
    if let Some(url) = open_url {
        let encoded: String = url
            .bytes()
            .map(|byte| match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (byte as char).to_string()
                }
                _ => format!("%{byte:02X}"),
            })
            .collect();
        readme.push_str(&format!(
            "\n[Open in Loupist](loupist://import?url={encoded})\n"
        ));
    }
    readme.push_str(&format!(
        "\nThis pack's content is shared under {} ({}). Loupist's code is licensed separately under AGPL-3.0-only.\n",
        license.title(),
        license.url(terms_url)
    ));
    if matches!(license, License::CcByNc | License::CcByNcSa) {
        readme.push_str("\nNon-commercial use only: use must not be primarily intended for commercial advantage or monetary compensation.\n");
    }
    if !credits.is_empty() {
        readme.push_str("\n## Credits\n\nIncluded material may have been selected, edited or adapted. Its original licenses and attribution remain applicable.\n");
        for credit in credits {
            readme.push_str(&format!(
                "\n- {} — {}; {}; {}; version {}.\n",
                fit(&credit.name, MAX_NAME),
                credit.author.as_deref().unwrap_or("Author not supplied"),
                credit
                    .license
                    .map(License::id)
                    .unwrap_or("License not supplied"),
                credit.url.as_deref().unwrap_or("Link not supplied"),
                credit.version
            ));
        }
    }
    readme
}

pub(crate) fn render_license(
    license: License,
    author: Option<&str>,
    terms_url: Option<&str>,
) -> String {
    format!(
        "{}\n\nCopyright holder: {}\n\n{}\n",
        license.title(),
        author.unwrap_or("the sharer of this Loupist pack"),
        license.url(terms_url)
    )
}

/// A path the format allows, by role. Anything else rejects the pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathRole {
    Manifest,
    Profile,
    Readme,
    License,
    Preview,
    Perspective,
    Method,
    Principle,
    FlatMethod,
    FlatPrinciple,
    Schema,
    Entities,
    Claims,
    Sources,
}

fn path_role(path: &str) -> Option<PathRole> {
    let fixed = match path {
        MANIFEST => Some(PathRole::Manifest),
        "LOUPIST.md" => Some(PathRole::Profile),
        "README.md" => Some(PathRole::Readme),
        "LICENSE" => Some(PathRole::License),
        "preview.png" => Some(PathRole::Preview),
        "lens/perspective.md" => Some(PathRole::Perspective),
        "world/schema.json" => Some(PathRole::Schema),
        "world/entities.jsonl" => Some(PathRole::Entities),
        "world/claims.jsonl" => Some(PathRole::Claims),
        "world/sources.jsonl" => Some(PathRole::Sources),
        _ => None,
    };
    if fixed.is_some() {
        return fixed;
    }
    let markdown = |prefix: &str| {
        path.strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(".md"))
            .filter(|slug| valid_slug(slug))
            .is_some()
    };
    if markdown("lens/methods/") {
        Some(PathRole::Method)
    } else if markdown("lens/principles/") {
        Some(PathRole::Principle)
    } else if markdown("method-") {
        Some(PathRole::FlatMethod)
    } else if markdown("principle-") {
        Some(PathRole::FlatPrinciple)
    } else {
        None
    }
}

/// Byte-bounded text without NUL or control characters other than newline and tab.
/// Bounds are bytes so a valid pack always fits the World and Lens limits.
fn bounded_text(value: &str, max_bytes: usize, empty_ok: bool) -> bool {
    (empty_ok || !value.trim().is_empty())
        && value.len() <= max_bytes
        && !value
            .chars()
            .any(|c| c == '\0' || (c.is_control() && !matches!(c, '\n' | '\t')))
}

fn single_line(value: &str, max_bytes: usize, empty_ok: bool) -> bool {
    bounded_text(value, max_bytes, empty_ok) && !value.contains(['\n', '\t'])
}

fn bounded_chars(value: &str, max_chars: usize, empty_ok: bool) -> bool {
    bounded_text(value, max_chars * 4, empty_ok) && value.chars().count() <= max_chars
}

fn valid_token(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':' | b'.'))
}

fn valid_url(value: &str) -> bool {
    value.len() <= 2_000
        && reqwest::Url::parse(value).is_ok_and(|url| {
            matches!(url.scheme(), "https" | "http")
                && url.username().is_empty()
                && url.password().is_none()
                && url.host_str().is_some()
        })
}

pub(crate) fn manifest_from(bytes: &[u8]) -> Result<Manifest, Rejection> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| Rejection::NotPack)?;
    let object = value.as_object().ok_or(Rejection::NotPack)?;
    let format = object
        .get("format")
        .and_then(serde_json::Value::as_str)
        .ok_or(Rejection::NotPack)?;
    let version = format
        .strip_prefix("loupist-pack/")
        .and_then(|version| version.parse::<u32>().ok())
        .ok_or(Rejection::NotPack)?;
    if version > 1 {
        return Err(Rejection::Newer);
    }
    if version != 1 {
        return Err(Rejection::NotPack);
    }
    // Fields beyond the format (connections, tools, permissions, settings)
    // are an attempt to carry authority, not damage.
    if object
        .keys()
        .any(|key| !MANIFEST_FIELDS.contains(&key.as_str()))
    {
        return Err(Rejection::Unsafe);
    }
    if decoded_spoof(&value) {
        return Err(Rejection::Spoof);
    }
    serde_json::from_value(value).map_err(|_| Rejection::Damaged)
}

pub(crate) fn validate_manifest(manifest: &Manifest) -> Result<(), Rejection> {
    let ok = uuid::Uuid::parse_str(&manifest.id)
        .is_ok_and(|id| id.hyphenated().to_string() == manifest.id)
        && single_line(&manifest.name, MAX_NAME, false)
        && manifest.version >= 1
        && single_line(&manifest.created_at, 40, false)
        && chrono::DateTime::parse_from_rfc3339(&manifest.created_at).is_ok()
        && (2..=16).contains(&manifest.language.len())
        && manifest
            .language
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        && manifest
            .author
            .as_ref()
            .is_none_or(|author| single_line(&author.display, 80, false))
        && (manifest.license != License::Personal
            || manifest.terms_url.as_deref().is_some_and(valid_terms_url))
        && manifest.terms_url.as_deref().is_none_or(valid_terms_url)
        && manifest.credits.len() <= MAX_SOURCES
        && manifest.credits.iter().all(valid_origin_entry)
        && manifest.origin.len() <= MAX_ORIGIN
        && manifest.origin.iter().all(|origin| {
            uuid::Uuid::parse_str(&origin.id).is_ok()
                && single_line(&origin.name, MAX_NAME, false)
                && origin.url.as_deref().is_none_or(valid_url)
                && origin
                    .commit
                    .as_deref()
                    .is_none_or(|commit| valid_token(commit, 64))
        })
        && manifest.asset_classes.len() <= ASSET_CLASSES.len()
        && manifest
            .asset_classes
            .iter()
            .all(|class| ASSET_CLASSES.contains(&class.as_str()))
        && single_line(&manifest.app, 40, false);
    if ok {
        Ok(())
    } else {
        Err(Rejection::Damaged)
    }
}

fn text_of(bytes: &[u8]) -> Result<&str, Rejection> {
    let text = std::str::from_utf8(bytes).map_err(|_| Rejection::Damaged)?;
    if super::scan::imitates_marker(text) {
        return Err(Rejection::Spoof);
    }
    Ok(text)
}

/// JSON text decoded first, so an escaped marker is found in its decoded strings.
fn decoded_json(text: &str) -> Result<serde_json::Value, Rejection> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|_| Rejection::Damaged)?;
    if decoded_spoof(&value) {
        return Err(Rejection::Spoof);
    }
    Ok(value)
}

fn jsonl<T: serde::de::DeserializeOwned>(bytes: &[u8], cap: usize) -> Result<Vec<T>, Rejection> {
    let text = text_of(bytes)?;
    let mut records = Vec::new();
    for line in text.split('\n') {
        if line.is_empty() {
            continue;
        }
        if line.len() > MAX_JSONL_LINE {
            return Err(Rejection::TooLarge);
        }
        records.push(serde_json::from_value(decoded_json(line)?).map_err(|_| Rejection::Damaged)?);
        if records.len() > cap {
            return Err(Rejection::TooLarge);
        }
    }
    Ok(records)
}

/// A page document under the Lens writer's own rules, so every page a
/// Loupist can hold exports and imports.
fn validate_lens_doc(document: &PackDoc) -> bool {
    use crate::knowledge::lens_page;
    let mut keys = std::collections::BTreeSet::new();
    document.sections.len() <= lens_page::MAX_SECTIONS
        && lens_page::summary_ok(&document.summary)
        && (!document.summary.trim().is_empty() || !document.sections.is_empty())
        && document.sections.iter().all(|section| {
            lens_page::SectionKey::new(&section.key).is_ok()
                && keys.insert(section.key.clone())
                && lens_page::heading_ok(&section.heading)
                && lens_page::markdown_ok(&section.markdown)
        })
}

fn valid_origin_entry(origin: &OriginEntry) -> bool {
    uuid::Uuid::parse_str(&origin.id).is_ok()
        && single_line(&origin.name, MAX_NAME, false)
        && origin.url.as_deref().is_none_or(valid_url)
        && origin
            .author
            .as_deref()
            .is_none_or(|author| single_line(author, 80, false))
}

fn validate_origin(origin: &Option<ItemOrigin>) -> bool {
    origin.as_ref().is_none_or(|origin| {
        origin.chain.len() <= MAX_SOURCES
            && origin.chain.iter().all(valid_origin_entry)
            && origin.url.as_deref().is_none_or(valid_url)
            && uuid::Uuid::parse_str(&origin.pack_id).is_ok()
            && single_line(&origin.name, MAX_NAME, false)
            && origin
                .author
                .as_deref()
                .is_none_or(|author| single_line(author, 80, false))
    })
}

fn validate_entity(entity: &EntityRecord) -> bool {
    let identifiers = &entity.identifiers;
    valid_key(&entity.key, 'e')
        && single_line(&entity.name, 200, false)
        && valid_token(&entity.kind, 128)
        && entity.aliases.len() + entity.local_names.len() <= 16
        && entity
            .aliases
            .iter()
            .chain(&entity.local_names)
            .all(|alias| single_line(alias, 200, false))
        && [
            &identifiers.ticker,
            &identifiers.exchange,
            &identifiers.cik,
            &identifiers.dart_corp_code,
            &identifiers.isin,
        ]
        .into_iter()
        .flatten()
        .all(|value| single_line(value, 40, false))
        && bounded_text(&entity.summary, 4_000, true)
}

/// The endpoint roles the World graph accepts, in order.
pub(crate) const ENDPOINT_ROLES: [[&str; 2]; 2] = [["subject", "object"], ["source", "target"]];

/// A relationship is exactly two different names in a directed role pair,
/// backed by at least one of the pack's Sources, so the map can load it.
fn validate_claim(
    claim: &ClaimRecord,
    entities: &BTreeSet<&str>,
    sources: &BTreeSet<&str>,
) -> bool {
    valid_key(&claim.key, 'c')
        && valid_token(&claim.relation, 64)
        && claim.endpoints.len() == 2
        && ENDPOINT_ROLES
            .iter()
            .any(|roles| claim.endpoints[0].role == roles[0] && claim.endpoints[1].role == roles[1])
        && claim.endpoints[0].entity != claim.endpoints[1].entity
        && claim
            .endpoints
            .iter()
            .all(|endpoint| entities.contains(endpoint.entity.as_str()))
        && bounded_text(&claim.proposition, 8_000, false)
        && single_line(&claim.attribution, 500, false)
        && claim.qualifiers.len() <= 16
        && claim
            .qualifiers
            .iter()
            .map(|qualifier| qualifier.key.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            == claim.qualifiers.len()
        && claim.qualifiers.iter().all(|qualifier| {
            valid_token(&qualifier.key, 64) && single_line(&qualifier.value, 512, false)
        })
        && (1..=32).contains(&claim.sources.len())
        && claim.sources.iter().collect::<BTreeSet<_>>().len() == claim.sources.len()
        && claim
            .sources
            .iter()
            .all(|source| sources.contains(source.as_str()))
        && claim
            .quote
            .as_deref()
            .is_none_or(|quote| bounded_text(quote, MAX_QUOTE, false))
        && [
            &claim.published_at,
            &claim.effective,
            &claim.valid_at,
            &claim.invalid_at,
        ]
        .into_iter()
        .flatten()
        .all(|point| crate::knowledge::validate_time_point(point).is_ok())
}

fn validate_source(source: &SourceRecord) -> bool {
    valid_key(&source.key, 's')
        && single_line(&source.title, 200, false)
        && single_line(&source.publisher, 200, true)
        && source.url.as_deref().is_none_or(valid_url)
        && source
            .filing
            .as_ref()
            .is_none_or(|filing| valid_token(&filing.id, 64))
        && source
            .doi
            .as_deref()
            .is_none_or(|doi| single_line(doi, 200, false))
        && source
            .published_at
            .as_ref()
            .is_none_or(|point| crate::knowledge::validate_time_point(point).is_ok())
}

/// Map records' shapes and bounds. The importer runs it again after hidden
/// characters are stripped, so a cleaned record still loads into the map.
pub(crate) fn world_records_valid(
    entities: &[EntityRecord],
    claims: &[ClaimRecord],
    sources: &[SourceRecord],
) -> bool {
    let entity_keys: BTreeSet<&str> = entities.iter().map(|e| e.key.as_str()).collect();
    let source_keys: BTreeSet<&str> = sources.iter().map(|s| s.key.as_str()).collect();
    let claim_keys: BTreeSet<&str> = claims.iter().map(|c| c.key.as_str()).collect();
    claim_keys.len() == claims.len()
        && entity_keys.len() == entities.len()
        && source_keys.len() == sources.len()
        && entities.iter().all(validate_entity)
        && sources.iter().all(validate_source)
        && claims
            .iter()
            .all(|claim| validate_claim(claim, &entity_keys, &source_keys))
}

/// The structural validator: caps, exact path allowlist, manifest, hashes,
/// UTF-8 text, delimiter spoofs and record shapes. No text is interpreted here.
pub(crate) fn validate(files: &Files) -> Result<ParsedPack, Rejection> {
    if files.len() > MAX_FILES || files.values().map(Vec::len).sum::<usize>() > MAX_BYTES {
        return Err(Rejection::TooLarge);
    }
    let manifest_bytes = files.get(MANIFEST).ok_or(Rejection::NotPack)?;
    let mut roles = BTreeMap::new();
    for path in files.keys() {
        if path.split('/').count() > MAX_DEPTH {
            return Err(Rejection::Unsafe);
        }
        roles.insert(path.as_str(), path_role(path).ok_or(Rejection::Unsafe)?);
    }
    let manifest = manifest_from(manifest_bytes)?;
    validate_manifest(&manifest)?;
    let listed: BTreeSet<&str> = manifest.files.keys().map(String::as_str).collect();
    let present: BTreeSet<&str> = files
        .keys()
        .map(String::as_str)
        .filter(|path| *path != MANIFEST)
        .collect();
    if listed != present {
        return Err(Rejection::Damaged);
    }
    for (path, entry) in &manifest.files {
        let bytes = &files[path];
        if entry.bytes != bytes.len() as u64 || entry.sha256 != sha256_hex(bytes) {
            return Err(Rejection::Damaged);
        }
    }
    let flat = roles
        .values()
        .filter(|role| matches!(role, PathRole::FlatMethod | PathRole::FlatPrinciple))
        .count();
    let nested = roles
        .values()
        .filter(|role| {
            !matches!(
                role,
                PathRole::Manifest
                    | PathRole::Readme
                    | PathRole::FlatMethod
                    | PathRole::FlatPrinciple
            )
        })
        .count();
    let shape_ok = match manifest.kind {
        PackKind::Loupist => flat == 0,
        PackKind::Method => {
            nested == 0 && flat == 1 && roles.values().any(|r| *r == PathRole::FlatMethod)
        }
        PackKind::Principle => {
            nested == 0 && flat == 1 && roles.values().any(|r| *r == PathRole::FlatPrinciple)
        }
    };
    if !shape_ok {
        return Err(Rejection::Damaged);
    }
    let mut parsed = ParsedPack {
        manifest: manifest.clone(),
        profile: None,
        perspective: None,
        methods: Vec::new(),
        principles: Vec::new(),
        entities: Vec::new(),
        claims: Vec::new(),
        sources: Vec::new(),
        extension: None,
    };
    for (path, role) in &roles {
        let bytes = &files[*path];
        match role {
            PathRole::Manifest => {}
            PathRole::Preview => {
                if bytes.len() > MAX_PREVIEW || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                    return Err(Rejection::Damaged);
                }
            }
            PathRole::Readme | PathRole::License => {
                if bytes.len() > MAX_MARKDOWN {
                    return Err(Rejection::TooLarge);
                }
                text_of(bytes)?;
            }
            PathRole::Profile
            | PathRole::Perspective
            | PathRole::Method
            | PathRole::Principle
            | PathRole::FlatMethod
            | PathRole::FlatPrinciple => {
                if bytes.len() > MAX_MARKDOWN {
                    return Err(Rejection::TooLarge);
                }
                let text = text_of(bytes)?;
                match role {
                    PathRole::Profile => {
                        let profile = parse_profile(text)?;
                        if !single_line(&profile.name, MAX_NAME, false)
                            || !bounded_chars(&profile.focus, MAX_FOCUS, true)
                            || !matches!(profile.tone.as_str(), "careful" | "balanced" | "direct")
                            || !single_line(&profile.language, 16, false)
                            || !valid_token(&profile.avatar, 32)
                            || !bounded_text(&profile.description, 4_000, true)
                        {
                            return Err(Rejection::Damaged);
                        }
                        parsed.profile = Some(profile);
                    }
                    PathRole::Perspective => {
                        let method = parse_method(text)?;
                        if method.key != "perspective"
                            || !crate::knowledge::lens_page::title_ok(&method.title)
                            || !validate_lens_doc(&method.document)
                            || !validate_origin(&method.origin)
                        {
                            return Err(Rejection::Damaged);
                        }
                        parsed.perspective = Some(method);
                    }
                    PathRole::Method | PathRole::FlatMethod => {
                        let method = parse_method(text)?;
                        if !valid_key(&method.key, 'm')
                            || !crate::knowledge::lens_page::title_ok(&method.title)
                            || !validate_lens_doc(&method.document)
                            || !validate_origin(&method.origin)
                        {
                            return Err(Rejection::Damaged);
                        }
                        parsed.methods.push(method);
                    }
                    _ => {
                        let principle = parse_principle(text)?;
                        if !valid_key(&principle.key, 'p')
                            || !bounded_chars(&principle.statement, 280, false)
                            || !principle
                                .why
                                .as_deref()
                                .is_none_or(|why| bounded_chars(why, 500, true))
                            || !principle
                                .example
                                .as_deref()
                                .is_none_or(|example| bounded_chars(example, 500, true))
                            || !validate_origin(&principle.origin)
                        {
                            return Err(Rejection::Damaged);
                        }
                        parsed.principles.push(principle);
                    }
                }
            }
            PathRole::Schema => {
                if bytes.len() > MAX_MARKDOWN {
                    return Err(Rejection::TooLarge);
                }
                let value = decoded_json(text_of(bytes)?)?;
                let count = |name: &str| {
                    value
                        .get(name)
                        .and_then(serde_json::Value::as_array)
                        .map(Vec::len)
                };
                let object_ok = value.as_object().is_some_and(|object| {
                    object
                        .keys()
                        .all(|key| matches!(key.as_str(), "types" | "relations"))
                });
                if !object_ok
                    || count("types").is_none_or(|n| n > 4)
                    || count("relations").is_none_or(|n| n > 6)
                {
                    return Err(Rejection::Damaged);
                }
                parsed.extension = Some(value);
            }
            PathRole::Entities => parsed.entities = jsonl(bytes, MAX_ENTITIES)?,
            PathRole::Claims => parsed.claims = jsonl(bytes, MAX_CLAIMS)?,
            PathRole::Sources => parsed.sources = jsonl(bytes, MAX_SOURCES)?,
        }
    }
    if parsed.methods.len() > MAX_METHODS || parsed.principles.len() > MAX_PRINCIPLES {
        return Err(Rejection::TooLarge);
    }
    let unique = |keys: Vec<&str>| {
        let count = keys.len();
        keys.into_iter().collect::<BTreeSet<_>>().len() == count
    };
    let records_ok = unique(parsed.methods.iter().map(|m| m.key.as_str()).collect())
        && unique(parsed.principles.iter().map(|p| p.key.as_str()).collect())
        && world_records_valid(&parsed.entities, &parsed.claims, &parsed.sources);
    if !records_ok {
        return Err(Rejection::Damaged);
    }
    let counts = Counts {
        perspective: u32::from(parsed.perspective.is_some()),
        methods: parsed.methods.len() as u32,
        principles: parsed.principles.len() as u32,
        entities: parsed.entities.len() as u32,
        claims: parsed.claims.len() as u32,
        sources: parsed.sources.len() as u32,
    };
    if counts != manifest.counts {
        return Err(Rejection::Damaged);
    }
    parsed
        .methods
        .sort_by(|a, b| a.title.cmp(&b.title).then(a.key.cmp(&b.key)));
    parsed
        .principles
        .sort_by(|a, b| a.statement.cmp(&b.statement).then(a.key.cmp(&b.key)));
    Ok(parsed)
}

/// Writes the manifest over `content` files, listing and hashing every one.
pub(crate) fn seal(mut manifest: Manifest, mut files: Files) -> (Manifest, Files) {
    files.remove(MANIFEST);
    manifest.files = files
        .iter()
        .map(|(path, bytes)| (path.clone(), file_entry(bytes)))
        .collect();
    let mut bytes = serde_json::to_vec_pretty(&manifest).expect("manifest serializes");
    bytes.push(b'\n');
    files.insert(MANIFEST.into(), bytes);
    (manifest, files)
}
