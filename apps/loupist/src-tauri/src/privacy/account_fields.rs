//! A brokerage account read's reviewed fields are replaced before the result
//! exists anywhere else: the model, datasets, result references, charts and exports.
//! This boundary does not depend on anything the app stores. An account operation
//! without a reviewed field list is refused, and account results cannot be saved
//! as Sources or Memory.
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

/// Whether this connection may expose the reviewed account fields.
/// This is the single boundary for any later decision to show them.
pub(crate) fn account_fields_visible(_state: &crate::app::AppState, _connection_id: &str) -> bool {
    false
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
        for private in ["1337", "48213900", "36050.0", "1200000"] {
            assert!(!text.contains(private), "{text}");
        }
    }
}
