//! Bounded argument text shared by the executor's tool handlers.
use super::*;

pub(super) fn bounded_text(
    value: Option<&Value>,
    min: usize,
    max: usize,
) -> Result<String, BrokerError> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| {
            let chars = text.chars().count();
            (min..=max).contains(&chars) && !text.contains('\0')
        })
        .map(str::to_owned)
        .ok_or(BrokerError::Malformed)
}

pub(super) fn optional_bounded_text(
    value: Option<&Value>,
    min: usize,
    max: usize,
) -> Result<Option<String>, BrokerError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(_) => bounded_text(value, min, max).map(Some),
    }
}

pub(super) fn optional_unique_strings(
    value: Option<&Value>,
    max_items: usize,
    max_chars: usize,
) -> Result<Vec<String>, BrokerError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let items = value
        .as_array()
        .filter(|items| items.len() <= max_items)
        .ok_or(BrokerError::Malformed)?;
    let mut unique = BTreeSet::new();
    let mut values = Vec::with_capacity(items.len());
    for item in items {
        let text = bounded_text(Some(item), 1, max_chars)?;
        if !unique.insert(text.clone()) {
            return Err(BrokerError::Malformed);
        }
        values.push(text);
    }
    Ok(values)
}
