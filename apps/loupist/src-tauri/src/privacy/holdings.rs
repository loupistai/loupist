//! Holdings redaction (finance-watchers.md §7.3, D9). Holdings reach a model
//! only through the identity projection (symbol, market, name, held flag).
//! A Korea Investment account read is the one tool result that carries
//! quantities, amounts, prices paid and the rates derived from them: every
//! field the reviewed catalog lists in `holding_fields` is replaced with
//! [`HIDDEN`] before the result exists anywhere else (Pi's copy, the dataset,
//! the result ref, a chart or an export). It runs in every run, interactive
//! and unattended. An account operation without a reviewed list is refused.
//! Numbers the user types into a message are theirs to send and untouched.
use serde_json::Value;

/// What a hidden value reads as, in every result a model can see.
pub const HIDDEN: &str = "[hidden on this computer]";

/// How the reviewed catalog treats one KIS operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AccountReview {
    /// A public market read: nothing to hide.
    Market,
    /// An account read and the fields that must never leave this computer.
    Account(Vec<String>),
    /// An account read nobody reviewed: refused before any request.
    Unreviewed,
}

/// The refusal for an account read whose fields were not reviewed.
pub const UNREVIEWED: &str = "account_fields_unreviewed: this account read has no reviewed list of fields to keep on this computer, so it is not made";

/// The refusal for saving an account read as a Source or in Memory.
pub const NOT_SAVED: &str = "account_data_not_saved: account and balance reads stay on this computer and cannot become Sources or Memory";

/// Replaces every listed field's value, at any depth, with [`HIDDEN`].
/// Returns how many values it hid.
pub fn redact_fields(value: &mut Value, fields: &[String]) -> usize {
    let mut hidden = 0;
    match value {
        Value::Object(map) => {
            for (key, item) in map.iter_mut() {
                if fields.iter().any(|field| field.eq_ignore_ascii_case(key)) {
                    if !item.is_null() && item.as_str() != Some(HIDDEN) {
                        *item = Value::String(HIDDEN.into());
                        hidden += 1;
                    }
                } else {
                    hidden += redact_fields(item, fields);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                hidden += redact_fields(item, fields);
            }
        }
        _ => {}
    }
    hidden
}

/// Whether a `kis_read` result is an account read (`operation: account.read`).
pub fn is_account_result(value: &Value) -> bool {
    value.get("operation").and_then(Value::as_str) == Some("account.read")
        || value.pointer("/data/operation/id").is_some()
            && value.get("schema_version").and_then(Value::as_str)
                == Some("loupist-kis-account-result/1")
}

/// The operation a `kis_read` result answered.
pub fn operation_id(value: &Value) -> Option<&str> {
    value
        .pointer("/query/operation_id")
        .or_else(|| value.pointer("/data/operation/id"))
        .and_then(Value::as_str)
}

/// The quantity, value and cost strings of the latest holdings snapshot, as a
/// model-facing text would print them (debug and test canary checks).
pub fn canaries(positions: &[crate::store::user_context::HoldingsPosition]) -> Vec<String> {
    let mut out = Vec::new();
    for position in positions {
        for value in [
            Some(position.quantity.as_str()),
            position.cost_basis.as_deref(),
            position.market_value.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            let plain = value.trim().trim_end_matches(".0").to_owned();
            // Short values ("10") would match unrelated numbers.
            if plain.chars().filter(char::is_ascii_digit).count() < 4 {
                continue;
            }
            out.push(plain.clone());
            if let Some(grouped) = grouped(&plain) {
                out.push(grouped);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn grouped(value: &str) -> Option<String> {
    let (integer, fraction) = value
        .split_once('.')
        .map_or((value, None), |(i, f)| (i, Some(f)));
    if integer.len() <= 3 || !integer.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut out = String::new();
    for (index, digit) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    if let Some(fraction) = fraction {
        out.push('.');
        out.push_str(fraction);
    }
    Some(out)
}

/// The first canary a text contains (debug and test builds check every
/// outbound prompt and tool result with it).
pub fn leaked<'a>(text: &str, canaries: &'a [String]) -> Option<&'a str> {
    canaries
        .iter()
        .find(|canary| text.contains(canary.as_str()))
        .map(String::as_str)
}

/// Debug and test builds: the first quantity, value or cost of the latest
/// holdings snapshot that a model-facing text contains.
#[cfg(any(test, debug_assertions))]
pub(crate) fn scan(store: &crate::store::SqliteStore, text: &str) -> Option<String> {
    let snapshot = store.holdings_latest().ok().flatten()?;
    leaked(text, &canaries(&snapshot.positions)).map(str::to_owned)
}

/// Debug builds say loudly when a held number reached a prompt or a tool result.
#[cfg(debug_assertions)]
pub(crate) fn debug_scan(store: &crate::store::SqliteStore, what: &str, text: &str) {
    if scan(store, text).is_some() {
        eprintln!("Holdings canary: a held quantity or value reached {what}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn listed_fields_are_hidden_at_any_depth_and_identity_stays() {
        let mut result = json!({"operation": "account.read", "data": {"response": {
            "output1": [{"pdno": "990417", "prdt_name": "Bada Shipping", "prpr": "51200", "hldg_qty": "1337", "evlu_amt": "48213900", "pchs_avg_pric": "36050.0"}],
            "output2": [{"tot_evlu_amt": "48213900", "dnca_tot_amt": "1200000"}]
        }}});
        let fields: Vec<String> = [
            "hldg_qty",
            "evlu_amt",
            "pchs_avg_pric",
            "tot_evlu_amt",
            "dnca_tot_amt",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert_eq!(redact_fields(&mut result, &fields), 5);
        let row = &result["data"]["response"]["output1"][0];
        assert_eq!(
            (row["pdno"].as_str(), row["prpr"].as_str()),
            (Some("990417"), Some("51200"))
        );
        assert_eq!(row["hldg_qty"], HIDDEN);
        assert_eq!(
            result["data"]["response"]["output2"][0]["tot_evlu_amt"],
            HIDDEN
        );
        // Idempotent: a second pass hides nothing more.
        assert_eq!(redact_fields(&mut result, &fields), 0);
        assert!(is_account_result(&result));
        let text = result.to_string();
        let canaries = vec![
            "1337".to_owned(),
            "48,213,900".to_owned(),
            "48213900".to_owned(),
        ];
        assert_eq!(leaked(&text, &canaries), None);
        assert_eq!(
            leaked("you hold 1,337 shares", &["1,337".to_owned()]),
            Some("1,337")
        );
        assert_eq!(grouped("48213900").as_deref(), Some("48,213,900"));
    }
}
