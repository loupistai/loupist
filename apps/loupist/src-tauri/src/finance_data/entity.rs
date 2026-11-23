//! OpenDART's company directory (`corpCode.xml`): one archive of every filer,
//! listed or not, with its corp_code, Korean and English names and stock code.
//! It is downloaded once in its own task with its own time limit, kept on disk,
//! refreshed in the background, and indexed in memory, so every DART tool
//! resolves a company the same way and a call that has a copy never waits.
use super::*;
use std::{
    collections::{BTreeSet, HashMap},
    io::Write,
};
use tokio::sync::watch;

const OPENDART_CORP_CODE_PATH: &[&str] = &["api", "corpCode.xml"];
/// A copy this old is refreshed in the background; calls keep using it.
const DIRECTORY_REFRESH_AGE: ChronoDuration = ChronoDuration::hours(24);
/// A saved copy older than this is not used.
const DIRECTORY_MAX_AGE: ChronoDuration = ChronoDuration::days(30);
/// The archive's own request limit. On 2026-11-23 it was a 3.6 MB ZIP (30 MB
/// of XML, 120,054 filers) that arrived in 3 to 6 seconds; the 20-second limit
/// of ordinary reads is too tight for it on a slow link.
const DIRECTORY_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(90);
/// How long a call waits for a first download before it reports that the
/// company list is still being prepared. The download carries on.
const DIRECTORY_FIRST_USE_WAIT: Duration = Duration::from_secs(25);
const DIRECTORY_FILE: &str = "corp-codes.zip";
/// Corporate-form words at the end of a name; they tell no two filers apart.
const NAME_SUFFIXES: [&str; 9] = [
    "CO",
    "LTD",
    "INC",
    "CORP",
    "CORPORATION",
    "COMPANY",
    "LIMITED",
    "PLC",
    "LLC",
];
/// Korean corporate-form markers, anywhere in a name.
const KOREAN_FORM_MARKERS: [&str; 3] = ["주식회사", "(주)", "㈜"];

#[derive(Clone, Debug)]
pub(super) struct EntityRecord {
    pub name: String,
    pub english_name: Option<String>,
    pub symbol: Option<String>,
    pub cik: Option<String>,
    pub corp_code: Option<String>,
    pub currency: Option<String>,
    pub provider: &'static str,
}

/// The directory indexed by corp_code, stock code and normalized names.
pub(super) struct DartDirectory {
    entries: Vec<EntityRecord>,
    name_keys: Vec<String>,
    english_keys: Vec<String>,
    by_corp_code: HashMap<String, usize>,
    by_stock_code: HashMap<String, usize>,
    by_name: HashMap<String, Vec<usize>>,
    fetched_at: DateTime<Utc>,
}

impl DartDirectory {
    pub(super) fn new(
        entries: Vec<EntityRecord>,
        fetched_at: DateTime<Utc>,
    ) -> Result<Self, FinanceDataError> {
        // An archive with no filers is a refusal or a broken reply, never a
        // directory to keep: it would answer "no company" for a day.
        if entries.is_empty() {
            return Err(FinanceDataError::InvalidResponse);
        }
        let mut by_corp_code = HashMap::with_capacity(entries.len());
        let mut by_stock_code = HashMap::new();
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::with_capacity(entries.len());
        let mut name_keys = Vec::with_capacity(entries.len());
        let mut english_keys = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            if let Some(code) = &entry.corp_code {
                by_corp_code.entry(code.clone()).or_insert(index);
            }
            if let Some(symbol) = &entry.symbol {
                by_stock_code
                    .entry(symbol.to_ascii_uppercase())
                    .or_insert(index);
            }
            let name = normalize_name(&entry.name);
            let english = entry
                .english_name
                .as_deref()
                .map(normalize_name)
                .unwrap_or_default();
            for key in [&name, &english] {
                if key.is_empty() {
                    continue;
                }
                let indexes = by_name.entry(key.clone()).or_default();
                if indexes.last() != Some(&index) {
                    indexes.push(index);
                }
            }
            name_keys.push(name);
            english_keys.push(english);
        }
        Ok(Self {
            entries,
            name_keys,
            english_keys,
            by_corp_code,
            by_stock_code,
            by_name,
            fetched_at,
        })
    }

    fn stale(&self, now: DateTime<Utc>) -> bool {
        now - self.fetched_at > DIRECTORY_REFRESH_AGE
    }

    fn corp_code(&self, index: usize) -> Option<String> {
        self.entries[index].corp_code.clone()
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum MatchGrade {
    CorpCodeExact,
    TickerExact,
    NameExact,
    NameContains,
}

impl MatchGrade {
    fn as_str(self) -> &'static str {
        match self {
            Self::CorpCodeExact => "corp_code_exact",
            Self::TickerExact => "ticker_exact",
            Self::NameExact => "name_exact",
            Self::NameContains => "name_contains",
        }
    }
}

/// How a tool's `company` argument names a filer.
#[derive(Debug)]
pub(crate) enum DartResolution {
    Found(String),
    /// No single filer: the closest candidates, best first (none matched when empty).
    Candidates(Vec<Value>),
}

pub(super) fn is_corp_code(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_digit())
}

/// KRX short codes are six characters starting with a digit; newer listings
/// may carry a letter (for example 0126Z0).
fn is_stock_code(value: &str) -> bool {
    value.len() == 6
        && value.as_bytes()[0].is_ascii_digit()
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// One key for a Korean or English name: corporate-form words dropped, letters
/// uppercased, spaces and punctuation removed. "SAMSUNG ELECTRONICS CO,.LTD",
/// "Samsung Electronics" and "삼성전자(주)"-style spellings meet their registered names.
pub(super) fn normalize_name(value: &str) -> String {
    let mut text = value.to_owned();
    for marker in KOREAN_FORM_MARKERS {
        text = text.replace(marker, " ");
    }
    let tokens = text
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_uppercase)
        .collect::<Vec<_>>();
    let mut end = tokens.len();
    while end > 1 && NAME_SUFFIXES.contains(&tokens[end - 1].as_str()) {
        end -= 1;
    }
    tokens[..end].concat()
}

pub(super) fn parse_opendart_corp_code(
    bytes: &[u8],
) -> Result<Vec<EntityRecord>, FinanceDataError> {
    let xml = if bytes.starts_with(b"PK") {
        extract_corp_code_xml(bytes)?
    } else {
        String::from_utf8(bytes.to_vec()).map_err(|_| FinanceDataError::InvalidResponse)?
    };
    let mut entries = Vec::new();
    for block in xml.split("<list>").skip(1) {
        let Some(end) = block.find("</list>") else {
            continue;
        };
        let block = &block[..end];
        let corp_code = xml_tag(block, "corp_code");
        let name = xml_tag(block, "corp_name");
        let english_name = xml_tag(block, "corp_eng_name");
        let stock_code = xml_tag(block, "stock_code");
        if !is_corp_code(&corp_code) || name.is_empty() {
            continue;
        }
        entries.push(EntityRecord {
            name,
            english_name: (!english_name.is_empty()).then_some(english_name),
            symbol: (!stock_code.is_empty()).then_some(stock_code),
            cik: None,
            corp_code: Some(corp_code),
            currency: Some("KRW".into()),
            provider: OPENDART_SOURCE_ID,
        });
    }
    Ok(entries)
}

fn extract_corp_code_xml(bytes: &[u8]) -> Result<String, FinanceDataError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| FinanceDataError::InvalidResponse)?;
    if archive.len() > 20_000 {
        return Err(FinanceDataError::InvalidResponse);
    }
    archive
        .validate_header_consistency()
        .map_err(|_| FinanceDataError::InvalidResponse)?;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|_| FinanceDataError::InvalidResponse)?;
        if !file
            .name()
            .map_err(|_| FinanceDataError::InvalidResponse)?
            .to_ascii_lowercase()
            .ends_with(".xml")
        {
            continue;
        }
        if file.size() > MAX_DECOMPRESSED_DOCUMENT_BYTES as u64 {
            return Err(FinanceDataError::InvalidResponse);
        }
        let mut xml = String::new();
        (&mut file)
            .take(MAX_DECOMPRESSED_DOCUMENT_BYTES as u64 + 1)
            .read_to_string(&mut xml)
            .map_err(|_| FinanceDataError::InvalidResponse)?;
        if xml.len() > MAX_DECOMPRESSED_DOCUMENT_BYTES {
            return Err(FinanceDataError::InvalidResponse);
        }
        return Ok(xml);
    }
    Err(FinanceDataError::InvalidResponse)
}

fn xml_tag(block: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    block
        .split_once(&open)
        .and_then(|(_, rest)| rest.split_once(&close))
        .map(|(value, _)| value.trim().to_owned())
        .unwrap_or_default()
}

/// Candidates for a lookup: an exact corp_code, then an exact ticker, then
/// exact names, then names containing the query; listed filers first within a
/// grade, then shorter names.
pub(super) fn match_entities(query: &str, directory: &DartDirectory, limit: usize) -> Vec<Value> {
    let raw = query.trim();
    let needle = normalize_name(raw);
    let mut seen = BTreeSet::new();
    let mut picked = Vec::new();
    let mut add = |grade: MatchGrade, index: usize| {
        if seen.insert(index) {
            picked.push((grade, index));
        }
    };
    if let Some(&index) = directory.by_corp_code.get(raw) {
        add(MatchGrade::CorpCodeExact, index);
    }
    if is_stock_code(raw) {
        if let Some(&index) = directory.by_stock_code.get(&raw.to_ascii_uppercase()) {
            add(MatchGrade::TickerExact, index);
        }
    }
    if !needle.is_empty() {
        for &index in directory.by_name.get(&needle).into_iter().flatten() {
            add(MatchGrade::NameExact, index);
        }
        for index in 0..directory.entries.len() {
            if directory.name_keys[index].contains(&needle)
                || directory.english_keys[index].contains(&needle)
                || directory.entries[index]
                    .symbol
                    .as_deref()
                    .is_some_and(|symbol| symbol.contains(&needle))
            {
                add(MatchGrade::NameContains, index);
            }
        }
    }
    let entries = &directory.entries;
    picked.sort_by(|(left_grade, left), (right_grade, right)| {
        let (left, right) = (&entries[*left], &entries[*right]);
        left_grade
            .cmp(right_grade)
            .then(left.symbol.is_none().cmp(&right.symbol.is_none()))
            .then(left.name.chars().count().cmp(&right.name.chars().count()))
            .then(left.name.cmp(&right.name))
    });
    picked.truncate(limit);
    picked
        .into_iter()
        .map(|(grade, index)| {
            let entry = &entries[index];
            json!({
                "name": entry.name,
                "english_name": entry.english_name,
                "symbol": entry.symbol,
                "cik": entry.cik,
                "corp_code": entry.corp_code,
                "exchange": Value::Null,
                "currency": entry.currency,
                "provider": entry.provider,
                "match_grade": grade.as_str(),
                "usage": "discovery"
            })
        })
        .collect()
}

/// A tool's `company`: a corp_code passes, a ticker is exact, and an exact name
/// resolves when one filer carries it, or one listed filer among several.
pub(super) fn resolve_company(directory: &DartDirectory, company: &str) -> DartResolution {
    let raw = company.trim();
    if is_corp_code(raw) {
        return DartResolution::Found(raw.to_owned());
    }
    if is_stock_code(raw) {
        if let Some(code) = directory
            .by_stock_code
            .get(&raw.to_ascii_uppercase())
            .and_then(|&index| directory.corp_code(index))
        {
            return DartResolution::Found(code);
        }
    }
    if let Some(indexes) = directory.by_name.get(&normalize_name(raw)) {
        let listed = indexes
            .iter()
            .copied()
            .filter(|&index| directory.entries[index].symbol.is_some())
            .collect::<Vec<_>>();
        let only = match (indexes.as_slice(), listed.as_slice()) {
            ([only], _) | (_, [only]) => Some(*only),
            _ => None,
        };
        if let Some(code) = only.and_then(|index| directory.corp_code(index)) {
            return DartResolution::Found(code);
        }
    }
    DartResolution::Candidates(match_entities(raw, directory, 8))
}

/// The saved archive under the app's cache directory.
pub(super) struct DartDirectoryStore {
    path: PathBuf,
}

impl DartDirectoryStore {
    pub(super) fn open(directory: PathBuf) -> Option<Self> {
        let parent = directory.parent()?;
        crate::fs::artifact_trust::ensure_private_directory(parent).ok()?;
        crate::fs::artifact_trust::ensure_private_directory(&directory).ok()?;
        Some(Self {
            path: directory.join(DIRECTORY_FILE),
        })
    }

    /// The saved archive and when it was saved, unless missing, oversized or too old.
    fn load(&self, now: DateTime<Utc>) -> Option<(Vec<u8>, DateTime<Utc>)> {
        let metadata = std::fs::symlink_metadata(&self.path).ok()?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_LARGE_PROVIDER_BYTES as u64 {
            return None;
        }
        let saved_at = DateTime::<Utc>::from(metadata.modified().ok()?).min(now);
        if now - saved_at > DIRECTORY_MAX_AGE {
            return None;
        }
        Some((std::fs::read(&self.path).ok()?, saved_at))
    }

    fn store(&self, bytes: &[u8]) -> std::io::Result<()> {
        let directory = self
            .path
            .parent()
            .ok_or_else(|| std::io::Error::other("cache path has no directory"))?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        temporary.write_all(bytes)?;
        temporary.persist(&self.path).map_err(|error| error.error)?;
        Ok(())
    }
}

#[derive(Clone)]
enum DirectoryLoad {
    Loading,
    Ready,
    Failed(Arc<FinanceDataError>),
}

#[derive(Default)]
pub(super) struct EntityDirectoryCache {
    dart: Option<Arc<DartDirectory>>,
    /// The download in flight; every caller without a copy waits on this one.
    loading: Option<watch::Receiver<DirectoryLoad>>,
    saved_copy_checked: bool,
}

/// Directory timing and, in tests, the archive source.
#[derive(Clone)]
pub(super) struct DartDirectorySettings {
    pub(super) store: Option<Arc<DartDirectoryStore>>,
    pub(super) first_use_wait: Duration,
    #[cfg(test)]
    pub(super) fetch: Option<ArchiveFetch>,
}

#[cfg(test)]
pub(super) type ArchiveFetch = Arc<
    dyn Fn() -> futures_util::future::BoxFuture<'static, Result<Vec<u8>, FinanceDataError>>
        + Send
        + Sync,
>;

impl Default for DartDirectorySettings {
    fn default() -> Self {
        Self {
            store: None,
            first_use_wait: DIRECTORY_FIRST_USE_WAIT,
            #[cfg(test)]
            fetch: None,
        }
    }
}

impl FinanceDataService {
    /// Keeps OpenDART's company directory under `directory` between launches.
    pub fn with_dart_directory_cache(mut self, directory: PathBuf) -> Self {
        self.dart_directory.store = DartDirectoryStore::open(directory).map(Arc::new);
        self
    }

    /// Candidates for `dart_company_lookup`.
    pub async fn resolve_entity(
        &self,
        query: &str,
        limit: usize,
        dart_key: &str,
    ) -> Result<Value, FinanceDataError> {
        let directory = self.dart_company_directory(dart_key).await?;
        let matches = match_entities(query, &directory, limit);
        Ok(json!({
            "kind": "finance_discovery",
            "usage": "discovery",
            "query": query,
            "matches": matches
        }))
    }

    /// The corp_code a DART tool's `company` names. A corp_code needs no directory.
    pub(crate) async fn resolve_dart_company(
        &self,
        company: &str,
        dart_key: &str,
    ) -> Result<DartResolution, FinanceDataError> {
        if is_corp_code(company.trim()) {
            return Ok(DartResolution::Found(company.trim().to_owned()));
        }
        let directory = self.dart_company_directory(dart_key).await?;
        Ok(resolve_company(&directory, company))
    }

    async fn dart_company_directory(
        &self,
        dart_key: &str,
    ) -> Result<Arc<DartDirectory>, FinanceDataError> {
        let mut receiver = {
            let mut cache = self.entity_directories.lock().await;
            if cache.dart.is_none() && !cache.saved_copy_checked {
                cache.saved_copy_checked = true;
                cache.dart = self.load_saved_dart_directory().await;
            }
            if let Some(directory) = cache.dart.clone() {
                if directory.stale(Utc::now()) && cache.loading.is_none() {
                    self.start_dart_directory_download(&mut cache, dart_key);
                }
                return Ok(directory);
            }
            match cache.loading.clone() {
                Some(receiver) => receiver,
                None => self.start_dart_directory_download(&mut cache, dart_key),
            }
        };
        // The watch guard is released before the next await.
        let failure = {
            let settled = tokio::time::timeout(
                self.dart_directory.first_use_wait,
                receiver.wait_for(|load| !matches!(load, DirectoryLoad::Loading)),
            )
            .await;
            match settled {
                Err(_) => return Err(FinanceDataError::DirectoryPreparing),
                Ok(Err(_)) => return Err(FinanceDataError::InvalidResponse),
                Ok(Ok(load)) => match &*load {
                    DirectoryLoad::Failed(error) => Some(error.duplicate()),
                    DirectoryLoad::Loading | DirectoryLoad::Ready => None,
                },
            }
        };
        if let Some(error) = failure {
            return Err(error);
        }
        self.entity_directories
            .lock()
            .await
            .dart
            .clone()
            .ok_or(FinanceDataError::InvalidResponse)
    }

    /// One download for every caller, in its own task so it finishes even
    /// when the call that started it has stopped waiting.
    fn start_dart_directory_download(
        &self,
        cache: &mut EntityDirectoryCache,
        dart_key: &str,
    ) -> watch::Receiver<DirectoryLoad> {
        let (sender, receiver) = watch::channel(DirectoryLoad::Loading);
        cache.loading = Some(receiver.clone());
        let service = self.clone();
        let dart_key = dart_key.to_owned();
        tokio::spawn(async move {
            let outcome = service.download_dart_directory(&dart_key).await;
            drop(dart_key);
            let load = {
                let mut cache = service.entity_directories.lock().await;
                cache.loading = None;
                match outcome {
                    Ok(directory) => {
                        cache.dart = Some(directory);
                        DirectoryLoad::Ready
                    }
                    // A failed refresh keeps the copy calls already use.
                    Err(error) => DirectoryLoad::Failed(Arc::new(error)),
                }
            };
            let _ = sender.send(load);
        });
        receiver
    }

    async fn download_dart_directory(
        &self,
        dart_key: &str,
    ) -> Result<Arc<DartDirectory>, FinanceDataError> {
        let bytes = self.fetch_dart_directory_archive(dart_key).await?;
        let store = self.saved_dart_directory_store();
        let directory = tokio::task::spawn_blocking(move || {
            let directory = DartDirectory::new(parse_opendart_corp_code(&bytes)?, Utc::now())?;
            if let Some(store) = store {
                // A copy that cannot be saved is downloaded again next launch.
                let _ = store.store(&bytes);
            }
            Ok::<_, FinanceDataError>(directory)
        })
        .await
        .map_err(|_| FinanceDataError::InvalidResponse)??;
        Ok(Arc::new(directory))
    }

    async fn fetch_dart_directory_archive(
        &self,
        dart_key: &str,
    ) -> Result<Vec<u8>, FinanceDataError> {
        let bytes = self.request_dart_directory_archive(dart_key).await?;
        // A refused key or a limit comes back as a status document, not a ZIP.
        validate_dart_document_status(&bytes)?;
        Ok(bytes)
    }

    async fn request_dart_directory_archive(
        &self,
        dart_key: &str,
    ) -> Result<Vec<u8>, FinanceDataError> {
        #[cfg(test)]
        if let Some(fetch) = &self.dart_directory.fetch {
            return fetch().await;
        }
        validate_api_key(dart_key)?;
        let public_url = fixed_url(OPENDART_API_ROOT, OPENDART_CORP_CODE_PATH)?;
        let mut request_url = public_url.clone();
        request_url
            .query_pairs_mut()
            .append_pair("crtfc_key", dart_key);
        let raw = self
            .fetch_provider(
                self.dart_client
                    .get(request_url)
                    .timeout(DIRECTORY_DOWNLOAD_TIMEOUT),
                OPENDART_SOURCE_ID,
                public_url.to_string(),
                MAX_LARGE_PROVIDER_BYTES,
            )
            .await?;
        Ok(raw.bytes)
    }

    async fn load_saved_dart_directory(&self) -> Option<Arc<DartDirectory>> {
        let store = self.saved_dart_directory_store()?;
        tokio::task::spawn_blocking(move || {
            let (bytes, saved_at) = store.load(Utc::now())?;
            let entries = parse_opendart_corp_code(&bytes).ok()?;
            DartDirectory::new(entries, saved_at).ok().map(Arc::new)
        })
        .await
        .ok()
        .flatten()
    }

    /// Evaluation replays serve their own fictional directory and never touch disk.
    fn saved_dart_directory_store(&self) -> Option<Arc<DartDirectoryStore>> {
        #[cfg(feature = "eval-harness")]
        if crate::eval_harness::replay::enabled() {
            return None;
        }
        self.dart_directory.store.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Recorded corpCode.xml entries (2026-11-23), including the unlisted
    /// filers' single-space stock code and DART's own English spellings.
    const RECORDED_DIRECTORY: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<result>
    <list>
        <corp_code>00434003</corp_code>
        <corp_name>다코</corp_name>
        <corp_eng_name>Daco corporation</corp_eng_name>
        <stock_code> </stock_code>
        <modify_date>20170630</modify_date>
    </list>
    <list>
        <corp_code>00126380</corp_code>
        <corp_name>삼성전자</corp_name>
        <corp_eng_name>SAMSUNG ELECTRONICS CO,.LTD</corp_eng_name>
        <stock_code>005930</stock_code>
        <modify_date>20251201</modify_date>
    </list>
    <list>
        <corp_code>00252135</corp_code>
        <corp_name>주성엔지니어링</corp_name>
        <corp_eng_name>JUSUNG ENGINEERING Co.,Ltd.</corp_eng_name>
        <stock_code>036930</stock_code>
        <modify_date>20260820</modify_date>
    </list>
    <list>
        <corp_code>00120021</corp_code>
        <corp_name>LG</corp_name>
        <corp_eng_name>LG Corp.</corp_eng_name>
        <stock_code>003550</stock_code>
        <modify_date>20221220</modify_date>
    </list>
    <list>
        <corp_code>99000001</corp_code>
        <corp_name>주성엔지니어링</corp_name>
        <corp_eng_name></corp_eng_name>
        <stock_code> </stock_code>
        <modify_date>20010101</modify_date>
    </list>
    <list>
        <corp_code>99000002</corp_code>
        <corp_name>노을상사</corp_name>
        <corp_eng_name>NOEUL TRADING CO., LTD.</corp_eng_name>
        <stock_code> </stock_code>
        <modify_date>20200101</modify_date>
    </list>
    <list>
        <corp_code>99000003</corp_code>
        <corp_name>노을상사</corp_name>
        <corp_eng_name></corp_eng_name>
        <stock_code> </stock_code>
        <modify_date>20200101</modify_date>
    </list>
</result>"#;

    fn archive(contents: &[u8]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("CORPCODE.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(contents).unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn recorded() -> DartDirectory {
        DartDirectory::new(
            parse_opendart_corp_code(&archive(RECORDED_DIRECTORY.as_bytes())).unwrap(),
            Utc::now(),
        )
        .unwrap()
    }

    fn found(resolution: DartResolution) -> String {
        match resolution {
            DartResolution::Found(code) => code,
            DartResolution::Candidates(candidates) => panic!("not resolved: {candidates:?}"),
        }
    }

    #[test]
    fn corp_code_zip_rejects_oversized_xml_and_reads_an_ordinary_directory() {
        assert_eq!(
            extract_corp_code_xml(&archive(b"<result/>")).unwrap(),
            "<result/>"
        );
        let oversized = archive(&vec![b'x'; MAX_DECOMPRESSED_DOCUMENT_BYTES + 1]);
        assert!(matches!(
            extract_corp_code_xml(&oversized),
            Err(FinanceDataError::InvalidResponse)
        ));
        assert!(DartDirectory::new(
            parse_opendart_corp_code(&archive(b"<result></result>")).unwrap(),
            Utc::now()
        )
        .is_err());
    }

    #[test]
    fn stock_codes_corp_codes_and_korean_or_english_names_resolve_to_one_filer() {
        let directory = recorded();
        for company in [
            "005930",
            "00126380",
            "삼성전자",
            "삼성 전자",
            "삼성전자(주)",
            "Samsung Electronics",
            "SAMSUNG ELECTRONICS CO., LTD.",
        ] {
            assert_eq!(found(resolve_company(&directory, company)), "00126380");
        }
        // One listed filer among several with the name is the company meant.
        assert_eq!(
            found(resolve_company(&directory, "주성엔지니어링")),
            "00252135"
        );
        assert_eq!(
            found(resolve_company(&directory, "Jusung Engineering")),
            "00252135"
        );
        assert_eq!(found(resolve_company(&directory, "LG Corp")), "00120021");
        // A name two unlisted filers share stays ambiguous, with both codes.
        let DartResolution::Candidates(candidates) = resolve_company(&directory, "노을상사")
        else {
            panic!("a shared unlisted name must not resolve");
        };
        let codes = candidates
            .iter()
            .map(|candidate| candidate["corp_code"].as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(codes, BTreeSet::from(["99000002", "99000003"]));
        assert!(matches!(
            resolve_company(&directory, "999999"),
            DartResolution::Candidates(candidates) if candidates.is_empty()
        ));
        // A short corporate word never strips a name down to nothing.
        assert_eq!(normalize_name("Daco corporation"), "DACO");
        assert_eq!(normalize_name("Corporation"), "CORPORATION");
    }

    #[test]
    fn lookups_rank_exact_codes_and_listed_filers_first() {
        let directory = recorded();
        let by_corp_code = match_entities("00252135", &directory, 5);
        assert_eq!(by_corp_code[0]["corp_code"], "00252135");
        assert_eq!(by_corp_code[0]["match_grade"], "corp_code_exact");
        let by_ticker = match_entities("036930", &directory, 1);
        assert_eq!(by_ticker[0]["name"], "주성엔지니어링");
        assert_eq!(by_ticker[0]["match_grade"], "ticker_exact");
        let by_name = match_entities("주성", &directory, 5);
        assert_eq!(by_name.len(), 2);
        assert_eq!(by_name[0]["corp_code"], "00252135");
        assert_eq!(by_name[0]["match_grade"], "name_contains");
        assert!(by_name[1]["symbol"].is_null());
        let english = match_entities("samsung electronics", &directory, 5);
        assert_eq!(english[0]["match_grade"], "name_exact");
    }

    fn service(fetch: ArchiveFetch, wait: Duration) -> FinanceDataService {
        let mut service = FinanceDataService::new().unwrap();
        service.dart_directory.fetch = Some(fetch);
        service.dart_directory.first_use_wait = wait;
        service
    }

    #[tokio::test]
    async fn concurrent_first_uses_share_one_download_and_later_calls_use_its_copy() {
        let downloads = Arc::new(AtomicUsize::new(0));
        let counter = downloads.clone();
        let fetch: ArchiveFetch = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                Ok(archive(RECORDED_DIRECTORY.as_bytes()))
            })
        });
        let service = service(fetch, Duration::from_secs(5));
        let key = "fixture-dart-key-0000";
        let (first, second) = tokio::join!(
            service.resolve_dart_company("036930", key),
            service.resolve_dart_company("Samsung Electronics", key)
        );
        assert_eq!(found(first.unwrap()), "00252135");
        assert_eq!(found(second.unwrap()), "00126380");
        let lookup = service.resolve_entity("주성", 3, key).await.unwrap();
        assert_eq!(lookup["matches"][0]["corp_code"], "00252135");
        // A corp_code resolves without the directory at all.
        assert_eq!(
            found(service.resolve_dart_company("00252135", key).await.unwrap()),
            "00252135"
        );
        assert_eq!(downloads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_slow_first_download_reports_preparing_and_finishes_for_the_next_call() {
        let (release, gate) = tokio::sync::oneshot::channel::<()>();
        let gate = Arc::new(Mutex::new(Some(gate)));
        let fetch: ArchiveFetch = Arc::new(move || {
            let gate = gate.clone();
            Box::pin(async move {
                if let Some(gate) = gate.lock().await.take() {
                    let _ = gate.await;
                }
                Ok(archive(RECORDED_DIRECTORY.as_bytes()))
            })
        });
        let service = service(fetch, Duration::from_millis(50));
        assert!(matches!(
            service
                .resolve_dart_company("036930", "fixture-dart-key-0000")
                .await,
            Err(FinanceDataError::DirectoryPreparing)
        ));
        release.send(()).unwrap();
        let mut resolved = None;
        for _ in 0..100 {
            match service
                .resolve_dart_company("036930", "fixture-dart-key-0000")
                .await
            {
                Ok(resolution) => {
                    resolved = Some(found(resolution));
                    break;
                }
                Err(FinanceDataError::DirectoryPreparing) => {}
                Err(error) => panic!("{error}"),
            }
        }
        assert_eq!(resolved.as_deref(), Some("00252135"));
    }

    #[tokio::test]
    async fn a_refused_download_is_typed_and_not_kept_as_an_empty_directory() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let fetch: ArchiveFetch = Arc::new(move || {
            let attempt = counter.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if attempt == 0 {
                    // The status document OpenDART sends for an unregistered key.
                    Ok(r#"{"status":"010","message":"등록되지 않은 키입니다."}"#
                        .as_bytes()
                        .to_vec())
                } else {
                    Ok(archive(RECORDED_DIRECTORY.as_bytes()))
                }
            })
        });
        let service = service(fetch, Duration::from_secs(5));
        assert!(matches!(
            service
                .resolve_dart_company("036930", "fixture-dart-key-0000")
                .await,
            Err(FinanceDataError::DartKeyRefused("010"))
        ));
        assert_eq!(
            found(
                service
                    .resolve_dart_company("036930", "fixture-dart-key-0000")
                    .await
                    .unwrap()
            ),
            "00252135"
        );
    }

    #[tokio::test]
    async fn a_saved_copy_answers_without_a_download_and_an_old_one_is_refreshed_in_the_background()
    {
        let temporary = tempfile::tempdir().unwrap();
        let store = DartDirectoryStore::open(temporary.path().join("cache").join("opendart"))
            .map(Arc::new)
            .unwrap();
        store
            .store(&archive(RECORDED_DIRECTORY.as_bytes()))
            .unwrap();
        let downloads = Arc::new(AtomicUsize::new(0));
        let counter = downloads.clone();
        let fetch: ArchiveFetch = Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(archive(RECORDED_DIRECTORY.as_bytes())) })
        });
        let mut fresh = service(fetch.clone(), Duration::from_secs(5));
        fresh.dart_directory.store = Some(store.clone());
        assert_eq!(
            found(
                fresh
                    .resolve_dart_company("005930", "fixture-dart-key-0000")
                    .await
                    .unwrap()
            ),
            "00126380"
        );
        assert_eq!(downloads.load(Ordering::SeqCst), 0);

        // A copy past the refresh age still answers at once; the refresh runs behind it.
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 24 * 3600);
        std::fs::File::options()
            .write(true)
            .open(&store.path)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let mut aged = service(fetch, Duration::from_secs(5));
        aged.dart_directory.store = Some(store);
        assert_eq!(
            found(
                aged.resolve_dart_company("036930", "fixture-dart-key-0000")
                    .await
                    .unwrap()
            ),
            "00252135"
        );
        for _ in 0..200 {
            if downloads.load(Ordering::SeqCst) == 1
                && aged.entity_directories.lock().await.loading.is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert_eq!(downloads.load(Ordering::SeqCst), 1);
        assert!(!aged
            .entity_directories
            .lock()
            .await
            .dart
            .as_ref()
            .unwrap()
            .stale(Utc::now()));
    }
}
