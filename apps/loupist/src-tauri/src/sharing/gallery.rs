//! The community index (W19.6): an open catalog repository whose CI validates
//! each listed pack (it validates, carries no holdings, has a license and fits
//! the caps) and nobody reviews by hand. The index URL is a build constant;
//! without it there is no Gallery, listing, Report or blocked-list check.
//! Rust fetches the index (https, no redirects, ≤ 4 MiB), keeps it in app data
//! for offline use and refreshes it after 6 h or on Refresh; entries the
//! index's `blocked` list names (report-and-remove) never appear.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::pack::{self, License, ASSET_CLASSES};
use super::scan::strip_hidden;

const MAX_INDEX_BYTES: usize = 4 * 1024 * 1024;
const MAX_ENTRIES: usize = 5_000;
const REFRESH_AFTER: Duration = Duration::from_secs(6 * 60 * 60);
/// Map images: PNG, at most 1 MiB each, 64 MiB kept, least recently used dropped first.
const MAX_PREVIEW_CACHE: u64 = 64 * 1024 * 1024;
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
/// Where listed packs' preview images are read; the index host serves only the index.
pub(crate) const RAW_BASE: &str = "https://raw.githubusercontent.com";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct Blocked {
    pub id: String,
    #[serde(default)]
    pub repo: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawCounts {
    #[serde(default)]
    methods: u32,
    #[serde(default)]
    principles: u32,
    #[serde(default)]
    entities: u32,
    #[serde(default)]
    claims: u32,
}

/// One listed pack as the index's CI wrote it (every field read from the pack at `commit`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEntry {
    id: String,
    repo: String,
    #[serde(default)]
    path: Option<String>,
    commit: String,
    name: String,
    #[serde(default)]
    focus: String,
    #[serde(default)]
    author: Option<String>,
    language: String,
    license: License,
    #[serde(default)]
    asset_classes: Vec<String>,
    #[serde(default)]
    counts: RawCounts,
    #[serde(default)]
    stars: u64,
    #[serde(default)]
    forks: u64,
    updated_at: String,
    /// The pack has `preview.png` at `commit`.
    #[serde(default)]
    preview: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawIndex {
    format: String,
    #[serde(default)]
    entries: Vec<serde_json::Value>,
    #[serde(default)]
    blocked: Vec<Blocked>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct EntryCounts {
    pub methods: u32,
    pub principles: u32,
    pub names: u32,
    pub links: u32,
}

/// A validated, listed pack. The renderer gets everything but the location,
/// which Rust resolves from the id when the person presses Preview or Use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct Entry {
    pub id: String,
    #[serde(skip)]
    pub owner: String,
    #[serde(skip)]
    pub repo_name: String,
    #[serde(skip)]
    pub path: Option<String>,
    #[serde(skip)]
    pub commit: String,
    /// "github.com/owner/repo"
    pub source: String,
    pub name: String,
    pub focus: String,
    pub author: Option<String>,
    pub language: String,
    pub license: License,
    pub asset_classes: Vec<String>,
    pub counts: EntryCounts,
    pub stars: u64,
    pub forks: u64,
    pub updated_at: String,
    pub has_preview: bool,
}

impl Entry {
    pub(crate) fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.repo_name)
    }
    /// The GitHub location of the listed pack at its listed commit.
    pub(crate) fn location(&self) -> super::deep_link::GithubLocation {
        super::deep_link::GithubLocation::Repo {
            owner: self.owner.clone(),
            repo: self.repo_name.clone(),
            reference: Some(self.commit.clone()),
            path: self.path.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Index {
    pub entries: Vec<Entry>,
    pub blocked: Vec<Blocked>,
}

fn segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// One line of display text: hidden characters removed, whitespace folded, bounded.
fn text(value: &str, max_chars: usize) -> Option<String> {
    let cleaned = strip_hidden(value).0;
    let line: String = cleaned
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(max_chars)
        .collect();
    (!line.chars().any(char::is_control)).then_some(line)
}

fn entry(raw: RawEntry) -> Option<Entry> {
    if !raw.license.redistributable() {
        return None;
    }
    let (owner, repo_name) = raw.repo.split_once('/')?;
    if !super::github::valid_login(owner) || !super::github::valid_repo_name(repo_name) {
        return None;
    }
    let path = raw
        .path
        .map(|path| path.trim_matches('/').to_owned())
        .filter(|path| !path.is_empty());
    if let Some(path) = &path {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() > 6 || !parts.iter().all(|part| segment(part)) {
            return None;
        }
    }
    let commit_ok = raw.commit.len() == 40 && raw.commit.bytes().all(|b| b.is_ascii_hexdigit());
    let language_ok = (2..=8).contains(&raw.language.len())
        && raw
            .language
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'-');
    let classes_ok = raw.asset_classes.len() <= ASSET_CLASSES.len()
        && raw
            .asset_classes
            .iter()
            .all(|class| ASSET_CLASSES.contains(&class.as_str()));
    let updated_ok = chrono::DateTime::parse_from_rfc3339(&raw.updated_at).is_ok();
    if uuid::Uuid::parse_str(&raw.id).is_err()
        || !commit_ok
        || !language_ok
        || !classes_ok
        || !updated_ok
    {
        return None;
    }
    let name = text(&raw.name, 120).filter(|name| !name.is_empty())?;
    Some(Entry {
        id: raw.id.to_ascii_lowercase(),
        source: format!("github.com/{owner}/{repo_name}"),
        owner: owner.to_owned(),
        repo_name: repo_name.to_owned(),
        path,
        commit: raw.commit.to_ascii_lowercase(),
        name,
        focus: text(&raw.focus, pack::MAX_FOCUS)?,
        author: match raw.author {
            Some(author) => Some(text(&author, 80)?).filter(|author| !author.is_empty()),
            None => None,
        },
        language: raw.language,
        license: raw.license,
        asset_classes: raw.asset_classes,
        counts: EntryCounts {
            methods: raw.counts.methods,
            principles: raw.counts.principles,
            names: raw.counts.entities,
            links: raw.counts.claims,
        },
        stars: raw.stars,
        forks: raw.forks,
        updated_at: raw.updated_at,
        has_preview: raw.preview,
    })
}

/// A `loupist-index/1` document. An entry that does not validate is left out
/// on its own; anything else wrong rejects the document.
pub(crate) fn parse(bytes: &[u8]) -> Option<Index> {
    if bytes.len() > MAX_INDEX_BYTES {
        return None;
    }
    let raw: RawIndex = serde_json::from_slice(bytes).ok()?;
    if raw.format != "loupist-index/1" || raw.entries.len() > MAX_ENTRIES {
        return None;
    }
    let mut seen = BTreeSet::new();
    let entries = raw
        .entries
        .into_iter()
        .filter_map(|value| serde_json::from_value::<RawEntry>(value).ok())
        .filter_map(entry)
        .filter(|entry| find(&raw.blocked, &entry.id, Some(&entry.full_name())).is_none())
        .filter(|entry| seen.insert(entry.id.clone()))
        .collect();
    Some(Index {
        entries,
        blocked: raw.blocked,
    })
}

/// The renderer's reason: reported, unsafe or author.
pub(crate) fn reason(blocked: &Blocked) -> &'static str {
    match blocked.reason.as_str() {
        "reported" => "reported",
        "author" => "author",
        _ => "unsafe",
    }
}

pub(crate) fn find<'a>(
    blocked: &'a [Blocked],
    pack_id: &str,
    repo: Option<&str>,
) -> Option<&'a Blocked> {
    blocked.iter().find(|entry| {
        entry.id.eq_ignore_ascii_case(pack_id)
            || repo.is_some_and(|repo| {
                entry
                    .repo
                    .as_deref()
                    .is_some_and(|blocked| blocked.eq_ignore_ascii_case(repo))
            })
    })
}

/// The index as last read: when, and whether this read reached the index.
#[derive(Debug, Clone)]
pub(crate) struct Loaded {
    pub index: std::sync::Arc<Index>,
    pub fetched_at_ms: i64,
    /// The index could not be reached; this is the copy kept from `fetched_at_ms`.
    pub offline: bool,
}

/// The index last read from each URL (one in a build; tests use their own).
type Memory = std::collections::BTreeMap<String, (Instant, std::sync::Arc<Index>, i64)>;

fn memory() -> &'static Mutex<Memory> {
    static MEMORY: Mutex<Memory> = Mutex::new(Memory::new());
    &MEMORY
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

fn client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Loupist-Sharing")
        .build()
        .ok()
}

/// A bounded GET with no redirects; None for anything but a 200 within `max`.
async fn get(url: &str, max: usize) -> Option<Vec<u8>> {
    use futures_util::StreamExt;
    let response = client()?.get(url).send().await.ok()?;
    if response.status().as_u16() != 200
        || response.content_length().is_some_and(|n| n > max as u64)
    {
        return None;
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.ok()?;
        if bytes.len() + chunk.len() > max {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    Some(bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| std::io::Error::other("no parent"))?;
    std::fs::create_dir_all(directory)?;
    let temporary = directory.join(format!(
        ".{}.{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("cache"),
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

fn modified_ms(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(modified.duration_since(UNIX_EPOCH).ok()?.as_millis() as i64)
}

fn index_file(cache_dir: &Path) -> PathBuf {
    cache_dir.join("index.json")
}

fn remember(url: &str, index: Index, fetched_at_ms: i64) -> std::sync::Arc<Index> {
    let index = std::sync::Arc::new(index);
    if let Ok(mut memory) = memory().lock() {
        memory.insert(
            url.to_owned(),
            (Instant::now(), index.clone(), fetched_at_ms),
        );
    }
    index
}

/// The index from `url`: the copy in memory or in `cache_dir` while younger
/// than 6 h (unless `refresh`), else a fresh read; when the index cannot be
/// read, the last copy marked offline; None when there has never been one.
pub(crate) async fn load_from(
    url: &str,
    cache_dir: Option<&Path>,
    refresh: bool,
) -> Option<Loaded> {
    let held = memory()
        .lock()
        .ok()
        .and_then(|memory| memory.get(url).cloned());
    if !refresh {
        if let Some((at, index, fetched_at_ms)) = &held {
            if at.elapsed() < REFRESH_AFTER {
                return Some(Loaded {
                    index: index.clone(),
                    fetched_at_ms: *fetched_at_ms,
                    offline: false,
                });
            }
        }
        if let Some(directory) = cache_dir {
            let file = index_file(directory);
            if let Some(at) =
                modified_ms(&file).filter(|at| now_ms() - at < REFRESH_AFTER.as_millis() as i64)
            {
                if let Some(index) = std::fs::read(&file).ok().and_then(|bytes| parse(&bytes)) {
                    return Some(Loaded {
                        index: remember(url, index, at),
                        fetched_at_ms: at,
                        offline: false,
                    });
                }
            }
        }
    }
    if let Some(bytes) = get(url, MAX_INDEX_BYTES).await {
        if let Some(index) = parse(&bytes) {
            let at = now_ms();
            if let Some(directory) = cache_dir {
                let _ = write_atomic(&index_file(directory), &bytes);
            }
            return Some(Loaded {
                index: remember(url, index, at),
                fetched_at_ms: at,
                offline: false,
            });
        }
    }
    if let Some((_, index, fetched_at_ms)) = held {
        return Some(Loaded {
            index,
            fetched_at_ms,
            offline: true,
        });
    }
    let directory = cache_dir?;
    let file = index_file(directory);
    let at = modified_ms(&file)?;
    let index = parse(&std::fs::read(&file).ok()?)?;
    Some(Loaded {
        index: remember(url, index, at),
        fetched_at_ms: at,
        offline: true,
    })
}

/// The configured index; None without one.
pub(crate) async fn load(cache_dir: Option<&Path>, refresh: bool) -> Option<Loaded> {
    load_from(super::github::index_url()?, cache_dir, refresh).await
}

/// The listed pack with this id, from the index as last read (no network).
pub(crate) fn cached_entry(entry_id: &str) -> Option<Entry> {
    let memory = memory().lock().ok()?;
    memory.values().find_map(|(_, index, _)| {
        index
            .entries
            .iter()
            .find(|entry| entry.id.eq_ignore_ascii_case(entry_id))
            .cloned()
    })
}

/// The report-and-remove list, if an index is configured and has ever been read.
pub(crate) async fn blocked_list(cache_dir: Option<&Path>) -> Option<Vec<Blocked>> {
    load(cache_dir, false)
        .await
        .map(|loaded| loaded.index.blocked.clone())
}

/// Whether the index removed this pack. Without a configured index, or when
/// it cannot be read and nothing is cached, nothing is blocked: import-side
/// review, quarantine and authority limits still apply.
pub(crate) async fn blocked_reason(
    cache_dir: Option<&Path>,
    pack_id: &str,
    repo: Option<&str>,
) -> Option<&'static str> {
    let list = blocked_list(cache_dir).await?;
    find(&list, pack_id, repo).map(reason)
}

fn preview_file(cache_dir: &Path, entry: &Entry) -> PathBuf {
    let place = pack::sha256_hex(
        format!(
            "{}/{}",
            entry.full_name(),
            entry.path.as_deref().unwrap_or("")
        )
        .as_bytes(),
    );
    cache_dir
        .join("previews")
        .join(format!("{}-{}.png", entry.commit, &place[..12]))
}

fn valid_png(bytes: &[u8]) -> bool {
    bytes.len() <= pack::MAX_PREVIEW && bytes.starts_with(&PNG_SIGNATURE)
}

/// Keeps the most recently used images within `cap` bytes.
fn evict(directory: &Path, keep: &Path, cap: u64) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            metadata.is_file().then(|| {
                (
                    metadata.modified().unwrap_or(UNIX_EPOCH),
                    metadata.len(),
                    entry.path(),
                )
            })
        })
        .collect();
    files.sort();
    let mut total: u64 = files.iter().map(|(_, size, _)| size).sum();
    for (_, size, path) in files {
        if total <= cap {
            break;
        }
        if path != keep && std::fs::remove_file(&path).is_ok() {
            total -= size;
        }
    }
}

/// A listed pack's Map image (PNG, ≤ 1 MiB, signature checked), cached by commit.
pub(crate) async fn preview_from(
    raw_base: &str,
    cache_dir: &Path,
    entry: &Entry,
) -> Option<Vec<u8>> {
    if !entry.has_preview {
        return None;
    }
    let file = preview_file(cache_dir, entry);
    if let Ok(bytes) = std::fs::read(&file) {
        if valid_png(&bytes) {
            if let Ok(handle) = std::fs::File::options().write(true).open(&file) {
                let _ = handle.set_modified(SystemTime::now());
            }
            return Some(bytes);
        }
        let _ = std::fs::remove_file(&file);
    }
    let folder = entry
        .path
        .as_deref()
        .map(|path| format!("{path}/"))
        .unwrap_or_default();
    let url = format!(
        "{}/{}/{}/{folder}preview.png",
        raw_base.trim_end_matches('/'),
        entry.full_name(),
        entry.commit
    );
    let bytes = get(&url, pack::MAX_PREVIEW)
        .await
        .filter(|bytes| valid_png(bytes))?;
    if write_atomic(&file, &bytes).is_ok() {
        if let Some(directory) = file.parent() {
            evict(directory, &file, MAX_PREVIEW_CACHE);
        }
    }
    Some(bytes)
}

/// The index repository (`owner`, `repo`) behind the configured index URL:
/// `raw.githubusercontent.com/{owner}/{repo}/…` or `github.com/{owner}/{repo}/…`.
pub(crate) fn index_repo(index_url: &str) -> Option<(String, String)> {
    let index = reqwest::Url::parse(index_url).ok()?;
    let parts: Vec<&str> = index.path().trim_start_matches('/').split('/').collect();
    let (owner, repo) = match (index.host_str()?, parts.as_slice()) {
        ("raw.githubusercontent.com" | "github.com", [owner, repo, ..]) => (*owner, *repo),
        _ => return None,
    };
    (super::github::valid_login(owner) && super::github::valid_repo_name(repo))
        .then(|| (owner.to_owned(), repo.to_owned()))
}

/// The community repository's report form with only the public pack URL filled in.
pub(crate) fn report_url(index_url: &str, pack_url: &str) -> Option<String> {
    let (owner, repo) = index_repo(index_url)?;
    let mut url =
        reqwest::Url::parse(&format!("https://github.com/{owner}/{repo}/issues/new")).ok()?;
    url.query_pairs_mut()
        .append_pair("template", "report.yml")
        .append_pair("pack", pack_url);
    Some(url.to_string())
}

/// A listing request's entry file: `entries/<owner>__<repo>.json` holding
/// `{repo, path, commit}` only; the index's CI reads everything else from the
/// pack at that commit, so an entry cannot claim what the pack does not hold.
pub(crate) fn listing_entry(repo: &str, path: &str, commit: &str) -> Option<(String, Vec<u8>)> {
    let (owner, name) = repo.split_once('/')?;
    if !super::github::valid_login(owner)
        || !super::github::valid_repo_name(name)
        || commit.len() != 40
        || !commit.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    let body = serde_json::json!({"repo": repo, "path": path, "commit": commit});
    let mut bytes = serde_json::to_vec_pretty(&body).ok()?;
    bytes.push(b'\n');
    Some((format!("entries/{owner}__{name}.json"), bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "7f1d1c3e-0000-4000-8000-000000000001";
    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

    fn listed(id: &str, repo: &str, extra: serde_json::Value) -> serde_json::Value {
        let mut entry = serde_json::json!({
            "id": id, "repo": repo, "path": "", "commit": SHA, "name": "Semiconductor supply chain",
            "focus": "Fabs, equipment makers and export rules.", "author": "Kim J.", "language": "en",
            "license": "CC-BY-4.0", "assetClasses": ["stocks", "etfs"],
            "counts": {"perspective": 1, "methods": 14, "principles": 6, "entities": 312, "claims": 1204, "sources": 88},
            "stars": 120, "forks": 34, "updatedAt": "2026-10-29T09:00:00Z", "preview": true
        });
        for (key, value) in extra.as_object().unwrap() {
            entry[key] = value.clone();
        }
        entry
    }

    #[test]
    fn the_blocked_list_matches_by_pack_id_or_repository() {
        let blocked = parse(
            br#"{"format":"loupist-index/1","generatedAt":"2026-11-01T00:00:00Z","entries":[],
                 "blocked":[{"id":"7f1d1c3e-0000-4000-8000-000000000001","repo":"kimj/loupist-bad","reason":"reported","at":"2026-10-30"}]}"#,
        )
        .unwrap()
        .blocked;
        assert_eq!(find(&blocked, ID, None).map(reason), Some("reported"));
        assert_eq!(
            find(&blocked, "other", Some("KimJ/Loupist-Bad")).map(reason),
            Some("reported")
        );
        assert!(find(&blocked, "other", Some("kimj/fine")).is_none());
        assert!(parse(br#"{"format":"other","blocked":[]}"#).is_none());
    }

    #[test]
    fn entries_validate_one_by_one_and_blocked_ones_never_appear() {
        let index = serde_json::json!({
            "format": "loupist-index/1", "generatedAt": "2026-11-01T00:00:00Z",
            "entries": [
                listed(ID, "kimj/loupist-chips", serde_json::json!({"name": "Semi\u{202e}conductor   supply chain"})),
                listed("7f1d1c3e-0000-4000-8000-000000000002", "kimj/loupist-bad", serde_json::json!({})),
                listed("7f1d1c3e-0000-4000-8000-000000000003", "../evil", serde_json::json!({})),
                listed("7f1d1c3e-0000-4000-8000-000000000004", "kimj/x", serde_json::json!({"commit": "main"})),
                listed("7f1d1c3e-0000-4000-8000-000000000005", "kimj/y", serde_json::json!({"assetClasses": ["options"]})),
                listed("7f1d1c3e-0000-4000-8000-000000000006", "kimj/z", serde_json::json!({"path": "packs/../../x"})),
                listed("7f1d1c3e-0000-4000-8000-000000000007", "hanul/desk", serde_json::json!({"path": "packs/desk/", "language": "ko", "preview": false})),
                {"id": "not-an-entry"},
                listed(ID, "kimj/duplicate", serde_json::json!({})),
            ],
            "blocked": [{"id": "7f1d1c3e-0000-4000-8000-000000000002", "repo": "kimj/loupist-bad", "reason": "unsafe"}]
        });
        let parsed = parse(index.to_string().as_bytes()).unwrap();
        let ids: Vec<&str> = parsed.entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, [ID, "7f1d1c3e-0000-4000-8000-000000000007"]);
        let chips = &parsed.entries[0];
        assert_eq!(
            chips.name, "Semiconductor supply chain",
            "hidden characters removed, spaces folded"
        );
        assert_eq!(chips.source, "github.com/kimj/loupist-chips");
        assert_eq!(
            chips.counts,
            EntryCounts {
                methods: 14,
                principles: 6,
                names: 312,
                links: 1204
            }
        );
        assert_eq!(parsed.entries[1].path.as_deref(), Some("packs/desk"));
        // The location stays in Rust: the renderer gets the id, never the repository path or commit.
        let view = serde_json::to_value(chips).unwrap();
        assert!(
            view.get("commit").is_none()
                && view.get("path").is_none()
                && view.get("owner").is_none()
        );
        assert_eq!(view["hasPreview"], true);
        // A document that is too large or of another format is rejected whole.
        assert!(parse(&vec![b' '; MAX_INDEX_BYTES + 1]).is_none());
    }

    #[test]
    fn reports_carry_only_the_public_pack_link() {
        let url = report_url(
            "https://raw.githubusercontent.com/loupist/community/main/index.json",
            "https://github.com/kimj/loupist-desk",
        )
        .unwrap();
        assert_eq!(
            url,
            "https://github.com/loupist/community/issues/new?template=report.yml&pack=https%3A%2F%2Fgithub.com%2Fkimj%2Floupist-desk"
        );
        assert!(report_url("https://example.com/index.json", "https://github.com/a/b").is_none());
    }

    fn index_with(entries: Vec<serde_json::Value>, blocked: serde_json::Value) -> String {
        serde_json::json!({"format": "loupist-index/1", "generatedAt": "2026-11-01T00:00:00Z", "entries": entries, "blocked": blocked}).to_string()
    }

    #[tokio::test]
    async fn the_index_is_kept_for_offline_use_and_read_again_only_when_stale_or_refreshed() {
        use super::super::github::fixture;
        use std::sync::{
            atomic::{AtomicU16, Ordering},
            Arc,
        };
        let cache = tempfile::tempdir().unwrap();
        let status = Arc::new(AtomicU16::new(200));
        let answer = status.clone();
        let body = index_with(
            vec![listed(ID, "kimj/loupist-chips", serde_json::json!({}))],
            serde_json::json!([]),
        );
        let server = fixture::serve(Arc::new(move |_, path, _| match path {
            "/index.json" => (answer.load(Ordering::SeqCst), body.clone()),
            "/moved" => (302, String::new()),
            _ => (404, "{}".into()),
        }))
        .await;
        let url = format!("{}/index.json", server.base);
        let fresh = load_from(&url, Some(cache.path()), false).await.unwrap();
        assert!(!fresh.offline);
        assert_eq!(fresh.index.entries[0].id, ID);
        assert!(
            cache.path().join("index.json").is_file(),
            "kept in app data"
        );
        // Within six hours the copy in memory answers; nothing is fetched.
        load_from(&url, Some(cache.path()), false).await.unwrap();
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert_eq!(cached_entry(ID).unwrap().full_name(), "kimj/loupist-chips");
        // Refresh while the index fails: the kept list, marked offline.
        status.store(503, Ordering::SeqCst);
        let offline = load_from(&url, Some(cache.path()), true).await.unwrap();
        assert!(offline.offline);
        assert_eq!(offline.fetched_at_ms, fresh.fetched_at_ms);
        // A later start with nothing in memory still has the copy in app data.
        let restarted = format!("{}/index.json?restart", server.base);
        let kept = load_from(&restarted, Some(cache.path()), true)
            .await
            .unwrap();
        assert!(kept.offline && kept.index.entries.len() == 1);
        // Never read: unavailable. Redirects are not followed.
        let empty = tempfile::tempdir().unwrap();
        assert!(
            load_from(&format!("{}/moved", server.base), Some(empty.path()), true)
                .await
                .is_none()
        );
    }

    #[tokio::test]
    async fn map_images_are_pngs_within_a_megabyte_cached_by_commit() {
        use super::super::github::fixture;
        use std::sync::Arc;
        let cache = tempfile::tempdir().unwrap();
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend_from_slice(b"fixture image");
        let image = png.clone();
        let server = fixture::serve_bytes(Arc::new(move |_, path, _| match path {
            p if p == format!("/kimj/loupist-chips/{SHA}/preview.png") => (200, image.clone()),
            p if p == format!("/hanul/desk/{SHA}/packs/desk/preview.png") => {
                (200, b"<html>not a png</html>".to_vec())
            }
            p if p == format!("/kimj/huge/{SHA}/preview.png") => {
                let mut huge = PNG_SIGNATURE.to_vec();
                huge.resize(pack::MAX_PREVIEW + 1, 0);
                (200, huge)
            }
            _ => (404, Vec::new()),
        }))
        .await;
        let index = parse(
            index_with(
                vec![
                    listed(ID, "kimj/loupist-chips", serde_json::json!({})),
                    listed(
                        "7f1d1c3e-0000-4000-8000-000000000007",
                        "hanul/desk",
                        serde_json::json!({"path": "packs/desk"}),
                    ),
                    listed(
                        "7f1d1c3e-0000-4000-8000-000000000008",
                        "kimj/huge",
                        serde_json::json!({}),
                    ),
                    listed(
                        "7f1d1c3e-0000-4000-8000-000000000009",
                        "kimj/plain",
                        serde_json::json!({"preview": false}),
                    ),
                ],
                serde_json::json!([]),
            )
            .as_bytes(),
        )
        .unwrap();
        let [chips, desk, huge, plain] = &index.entries[..] else {
            panic!("entries")
        };
        assert_eq!(
            preview_from(&server.base, cache.path(), chips).await,
            Some(png.clone())
        );
        assert_eq!(
            preview_from(&server.base, cache.path(), chips).await,
            Some(png)
        );
        assert_eq!(
            server.requests.lock().unwrap().len(),
            1,
            "the second read is the cached copy"
        );
        assert!(
            preview_from(&server.base, cache.path(), desk)
                .await
                .is_none(),
            "signature checked"
        );
        assert!(
            preview_from(&server.base, cache.path(), huge)
                .await
                .is_none(),
            "1 MiB at most"
        );
        assert!(preview_from(&server.base, cache.path(), plain)
            .await
            .is_none());
        assert_eq!(
            server.requests.lock().unwrap().len(),
            3,
            "no request without a preview"
        );
    }

    #[test]
    fn the_image_cache_drops_the_least_recently_used_first() {
        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("old.png");
        let newer = directory.path().join("newer.png");
        let kept = directory.path().join("kept.png");
        for (path, age) in [(&old, 300), (&newer, 200), (&kept, 100)] {
            std::fs::write(path, vec![0u8; 100]).unwrap();
            let file = std::fs::File::options().write(true).open(path).unwrap();
            file.set_modified(SystemTime::now() - Duration::from_secs(age))
                .unwrap();
        }
        evict(directory.path(), &kept, 250);
        assert!(!old.exists() && newer.exists() && kept.exists());
        // The image just written stays even when it is the oldest.
        evict(directory.path(), &newer, 100);
        assert!(newer.exists() && !kept.exists());
    }

    #[test]
    fn a_listing_entry_names_the_repository_path_and_commit_only() {
        let (path, bytes) = listing_entry("kimj/loupist-desk", "", SHA).unwrap();
        assert_eq!(path, "entries/kimj__loupist-desk.json");
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"repo": "kimj/loupist-desk", "path": "", "commit": SHA})
        );
        assert!(listing_entry("kimj/../x", "", SHA).is_none());
        assert!(listing_entry("kimj/desk", "", "main").is_none());
    }
}
