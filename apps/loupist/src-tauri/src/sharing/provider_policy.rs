//! Export policy, independent of provider access/credentials. Unknown means restricted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DataPolicy {
    PublicRecord,
    Restricted,
}
use DataPolicy::{PublicRecord, Restricted};

/// Every bundled OpenBB provider and native financial/web source, plus native aliases.
/// PublicRecord is an export classification, not a declaration of copyright status.
pub(crate) const PROVIDERS: &[(&str, DataPolicy)] = &[
    ("sec", PublicRecord),
    ("opendart", PublicRecord),
    ("dart", PublicRecord),
    ("fred", PublicRecord),
    ("bls", PublicRecord),
    ("bea", PublicRecord),
    ("us_treasury", PublicRecord),
    ("federal_reserve", PublicRecord),
    ("ecb", PublicRecord),
    ("imf", PublicRecord),
    ("oecd", PublicRecord),
    ("worldbank", PublicRecord),
    ("world_bank", PublicRecord),
    ("world-bank", PublicRecord),
    ("cftc", PublicRecord),
    ("government_us", PublicRecord),
    ("congress_gov", PublicRecord),
    ("eia", PublicRecord),
    ("fmp", Restricted),
    ("intrinio", Restricted),
    ("polygon", Restricted),
    ("tiingo", Restricted),
    ("alpha_vantage", Restricted),
    ("benzinga", Restricted),
    ("tradier", Restricted),
    ("biztoc", Restricted),
    ("cboe", Restricted),
    ("nasdaq", Restricted),
    ("tmx", Restricted),
    ("deribit", Restricted),
    ("finra", Restricted),
    ("krx", Restricted),
    ("kis", Restricted),
    ("koreainvestment", Restricted),
    ("defillama", Restricted),
    ("famafrench", Restricted),
    ("yfinance", Restricted),
    ("seeking_alpha", Restricted),
    ("wsj", Restricted),
    ("finviz", Restricted),
    ("stockgrid", Restricted),
    ("multpl", Restricted),
    ("econdb", Restricted),
    ("tradingeconomics", Restricted),
    ("onchain", Restricted),
    ("native-web", Restricted),
    ("web-research", Restricted),
    ("exa", Restricted),
    ("openbb", Restricted),
];

pub(crate) fn classify(identity: &str) -> DataPolicy {
    let identity = identity.to_ascii_lowercase();
    if let Some(tool) = identity.strip_prefix("native-finance:") {
        return match tool {
            "dart_company_lookup"
            | "dart_filing_search"
            | "dart_filing_read"
            | "dart_financials"
            | "dart_major_report"
            | "dart_ownership"
            | "dart_dividends"
            | "worldbank_series" => PublicRecord,
            _ => Restricted,
        };
    }
    // OpenBB's saved provider is normally the bare provider id. Accept its explicit
    // namespace too, but never infer a public provider from an arbitrary suffix.
    let identity = identity.strip_prefix("openbb:").unwrap_or(&identity);
    let family = identity.split([':', '.', '/']).next().unwrap_or_default();
    PROVIDERS
        .iter()
        .find(|(id, _)| *id == family)
        .map(|(_, policy)| *policy)
        .unwrap_or(Restricted)
}
