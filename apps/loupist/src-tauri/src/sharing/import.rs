//! Opening, reviewing and adding packs. A pack is staged in memory for review
//! (never inside a Loupist, gone after an hour); nothing is active before Add,
//! and Lens text becomes a record only when the person accepts it.
use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::path::{Component, Path};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::pack::{self, Files, ItemOrigin, License, PackKind, ParsedPack, Rejection};
use super::scan::{clean_and_scan, strip_hidden, Flag, ItemFinding};

const STAGING_TTL: Duration = Duration::from_secs(60 * 60);
/// Four reviews at once; unit tests share this process-wide map in parallel.
const MAX_STAGED: usize = if cfg!(test) { 64 } else { 4 };
/// macOS adds these on its own; they are skipped, not treated as hidden files.
const OS_METADATA: [&str; 2] = [".DS_Store", "__MACOSX"];

fn os_metadata(path: &str) -> bool {
    path.split('/').any(|part| OS_METADATA.contains(&part))
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', '\0'])
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn hidden(path: &str) -> bool {
    path.split('/').any(|part| part.starts_with('.'))
}

fn folder_read_error(error: crate::fs::artifact_trust::ArtifactTrustError) -> Rejection {
    use crate::fs::artifact_trust::ArtifactTrustError;
    match error {
        ArtifactTrustError::Oversized => Rejection::TooLarge,
        ArtifactTrustError::Untrusted => Rejection::Unsafe,
        ArtifactTrustError::Io(_) => Rejection::NotPack,
    }
}

/// A local folder on the Skill-importer bounds. Hidden files, links and
/// programs reject the pack; a folder holding exactly one pack folder opens it.
pub(crate) fn read_folder(root: &Path) -> Result<Files, Rejection> {
    fn visit(
        root: &Path,
        directory: &Path,
        depth: usize,
        files: &mut Files,
        total: &mut usize,
    ) -> Result<(), Rejection> {
        if depth > pack::MAX_DEPTH {
            return Err(Rejection::Unsafe);
        }
        let entries = std::fs::read_dir(directory).map_err(|_| Rejection::NotPack)?;
        for entry in entries {
            let entry = entry.map_err(|_| Rejection::NotPack)?;
            let name = entry.file_name().to_string_lossy().to_string();
            if OS_METADATA.contains(&name.as_str()) {
                continue;
            }
            let kind = entry.file_type().map_err(|_| Rejection::NotPack)?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|_| Rejection::Unsafe)?
                .to_string_lossy()
                .replace('\\', "/");
            if name.starts_with('.') || kind.is_symlink() {
                return Err(Rejection::Unsafe);
            }
            if kind.is_dir() {
                visit(root, &path, depth + 1, files, total)?;
                continue;
            }
            if !kind.is_file() {
                return Err(Rejection::Unsafe);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = entry
                    .metadata()
                    .map_err(|_| Rejection::NotPack)?
                    .permissions()
                    .mode();
                if mode & 0o111 != 0 {
                    return Err(Rejection::Unsafe);
                }
            }
            if files.len() >= pack::MAX_FILES {
                return Err(Rejection::TooLarge);
            }
            let bytes = crate::fs::artifact_trust::read_private_regular_file_bounded(
                &path,
                pack::MAX_BYTES as u64,
            )
            .map_err(folder_read_error)?;
            *total += bytes.len();
            if *total > pack::MAX_BYTES {
                return Err(Rejection::TooLarge);
            }
            files.insert(relative, bytes);
        }
        Ok(())
    }
    let metadata = std::fs::symlink_metadata(root).map_err(|_| Rejection::NotPack)?;
    if !metadata.is_dir() {
        return Err(Rejection::NotPack);
    }
    let root = if root.join(pack::MANIFEST).exists() {
        root.to_path_buf()
    } else {
        let mut folders = std::fs::read_dir(root)
            .map_err(|_| Rejection::NotPack)?
            .filter_map(Result::ok)
            .filter(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                !OS_METADATA.contains(&name.as_str())
            })
            .collect::<Vec<_>>();
        match folders.as_slice() {
            [only]
                if only.file_type().is_ok_and(|kind| kind.is_dir())
                    && only.path().join(pack::MANIFEST).exists() =>
            {
                folders.remove(0).path()
            }
            _ => return Err(Rejection::NotPack),
        }
    };
    let mut files = Files::new();
    visit(&root, &root, 1, &mut files, &mut 0)?;
    Ok(files)
}

/// A ZIP (a saved pack, or a GitHub archive). A single top-level folder is
/// stripped; `subpath` keeps one folder of a repository.
pub(crate) fn read_zip(bytes: &[u8], subpath: Option<&str>) -> Result<Files, Rejection> {
    if bytes.len() > pack::MAX_BYTES {
        return Err(Rejection::TooLarge);
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| Rejection::NotPack)?;
    if archive.len() > 20_000 {
        return Err(Rejection::TooLarge);
    }
    archive
        .validate_header_consistency()
        .map_err(|_| Rejection::Damaged)?;
    let mut entries = Vec::new();
    let mut total = 0usize;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|_| Rejection::Damaged)?;
        let name = file.name().map_err(|_| Rejection::Damaged)?.into_owned();
        if file.is_dir() || os_metadata(&name) {
            continue;
        }
        if !safe_relative(&name) {
            return Err(Rejection::Unsafe);
        }
        if let Some(mode) = file.unix_mode() {
            if mode & 0o170000 == 0o120000 || mode & 0o111 != 0 {
                return Err(Rejection::Unsafe);
            }
        }
        let mut contents = Vec::new();
        (&mut file)
            .take(pack::MAX_BYTES as u64 + 1)
            .read_to_end(&mut contents)
            .map_err(|_| Rejection::Damaged)?;
        total += contents.len();
        if total > pack::MAX_BYTES * 2 {
            return Err(Rejection::TooLarge);
        }
        entries.push((name, contents));
    }
    let mut prefix = String::new();
    if !entries.iter().any(|(name, _)| name == pack::MANIFEST) {
        let first: Vec<&str> = entries
            .iter()
            .map(|(name, _)| name.split('/').next().unwrap_or_default())
            .collect();
        if let Some(head) = first.first() {
            if first.iter().all(|part| part == head)
                && entries.iter().all(|(name, _)| name.contains('/'))
            {
                prefix = format!("{head}/");
            }
        }
    }
    if let Some(subpath) = subpath {
        prefix.push_str(subpath.trim_matches('/'));
        prefix.push('/');
    }
    let mut files = Files::new();
    let mut size = 0usize;
    for (name, contents) in entries {
        let Some(path) = name.strip_prefix(&prefix) else {
            if subpath.is_some() {
                continue;
            }
            return Err(Rejection::NotPack);
        };
        if hidden(path) {
            return Err(Rejection::Unsafe);
        }
        if path.split('/').count() > pack::MAX_DEPTH {
            return Err(Rejection::Unsafe);
        }
        size += contents.len();
        if files.len() >= pack::MAX_FILES || size > pack::MAX_BYTES {
            return Err(Rejection::TooLarge);
        }
        files.insert(path.to_owned(), contents);
    }
    if files.is_empty() {
        return Err(Rejection::NotPack);
    }
    Ok(files)
}

/// Where a pack came from.
#[derive(Debug, Clone, Default)]
pub(crate) struct Fetched {
    pub files: Files,
    pub source_url: Option<String>,
    pub label: Option<String>,
    pub repo: Option<String>,
    pub path: Option<String>,
    pub commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredItem {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<pack::PackDoc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ItemOrigin>,
    pub markdown: String,
    pub findings: Vec<ItemFinding>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct ItemView {
    pub key: String,
    pub kind: &'static str,
    pub title: String,
    pub markdown: String,
    pub findings: Vec<ItemFinding>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct OriginView {
    pub id: String,
    pub name: String,
    pub version: u32,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreditView {
    pub name: String,
    pub author: Option<String>,
    pub license: Option<License>,
    pub url: Option<String>,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct ManifestView {
    pub id: String,
    pub kind: PackKind,
    pub name: String,
    pub version: u32,
    pub created_at: String,
    pub language: String,
    pub license: License,
    pub license_url: String,
    pub redistributable: bool,
    pub author: Option<String>,
    pub origin: Vec<OriginView>,
    pub credits: Vec<CreditView>,
    pub asset_classes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProfileView {
    pub name: String,
    pub focus: String,
    pub tone: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct HostCount {
    pub host: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExtensionCounts {
    pub types: u32,
    pub relations: u32,
}

/// One map record whose text looks like an instruction, names a tool, asks
/// for secrets, could send data out or had hidden characters (removed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct MapFinding {
    pub kind: &'static str,
    /// The record's own name, relationship or title, hidden characters removed.
    pub label: String,
    pub flags: Vec<Flag>,
}

/// Map findings listed on the review screen; the rest are counted.
const MAX_MAP_FINDINGS: usize = 50;

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct WorldView {
    pub names: u32,
    pub links: u32,
    pub sources: u32,
    pub source_hosts: Vec<HostCount>,
    pub more_hosts: u32,
    pub kinds: Vec<String>,
    pub relations: Vec<String>,
    pub extension: ExtensionCounts,
    pub findings: Vec<MapFinding>,
    /// Every flagged map record, listed or not.
    pub flagged: u32,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct PackPreview {
    pub staging_id: String,
    pub manifest: ManifestView,
    pub source_label: Option<String>,
    pub source_url: Option<String>,
    pub profile: ProfileView,
    pub perspective: Option<ItemView>,
    pub methods: Vec<ItemView>,
    pub principles: Vec<ItemView>,
    pub world: WorldView,
    /// Flags on the pack's own words: its name, author, origin and profile.
    pub profile_findings: Vec<Flag>,
    pub flagged_count: u32,
    pub blocked: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum OpenResult {
    Preview {
        preview: Box<PackPreview>,
    },
    #[serde(rename_all = "camelCase")]
    Rejected {
        reason: Rejection,
        source_label: Option<String>,
    },
}

/// One reviewed pack, held for Add.
#[derive(Debug, Clone)]
pub(crate) struct Staged {
    pub parsed: ParsedPack,
    pub fetched: Fetched,
    pub items: Vec<(String, &'static str, StoredItem)>,
    pub profile: ProfileView,
    /// The profile focus is used only when the scan found nothing in it.
    pub focus_clean: bool,
    pub blocked: Option<&'static str>,
    created: Instant,
}

fn staging() -> &'static Mutex<BTreeMap<String, Arc<Staged>>> {
    static STAGING: std::sync::OnceLock<Mutex<BTreeMap<String, Arc<Staged>>>> =
        std::sync::OnceLock::new();
    STAGING.get_or_init(Default::default)
}

pub(crate) fn staged(staging_id: &str) -> Option<Arc<Staged>> {
    let mut staging = staging().lock().ok()?;
    staging.retain(|_, staged| staged.created.elapsed() < STAGING_TTL);
    staging.get(staging_id).cloned()
}

pub(crate) fn unstage(staging_id: &str) {
    if let Ok(mut staging) = staging().lock() {
        staging.remove(staging_id);
    }
}

pub(crate) fn method_markdown(document: &pack::PackDoc) -> String {
    let mut text = document.summary.clone();
    for section in &document.sections {
        if !section.markdown.trim().is_empty() {
            text.push_str(&format!(
                "\n\n## {}\n\n{}",
                section.heading, section.markdown
            ));
        }
    }
    text.trim_start().to_owned()
}

pub(crate) fn principle_markdown(
    statement: &str,
    why: Option<&str>,
    example: Option<&str>,
) -> String {
    let mut text = statement.to_owned();
    for (heading, field) in [("Why", why), ("Example", example)] {
        if let Some(field) = field.filter(|field| !field.trim().is_empty()) {
            text.push_str(&format!("\n\n## {heading}\n\n{field}"));
        }
    }
    text
}

fn cleaned(text: &str) -> String {
    strip_hidden(text).0
}

fn title_findings(title: &str, findings: &mut Vec<ItemFinding>) {
    for finding in clean_and_scan(title).1 {
        if !findings.iter().any(|known| known.flag == finding.flag) {
            findings.push(ItemFinding {
                flag: finding.flag,
                lines: Vec::new(),
            });
        }
    }
    findings.sort_by_key(|finding| finding.flag);
}

fn origin_findings(origin: Option<&ItemOrigin>, findings: &mut Vec<ItemFinding>) {
    if let Some(origin) = origin {
        for entry in origin.entries() {
            title_findings(&entry.name, findings);
            if let Some(author) = entry.author {
                title_findings(&author, findings);
            }
        }
    }
}

fn method_item(method: &pack::MethodDoc) -> StoredItem {
    let raw = method_markdown(&method.document);
    let (markdown, mut findings) = clean_and_scan(&raw);
    title_findings(&method.title, &mut findings);
    origin_findings(method.origin.as_ref(), &mut findings);
    let document = &method.document;
    StoredItem {
        title: cleaned(&method.title),
        document: Some(pack::PackDoc {
            summary: cleaned(&document.summary),
            sections: document
                .sections
                .iter()
                .map(|section| pack::PackSection {
                    key: section.key.clone(),
                    heading: cleaned(&section.heading),
                    markdown: cleaned(&section.markdown),
                })
                .collect(),
        }),
        statement: None,
        why: None,
        example: None,
        origin: method.origin.clone().map(cleaned_origin),
        markdown,
        findings,
    }
}

fn principle_item(principle: &pack::PrincipleDoc) -> StoredItem {
    let raw = principle_markdown(
        &principle.statement,
        principle.why.as_deref(),
        principle.example.as_deref(),
    );
    let (markdown, mut findings) = clean_and_scan(&raw);
    origin_findings(principle.origin.as_ref(), &mut findings);
    let statement = cleaned(&principle.statement);
    StoredItem {
        title: statement.split_whitespace().collect::<Vec<_>>().join(" "),
        document: None,
        statement: Some(statement),
        why: principle.why.as_deref().map(cleaned),
        example: principle.example.as_deref().map(cleaned),
        origin: principle.origin.clone().map(cleaned_origin),
        markdown,
        findings,
    }
}

fn view(key: &str, kind: &'static str, item: &StoredItem) -> ItemView {
    ItemView {
        key: key.to_owned(),
        kind,
        title: item.title.clone(),
        markdown: item.markdown.clone(),
        findings: item.findings.clone(),
    }
}

pub(crate) fn stored_view(key: &str, kind: &str, body_json: &str) -> Option<ItemView> {
    let item: StoredItem = serde_json::from_str(body_json).ok()?;
    let kind = match kind {
        "perspective" => "perspective",
        "principle" => "principle",
        _ => "method",
    };
    Some(view(key, kind, &item))
}

fn clean_entry(entry: &mut pack::OriginEntry) {
    clean_in_place(&mut entry.name);
    if let Some(author) = &mut entry.author {
        clean_in_place(author);
    }
}

fn cleaned_origin(mut origin: ItemOrigin) -> ItemOrigin {
    clean_in_place(&mut origin.name);
    if let Some(author) = &mut origin.author {
        clean_in_place(author);
    }
    origin.chain.iter_mut().for_each(clean_entry);
    origin
}

fn clean_metadata_and_lens(parsed: &mut ParsedPack) {
    let manifest = &mut parsed.manifest;
    clean_in_place(&mut manifest.name);
    if let Some(author) = &mut manifest.author {
        clean_in_place(&mut author.display);
    }
    manifest
        .origin
        .iter_mut()
        .chain(&mut manifest.credits)
        .for_each(clean_entry);
    if let Some(profile) = &mut parsed.profile {
        clean_in_place(&mut profile.name);
        clean_in_place(&mut profile.focus);
        clean_in_place(&mut profile.description);
    }
    for method in parsed.perspective.iter_mut().chain(&mut parsed.methods) {
        clean_in_place(&mut method.title);
        clean_in_place(&mut method.document.summary);
        for section in &mut method.document.sections {
            clean_in_place(&mut section.heading);
            clean_in_place(&mut section.markdown);
        }
        method.origin = method.origin.take().map(cleaned_origin);
    }
    for principle in &mut parsed.principles {
        clean_in_place(&mut principle.statement);
        if let Some(why) = &mut principle.why {
            clean_in_place(why);
        }
        if let Some(example) = &mut principle.example {
            clean_in_place(example);
        }
        principle.origin = principle.origin.take().map(cleaned_origin);
    }
}

fn ranked(counts: BTreeMap<String, u32>) -> Vec<(String, u32)> {
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    ranked
}

fn flags_of<'a>(texts: impl IntoIterator<Item = &'a str>) -> Vec<Flag> {
    let mut flags = std::collections::BTreeSet::new();
    for text in texts {
        flags.extend(
            clean_and_scan(text)
                .1
                .into_iter()
                .map(|finding| finding.flag),
        );
    }
    flags.into_iter().collect()
}

fn clean_in_place(text: &mut String) {
    let (cleaned, _) = strip_hidden(text);
    *text = cleaned;
}

/// Map text gets the same hygiene as Lens text: hidden characters are removed
/// from what will be added, and the instruction-like scan runs over every
/// string of every name, relationship and Source.
fn clean_world(parsed: &mut ParsedPack) -> (Vec<MapFinding>, u32) {
    let mut findings = Vec::new();
    let mut flagged = 0;
    let mut note = |kind: &'static str, label: &str, flags: Vec<Flag>| {
        if flags.is_empty() {
            return;
        }
        flagged += 1;
        if findings.len() < MAX_MAP_FINDINGS {
            findings.push(MapFinding {
                kind,
                label: strip_hidden(label).0.chars().take(120).collect(),
                flags,
            });
        }
    };
    for entity in &mut parsed.entities {
        let identifiers = &entity.identifiers;
        let flags = flags_of(
            [entity.name.as_str(), entity.summary.as_str()]
                .into_iter()
                .chain(entity.aliases.iter().map(String::as_str))
                .chain(entity.local_names.iter().map(String::as_str))
                .chain(
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
                ),
        );
        note("entity", &entity.name, flags);
        clean_in_place(&mut entity.name);
        clean_in_place(&mut entity.summary);
        entity.aliases.iter_mut().for_each(clean_in_place);
        entity.local_names.iter_mut().for_each(clean_in_place);
        let identifiers = &mut entity.identifiers;
        for value in [
            &mut identifiers.ticker,
            &mut identifiers.exchange,
            &mut identifiers.cik,
            &mut identifiers.dart_corp_code,
            &mut identifiers.isin,
        ]
        .into_iter()
        .flatten()
        {
            clean_in_place(value);
        }
    }
    for claim in &mut parsed.claims {
        let flags = flags_of(
            [claim.proposition.as_str(), claim.attribution.as_str()]
                .into_iter()
                .chain(claim.qualifiers.iter().map(|q| q.value.as_str()))
                .chain(claim.quote.as_deref()),
        );
        note("claim", &claim.proposition, flags);
        clean_in_place(&mut claim.proposition);
        clean_in_place(&mut claim.attribution);
        claim
            .qualifiers
            .iter_mut()
            .for_each(|qualifier| clean_in_place(&mut qualifier.value));
        if let Some(quote) = &mut claim.quote {
            clean_in_place(quote);
        }
    }
    for source in &mut parsed.sources {
        let flags = flags_of(
            [source.title.as_str(), source.publisher.as_str()]
                .into_iter()
                .chain(source.doi.as_deref()),
        );
        note("source", &source.title, flags);
        clean_in_place(&mut source.title);
        clean_in_place(&mut source.publisher);
        if let Some(doi) = &mut source.doi {
            clean_in_place(doi);
        }
    }
    (findings, flagged)
}

fn world_view(parsed: &ParsedPack, findings: Vec<MapFinding>, flagged: u32) -> WorldView {
    let mut hosts = BTreeMap::<String, u32>::new();
    for source in &parsed.sources {
        let host = source
            .url
            .as_deref()
            .and_then(|url| reqwest::Url::parse(url).ok())
            .and_then(|url| {
                url.host_str()
                    .map(|h| h.trim_start_matches("www.").to_owned())
            })
            .unwrap_or_else(|| source.publisher.clone());
        if !host.is_empty() {
            *hosts.entry(host).or_default() += 1;
        }
    }
    let hosts = ranked(hosts);
    let more_hosts = hosts.iter().skip(6).map(|(_, count)| count).sum();
    let mut kinds = BTreeMap::<String, u32>::new();
    for entity in &parsed.entities {
        *kinds.entry(entity.kind.clone()).or_default() += 1;
    }
    let mut relations = BTreeMap::<String, u32>::new();
    for claim in &parsed.claims {
        *relations.entry(claim.relation.clone()).or_default() += 1;
    }
    let count = |name: &str| {
        parsed
            .extension
            .as_ref()
            .and_then(|value| value.get(name))
            .and_then(Value::as_array)
            .map_or(0, Vec::len) as u32
    };
    WorldView {
        names: parsed.entities.len() as u32,
        links: parsed.claims.len() as u32,
        sources: parsed.sources.len() as u32,
        source_hosts: hosts
            .into_iter()
            .take(6)
            .map(|(host, count)| HostCount { host, count })
            .collect(),
        more_hosts,
        kinds: ranked(kinds).into_iter().take(8).map(|(k, _)| k).collect(),
        relations: ranked(relations)
            .into_iter()
            .take(8)
            .map(|(r, _)| r)
            .collect(),
        extension: ExtensionCounts {
            types: count("types"),
            relations: count("relations"),
        },
        findings,
        flagged,
    }
}

/// Every perspective, method and principle of a validated pack, cleaned and scanned.
pub(crate) fn items_of(parsed: &ParsedPack) -> Vec<(String, &'static str, StoredItem)> {
    let mut items = Vec::new();
    if let Some(perspective) = &parsed.perspective {
        items.push((
            perspective.key.clone(),
            "perspective",
            method_item(perspective),
        ));
    }
    for method in &parsed.methods {
        items.push((method.key.clone(), "method", method_item(method)));
    }
    for principle in &parsed.principles {
        items.push((
            principle.key.clone(),
            "principle",
            principle_item(principle),
        ));
    }
    items
}

/// The stored form of an item and its hash; versions compare items by it.
pub(crate) fn body_of(item: &StoredItem) -> (String, String) {
    let json = serde_json::to_string(item).expect("stored item serializes");
    let hash = super::pack::sha256_hex(json.as_bytes());
    (json, hash)
}

/// The item as the review shows it.
pub(crate) fn item_view(key: &str, kind: &'static str, item: &StoredItem) -> ItemView {
    view(key, kind, item)
}

/// Validates fetched files and stages a review, or reports why it cannot be added.
pub(crate) fn stage(fetched: Fetched, blocked: Option<&'static str>) -> OpenResult {
    let rejected = |reason| OpenResult::Rejected {
        reason,
        source_label: fetched.label.clone(),
    };
    let mut parsed = match pack::validate(&fetched.files) {
        Ok(parsed) => parsed,
        Err(reason) => return rejected(reason),
    };
    let focus_clean = parsed.profile.as_ref().is_none_or(|profile| {
        clean_and_scan(&profile.focus).1.is_empty()
            && clean_and_scan(&profile.description).1.is_empty()
    });
    // The pack's own words: manifest name, author and origin, and the profile.
    let manifest = &parsed.manifest;
    let profile_findings = flags_of(
        std::iter::once(manifest.name.as_str())
            .chain(
                manifest
                    .author
                    .as_ref()
                    .map(|author| author.display.as_str()),
            )
            .chain(
                manifest
                    .origin
                    .iter()
                    .chain(&manifest.credits)
                    .flat_map(|entry| {
                        std::iter::once(entry.name.as_str()).chain(entry.author.as_deref())
                    }),
            )
            .chain(parsed.profile.iter().flat_map(|profile| {
                [
                    profile.name.as_str(),
                    profile.focus.as_str(),
                    profile.description.as_str(),
                ]
            })),
    );
    // Scan the original strings, then retain only cleaned text for review and Add.
    let items = items_of(&parsed);
    clean_metadata_and_lens(&mut parsed);
    let (map_findings, map_flagged) = clean_world(&mut parsed);
    if !pack::cleaned_text_valid(&parsed)
        || !pack::world_records_valid(&parsed.entities, &parsed.claims, &parsed.sources)
    {
        return rejected(Rejection::Damaged);
    }
    let manifest = &parsed.manifest;
    let profile = match &parsed.profile {
        Some(profile) => ProfileView {
            name: profile.name.clone(),
            focus: profile.focus.clone(),
            tone: profile.tone.clone(),
            description: profile.description.clone(),
        },
        None => ProfileView {
            name: manifest.name.clone(),
            focus: String::new(),
            tone: "balanced".into(),
            description: String::new(),
        },
    };
    let staging_id = format!("stg_{}", uuid::Uuid::new_v4().simple());
    let preview = PackPreview {
        staging_id: staging_id.clone(),
        manifest: ManifestView {
            id: manifest.id.clone(),
            kind: manifest.kind,
            name: manifest.name.clone(),
            version: manifest.version,
            created_at: manifest.created_at.clone(),
            language: manifest.language.clone(),
            license: manifest.license,
            license_url: manifest
                .license
                .url(manifest.terms_url.as_deref())
                .to_owned(),
            redistributable: manifest.license.redistributable(),
            author: manifest
                .author
                .as_ref()
                .map(|author| author.display.clone()),
            origin: manifest
                .origin
                .iter()
                .map(|origin| OriginView {
                    id: origin.id.clone(),
                    name: origin.name.clone(),
                    version: origin.version,
                    url: origin.url.clone(),
                })
                .collect(),
            credits: manifest
                .credits
                .iter()
                .map(|credit| CreditView {
                    name: credit.name.clone(),
                    author: credit.author.clone(),
                    license: credit.license,
                    url: credit.url.clone(),
                    version: credit.version,
                })
                .collect(),
            asset_classes: manifest.asset_classes.clone(),
        },
        source_label: fetched.label.clone(),
        source_url: fetched.source_url.clone(),
        profile: profile.clone(),
        perspective: items
            .iter()
            .find(|(_, kind, _)| *kind == "perspective")
            .map(|(key, kind, item)| view(key, kind, item)),
        methods: items
            .iter()
            .filter(|(_, kind, _)| *kind == "method")
            .map(|(key, kind, item)| view(key, kind, item))
            .collect(),
        principles: items
            .iter()
            .filter(|(_, kind, _)| *kind == "principle")
            .map(|(key, kind, item)| view(key, kind, item))
            .collect(),
        world: world_view(&parsed, map_findings, map_flagged),
        flagged_count: items
            .iter()
            .filter(|(_, _, item)| !item.findings.is_empty())
            .count() as u32
            + map_flagged
            + u32::from(!profile_findings.is_empty()),
        profile_findings,
        blocked,
    };
    if let Ok(mut staging) = staging().lock() {
        staging.retain(|_, staged| staged.created.elapsed() < STAGING_TTL);
        while staging.len() >= MAX_STAGED {
            let oldest = staging
                .iter()
                .min_by_key(|(_, staged)| staged.created)
                .map(|(id, _)| id.clone());
            match oldest {
                Some(id) => staging.remove(&id),
                None => break,
            };
        }
        staging.insert(
            staging_id,
            Arc::new(Staged {
                parsed,
                fetched,
                items,
                profile,
                focus_clean,
                blocked,
                created: Instant::now(),
            }),
        );
    }
    OpenResult::Preview {
        preview: Box::new(preview),
    }
}

/// The JSON origin a shared Lens record carries in its change and growth rows.
pub(crate) fn lens_origin(row: &super::store::ImportRow) -> Value {
    json!({
        "kind": "shared_pack",
        "importId": row.import_id,
        "packId": row.pack_id,
        "name": row.name,
        "author": row.author,
        "license": row.license.id(),
        "redistributable": row.license.redistributable(),
        "version": row.version,
        "url": row.source_url,
        "chain": row.origin,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn zip(entries: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes, mode) in entries {
            let options = zip::write::SimpleFileOptions::default().unix_permissions(*mode & 0o777);
            if mode & 0o170000 == 0o120000 {
                writer
                    .add_symlink(*name, std::str::from_utf8(bytes).unwrap(), options)
                    .unwrap();
                continue;
            }
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn folder_read_failures_keep_their_rejection_class() {
        use crate::fs::artifact_trust::ArtifactTrustError;
        assert_eq!(
            folder_read_error(ArtifactTrustError::Oversized),
            Rejection::TooLarge
        );
        assert_eq!(
            folder_read_error(ArtifactTrustError::Untrusted),
            Rejection::Unsafe
        );
        assert_eq!(
            folder_read_error(ArtifactTrustError::Io(std::io::Error::from(
                std::io::ErrorKind::PermissionDenied
            ))),
            Rejection::NotPack
        );
    }

    #[test]
    fn archives_reject_cloaked_local_filenames_and_preserve_legacy_names() {
        let mut cloaked = zip(&[
            ("safe.txt", b"content", 0o644),
            ("loupist-pack.json", b"{}", 0o644),
        ]);
        // Keep the central directory's safe name while changing only the local
        // header, with the same byte length. No filesystem extraction occurs.
        assert_eq!(&cloaked[30..38], b"safe.txt");
        cloaked[30..38].copy_from_slice(b"../x.txt");
        assert!(read_zip(&cloaked, None).is_err());

        let mut legacy = zip(&[("cafe.txt", b"content", 0o644)]);
        let name = b"cafe.txt";
        let offsets: Vec<_> = legacy
            .windows(name.len())
            .enumerate()
            .filter_map(|(offset, bytes)| (bytes == name).then_some(offset))
            .collect();
        assert_eq!(offsets.len(), 2, "local and central filenames");
        for offset in offsets {
            legacy[offset + 3] = 0x82; // CP437 é, with the UTF-8 flag unset.
        }
        assert_eq!(read_zip(&legacy, None).unwrap()["café.txt"], b"content");
    }

    #[test]
    fn archives_strip_one_top_folder_and_reject_links_programs_and_hidden_files() {
        let files = read_zip(
            &zip(&[
                ("repo-abc/loupist-pack.json", b"{}", 0o644),
                ("repo-abc/README.md", b"#", 0o644),
                ("repo-abc/__MACOSX/._README.md", b"x", 0o644),
            ]),
            None,
        )
        .unwrap();
        assert_eq!(
            files.keys().cloned().collect::<Vec<_>>(),
            ["README.md", "loupist-pack.json"]
        );
        let sub = read_zip(
            &zip(&[
                ("repo-abc/other/x.md", b"x", 0o644),
                ("repo-abc/packs/desk/loupist-pack.json", b"{}", 0o644),
            ]),
            Some("packs/desk"),
        )
        .unwrap();
        assert_eq!(
            sub.keys().cloned().collect::<Vec<_>>(),
            ["loupist-pack.json"]
        );
        assert_eq!(
            read_zip(&zip(&[("loupist-pack.json", b"{}", 0o755)]), None).unwrap_err(),
            Rejection::Unsafe
        );
        assert_eq!(
            read_zip(
                &zip(&[
                    ("loupist-pack.json", b"{}", 0o644),
                    (".hidden", b"x", 0o644)
                ]),
                None
            )
            .unwrap_err(),
            Rejection::Unsafe
        );
        assert_eq!(
            read_zip(&zip(&[("link", b"loupist-pack.json", 0o120777)]), None).unwrap_err(),
            Rejection::Unsafe
        );
        assert_eq!(
            read_zip(b"not a zip", None).unwrap_err(),
            Rejection::NotPack
        );
    }

    #[cfg(unix)]
    #[test]
    fn folders_reject_links_and_hidden_files_and_open_a_single_pack_folder() {
        let root = tempfile::tempdir().unwrap();
        let pack = root.path().join("desk");
        std::fs::create_dir(&pack).unwrap();
        std::fs::write(pack.join("loupist-pack.json"), b"{}").unwrap();
        std::fs::write(root.path().join(".DS_Store"), b"x").unwrap();
        let files = read_folder(root.path()).unwrap();
        assert_eq!(
            files.keys().cloned().collect::<Vec<_>>(),
            ["loupist-pack.json"]
        );
        std::os::unix::fs::symlink(pack.join("loupist-pack.json"), pack.join("README.md")).unwrap();
        assert_eq!(read_folder(&pack).unwrap_err(), Rejection::Unsafe);
        std::fs::remove_file(pack.join("README.md")).unwrap();
        std::fs::write(pack.join(".env"), b"x").unwrap();
        assert_eq!(read_folder(&pack).unwrap_err(), Rejection::Unsafe);
    }
}
