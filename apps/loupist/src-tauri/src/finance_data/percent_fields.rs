//! Provider response fields documented as percent values (`1.23` means
//! 1.23 %), so the number matcher (`knowledge/numbers.rs`) reads them as
//! percents instead of guessing a fraction. Keep this table beside the provider
//! schemas it describes.
//!
//! - KRX daily trading (`krx.rs`): `FLUC_RT` (등락률); normalized rows carry it
//!   as `change_percent`.
//! - KIS (`marketplace/kis-read-api-v1.json`): `*_ctrt` (대비율), `*_rate` and
//!   numbered `*_rate1…` (비율·율), `*_rt` (수익률·손익률), and overseas `rate`
//!   / `n_rate` (등락율).
//! - Normalized and OpenBB rows: `*_percent`, `*_pct`.
//!
//! `*_ratio` fields (OpenBB valuation and liquidity ratios) are multiples or
//! fractions, not percents, and are not listed.

/// Exact field names, compared without case.
const PERCENT_FIELDS: &[&str] = &["fluc_rt", "change_percent", "rate", "n_rate"];

/// Field name endings, compared without case.
const PERCENT_SUFFIXES: &[&str] = &["_ctrt", "_rate", "_rt", "_percent", "_pct", "_pc"];

/// Whether a provider field holds a percent value.
pub(crate) fn is_percent_field(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    // `seln_ernn_rate1` numbers one of several rate columns.
    let base = lower.trim_end_matches(|c: char| c.is_ascii_digit());
    PERCENT_FIELDS.contains(&base)
        || PERCENT_SUFFIXES.iter().any(|suffix| base.ends_with(suffix))
        || base.contains("percent")
        || base.contains("pct")
}

#[cfg(test)]
mod tests {
    use super::is_percent_field;

    #[test]
    fn provider_percent_fields_are_listed_and_ratios_are_not() {
        for key in [
            "FLUC_RT",
            "prdy_ctrt",
            "w52_hgpr_vrss_prpr_ctrt",
            "prdy_vrss_vol_rate",
            "seln_ernn_rate3",
            "evlu_pfls_rt",
            "rate",
            "change_percent",
            "dividend_yield_pct",
        ] {
            assert!(is_percent_field(key), "{key}");
        }
        for key in [
            "current_ratio",
            "price_to_earnings_ratio",
            "TDD_CLSPRC",
            "prdy_vrss",
            "ctrt_size",
            "grmn_rate_cls_code",
        ] {
            assert!(!is_percent_field(key), "{key}");
        }
    }
}
