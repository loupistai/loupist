use super::*;
use futures_util::{stream, StreamExt};
use serde::Serialize;
use std::{
    collections::BinaryHeap,
    io::{BufRead, BufReader},
};

const MASTER_ROOT: &str = "https://new.real.download.dws.co.kr/common/master";
const MASTER_DOCS: &str = "https://apiportal.koreainvestment.com/apiservice-category";

#[derive(Clone, Copy)]
enum Layout {
    Domestic(&'static str),
    Bond,
    DomesticDerivative,
    Commodity,
    OverseasEquity,
    OverseasDerivative,
}

#[derive(Clone, Copy)]
struct MasterFile {
    name: &'static str,
    market: &'static str,
    layout: Layout,
}

macro_rules! file {
    ($name:literal, $market:literal, $layout:expr) => {
        MasterFile {
            name: $name,
            market: $market,
            layout: $layout,
        }
    };
}

// File names and layout references from KIS Developers' master file page.
const MASTERS: &[MasterFile] = &[
    file!("kospi_code.mst", "kospi", Layout::Domestic("J")),
    file!("kosdaq_code.mst", "kosdaq", Layout::Domestic("J")),
    file!("konex_code.mst", "konex", Layout::Domestic("J")),
    file!("elw_code.mst", "elw", Layout::Domestic("W")),
    file!("bond_code.mst", "domestic_bond", Layout::Bond),
    file!(
        "fo_idx_code_mts.mst",
        "domestic_derivatives",
        Layout::DomesticDerivative
    ),
    file!(
        "fo_stk_code_mts.mst",
        "domestic_derivatives",
        Layout::DomesticDerivative
    ),
    file!("fo_com_code.mst", "domestic_derivatives", Layout::Commodity),
    file!("nasmst.cod", "overseas_equity", Layout::OverseasEquity),
    file!("nysmst.cod", "overseas_equity", Layout::OverseasEquity),
    file!("amsmst.cod", "overseas_equity", Layout::OverseasEquity),
    file!("shsmst.cod", "overseas_equity", Layout::OverseasEquity),
    file!("shimst.cod", "overseas_equity", Layout::OverseasEquity),
    file!("szsmst.cod", "overseas_equity", Layout::OverseasEquity),
    file!("szimst.cod", "overseas_equity", Layout::OverseasEquity),
    file!("hksmst.cod", "overseas_equity", Layout::OverseasEquity),
    file!("tsemst.cod", "overseas_equity", Layout::OverseasEquity),
    file!("hnxmst.cod", "overseas_equity", Layout::OverseasEquity),
    file!("hsxmst.cod", "overseas_equity", Layout::OverseasEquity),
    file!("ffcode.mst", "overseas_futures", Layout::OverseasDerivative),
    file!("focode.mst", "overseas_options", Layout::OverseasDerivative),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SymbolQuery {
    query: String,
    market: Option<String>,
    limit: Option<usize>,
}

#[derive(Eq, Ord, PartialEq, PartialOrd, Serialize)]
struct Symbol {
    market: &'static str,
    symbol: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    english_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    market_code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exchange_code: Option<String>,
}

impl FinanceDataService {
    pub(crate) async fn kis_symbol_lookup(&self, params: Value) -> Result<Value, FinanceDataError> {
        let query: SymbolQuery = strict_query(params, "kis_symbol_lookup")?;
        let term = normalize(&query.query);
        let limit = query.limit.unwrap_or(10);
        if term.is_empty()
            || query.query.len() > 384
            || query.query.chars().any(char::is_control)
            || !(1..=20).contains(&limit)
            || query
                .market
                .as_deref()
                .is_some_and(|market| !MASTERS.iter().any(|file| file.market == market))
        {
            return Err(FinanceDataError::InvalidQuery(
                "kis_symbol_lookup takes a name or symbol as query, at most 20 as limit, and market as one of the listed values",
            ));
        }
        let files = MASTERS.iter().copied().filter(|file| {
            query
                .market
                .as_deref()
                .is_none_or(|market| file.market == market)
        });
        let term = term.as_str();
        let mut reads = stream::iter(files)
            .map(|file| async move { (file.name, self.search_kis_master(file, term, limit).await) })
            .buffer_unordered(4);
        let mut matches = Vec::new();
        let mut failed_sources = Vec::new();
        let mut first_error = None;
        let mut matched = 0;
        let mut read_sources = 0;
        while let Some((name, result)) = reads.next().await {
            match result {
                Ok((hits, count)) => {
                    read_sources += 1;
                    matched += count;
                    matches.extend(hits);
                }
                Err(error) => {
                    failed_sources.push(name);
                    first_error.get_or_insert(error);
                }
            }
        }
        if let (0, Some(error)) = (read_sources, first_error) {
            return Err(error);
        }
        matches.sort_unstable();
        matches.dedup();
        failed_sources.sort_unstable();
        // The description says what match_type and the codes mean; the
        // warnings name what this search left out.
        let mut warnings = Vec::new();
        if matched > limit {
            warnings.push(format!(
                "{matched} entries matched and the best {limit} are shown: set market, or search the exact symbol or full name, to narrow it."
            ));
        }
        if !failed_sources.is_empty() {
            warnings.push(format!(
                "These KIS directories could not be read, so their entries are missing: {}. A symbol not found may be in one of them.",
                failed_sources.join(", ")
            ));
        }
        Ok(json!({
            "schema_version": "loupist-finance-result/1", "source_id": KIS_SOURCE_ID,
            "tool": "kis_symbol_lookup", "failed_sources": failed_sources, "warnings": warnings,
            "provenance": {"source_url": MASTER_DOCS, "retrieved_at": Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)},
            "quality": {"status": if failed_sources.is_empty() { "pass" } else { "warn" }},
            "match_count": matched, "truncated": matched > limit,
            "matches": matches.into_iter().take(limit).map(|(rank, symbol)| {
                let mut row = json!(symbol);
                row["match_type"] = json!(["symbol_exact", "name_exact", "name_prefix", "name_contains"][usize::from(rank)]);
                row
            }).collect::<Vec<_>>(),
        }))
    }

    async fn search_kis_master(
        &self,
        file: MasterFile,
        term: &str,
        limit: usize,
    ) -> Result<(Vec<(u8, Symbol)>, usize), FinanceDataError> {
        let url = format!("{MASTER_ROOT}/{}.zip", file.name);
        let raw = self
            .fetch_provider(
                self.client.get(&url),
                KIS_SOURCE_ID,
                url,
                MAX_KIS_RESPONSE_BYTES,
            )
            .await?;
        let term = term.to_owned();
        tokio::task::spawn_blocking(move || search_archive(file, &raw.bytes, &term, limit))
            .await
            .map_err(|_| FinanceDataError::InvalidResponse)?
    }
}

fn search_archive(
    file: MasterFile,
    bytes: &[u8],
    term: &str,
    limit: usize,
) -> Result<(Vec<(u8, Symbol)>, usize), FinanceDataError> {
    let invalid = |_| FinanceDataError::InvalidResponse;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(invalid)?;
    if archive.len() != 1 {
        return Err(FinanceDataError::InvalidResponse);
    }
    archive.validate_header_consistency().map_err(invalid)?;
    let entry = archive.by_index(0).map_err(invalid)?;
    let maximum = MAX_DECOMPRESSED_DOCUMENT_BYTES as u64;
    if !entry
        .name()
        .map_err(invalid)?
        .eq_ignore_ascii_case(file.name)
        || entry.size() > maximum
    {
        return Err(FinanceDataError::InvalidResponse);
    }
    let mut reader = BufReader::new(entry.take(maximum + 1));
    let mut line = Vec::new();
    let mut hits = BinaryHeap::new();
    let mut read = 0u64;
    let mut valid = 0;
    let mut count = 0;
    loop {
        line.clear();
        let bytes = reader
            .read_until(b'\n', &mut line)
            .map_err(|_| FinanceDataError::InvalidResponse)?;
        if bytes == 0 {
            break;
        }
        read += bytes as u64;
        if read > maximum {
            return Err(FinanceDataError::InvalidResponse);
        }
        if let Some(symbol) = parse_symbol(file, line.trim_ascii_end()) {
            valid += 1;
            if let Some(rank) = rank(&symbol, term) {
                count += 1;
                hits.push((rank, symbol));
                if hits.len() > limit {
                    hits.pop();
                }
            }
        }
    }
    if valid == 0 {
        return Err(FinanceDataError::InvalidResponse);
    }
    Ok((hits.into_vec(), count))
}

fn parse_symbol(file: MasterFile, line: &[u8]) -> Option<Symbol> {
    let mut market_code = None;
    let mut exchange = None;
    let mut english = None;
    let (symbol, name) = match file.layout {
        Layout::Domestic(code) => {
            market_code = Some(code);
            (line.get(..9)?, line.get(21..61)?)
        }
        Layout::Bond => {
            market_code = Some("B");
            (line.get(4..16)?, line.get(16..56)?)
        }
        Layout::DomesticDerivative => {
            let mut fields = line.split(|byte| *byte == b'|');
            (fields.nth(1)?, fields.nth(1)?)
        }
        Layout::Commodity => {
            market_code = Some("CF");
            (line.get(2..11)?, line.get(23..63)?)
        }
        Layout::OverseasEquity => {
            let mut fields = line.split(|byte| *byte == b'\t');
            exchange = decode(fields.nth(2)?);
            let symbol = fields.nth(1)?;
            let name = fields.nth(1)?;
            english = fields.next().and_then(decode);
            (symbol, name)
        }
        Layout::OverseasDerivative => {
            exchange = decode(line.get(132..142)?);
            (line.get(..32)?, line.get(82..132)?)
        }
    };
    let symbol = decode(symbol)?;
    let name = decode(name).or_else(|| english.clone())?;
    if !symbol
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b" ./-+_".contains(&byte))
    {
        return None;
    }
    Some(Symbol {
        market: file.market,
        symbol,
        name,
        english_name: english,
        market_code,
        exchange_code: exchange,
    })
}

fn decode(field: &[u8]) -> Option<String> {
    let (value, invalid) = encoding_rs::EUC_KR.decode_without_bom_handling(field);
    let value = value.trim();
    (!invalid && !value.is_empty() && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

fn rank(symbol: &Symbol, term: &str) -> Option<u8> {
    if normalize(&symbol.symbol) == term {
        return Some(0);
    }
    [Some(symbol.name.as_str()), symbol.english_name.as_deref()]
        .into_iter()
        .flatten()
        .filter_map(|name| {
            let name = normalize(name);
            if name == term {
                Some(1)
            } else if name.starts_with(term) {
                Some(2)
            } else if name.contains(term) {
                Some(3)
            } else {
                None
            }
        })
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn archive(name: &str, contents: &str) -> Vec<u8> {
        let mut zipped = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zipped
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        let (bytes, _, replaced) = encoding_rs::EUC_KR.encode(contents);
        assert!(!replaced);
        zipped.write_all(&bytes).unwrap();
        zipped.finish().unwrap().into_inner()
    }

    fn source(name: &str) -> MasterFile {
        *MASTERS.iter().find(|file| file.name == name).unwrap()
    }

    #[test]
    fn korean_and_english_names_resolve_to_kis_markets_and_symbols() {
        let file = source("nasmst.cod");
        let bytes = archive(
            "NASMST.COD",
            concat!(
                "US\t22\tNAS\t나스닥\tWIDG\tNASWIDG\t합성 도구\tWidget Company\n",
                "US\t22\tNAS\t나스닥\tALSO\tNASALSO\t다른 도구\tAnother Widget\n",
            ),
        );
        for query in [" Widget Company ", "합성도구", "widg"] {
            let (matches, _) = search_archive(file, &bytes, &normalize(query), 1).unwrap();
            let (_, hit) = &matches[0];
            assert_eq!(hit.symbol, "WIDG");
            assert_eq!(hit.exchange_code.as_deref(), Some("NAS"));
            assert_eq!(hit.market, "overseas_equity");
        }
        let (matches, count) = search_archive(file, &bytes, "도구", 1).unwrap();
        assert_eq!((matches.len(), count), (1, 2));
        assert!(search_archive(file, &archive("other.cod", "data"), "data", 1).is_err());
    }

    #[test]
    fn bond_elw_and_futures_contracts_keep_their_own_market_codes() {
        let bond = "F902KR1234567890시험채권                                00\n";
        let elw = "52X001   KR1234567890시험워런트                              0\n";
        let future = format!("{:<82}{:<50}{:<10}0\n", "TEST26", "Synthetic Future", "CME");
        for (file, text, term, symbol, market, market_code, exchange) in [
            (
                "bond_code.mst",
                bond,
                "시험채권",
                "KR1234567890",
                "domestic_bond",
                Some("B"),
                None,
            ),
            (
                "elw_code.mst",
                elw,
                "시험워런트",
                "52X001",
                "elw",
                Some("W"),
                None,
            ),
            (
                "ffcode.mst",
                future.as_str(),
                "syntheticfuture",
                "TEST26",
                "overseas_futures",
                None,
                Some("CME"),
            ),
        ] {
            let (hits, count) =
                search_archive(source(file), &archive(file, text), term, 1).unwrap();
            assert_eq!(count, 1);
            let hit = &hits[0].1;
            assert_eq!(hit.symbol, symbol);
            assert_eq!(hit.market, market);
            assert_eq!(hit.market_code, market_code);
            assert_eq!(hit.exchange_code.as_deref(), exchange);
        }
    }
}
