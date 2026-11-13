//! Response schemas, byte-identical to the Pydantic `model_json_schema()` the
//! worker sent, and strict/lax parsing of model results.
//!
//! Pydantic sorts every schema object's keys except `properties` (field order)
//! and `default`; required fields keep declaration order.

use serde_json::Value;

use super::pyjson::Py;

/// Pydantic's field title: `name.title().replace("_", " ")`.
pub(crate) fn field_title(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut previous_cased = false;
    for ch in name.chars() {
        let cased = ch.is_lowercase() || ch.is_uppercase();
        if cased {
            if previous_cased {
                out.extend(ch.to_lowercase());
            } else {
                out.extend(ch.to_uppercase());
            }
        } else {
            out.push(ch);
        }
        previous_cased = cased;
    }
    out.replace('_', " ").trim().to_string()
}

fn string_field(name: &str, description: &str) -> (String, Py) {
    (
        name.into(),
        Py::dict([
            ("description", Py::str(description)),
            ("title", Py::str(field_title(name))),
            ("type", Py::str("string")),
        ]),
    )
}

fn integer_field(name: &str, description: Option<&str>) -> (String, Py) {
    let mut items = Vec::new();
    if let Some(description) = description {
        items.push(("description", Py::str(description)));
    }
    items.push(("title", Py::str(field_title(name))));
    items.push(("type", Py::str("integer")));
    (name.into(), Py::dict(items))
}

fn integer_list_field(name: &str, description: &str) -> (String, Py) {
    (
        name.into(),
        Py::dict([
            ("description", Py::str(description)),
            ("items", Py::dict([("type", Py::str("integer"))])),
            ("title", Py::str(field_title(name))),
            ("type", Py::str("array")),
        ]),
    )
}

fn optional_string_field(name: &str, description: &str) -> (String, Py) {
    (
        name.into(),
        Py::dict([
            (
                "anyOf",
                Py::List(vec![
                    Py::dict([("type", Py::str("string"))]),
                    Py::dict([("type", Py::str("null"))]),
                ]),
            ),
            ("default", Py::None),
            ("description", Py::str(description)),
            ("title", Py::str(field_title(name))),
        ]),
    )
}

fn model_list_field(name: &str, model: &str, description: Option<&str>) -> (String, Py) {
    let mut items = Vec::new();
    if let Some(description) = description {
        items.push(("description", Py::str(description)));
    }
    items.push((
        "items",
        Py::dict([("$ref", Py::str(format!("#/$defs/{model}")))]),
    ));
    items.push(("title", Py::str(field_title(name))));
    items.push(("type", Py::str("array")));
    (name.into(), Py::dict(items))
}

fn model(
    title: &str,
    properties: Vec<(String, Py)>,
    required: &[&str],
    forbid_extra: bool,
    defs: Vec<(String, Py)>,
) -> Py {
    let mut items = Vec::new();
    if !defs.is_empty() {
        let mut defs = defs;
        defs.sort_by(|left, right| left.0.cmp(&right.0));
        items.push(("$defs", Py::Dict(defs)));
    }
    if forbid_extra {
        items.push(("additionalProperties", Py::Bool(false)));
    }
    items.push(("properties", Py::Dict(properties)));
    if !required.is_empty() {
        items.push(("required", Py::strings(required.iter().copied())));
    }
    items.push(("title", Py::str(title)));
    items.push(("type", Py::str("object")));
    Py::dict(items)
}

pub(crate) fn extracted_entities() -> Py {
    let entity = model(
        "ExtractedEntity",
        vec![
            string_field("name", "Identifying name of a concrete entity, never a quantity, measurement, figure with a unit or sentence. Numbers in named products, securities, tickers, fiscal entities and events are allowed."),
            integer_field(
                "entity_type_id",
                Some("ID of the classified entity type. Must be one of the provided entity_type_id integers."),
            ),
            integer_list_field(
                "episode_indices",
                "List of episode numbers (0-indexed) this entity was extracted from. When processing a single episode, this should be [0].",
            ),
        ],
        &["name", "entity_type_id"],
        false,
        Vec::new(),
    );
    model(
        "ExtractedEntities",
        vec![model_list_field(
            "extracted_entities",
            "ExtractedEntity",
            Some("List of extracted entities"),
        )],
        &["extracted_entities"],
        false,
        vec![("ExtractedEntity".into(), entity)],
    )
}

pub(crate) fn node_resolutions() -> Py {
    let duplicate = model(
        "NodeDuplicate",
        vec![
            integer_field("id", Some("integer id of the entity")),
            string_field(
                "name",
                "Name of the entity. Should be the most complete and descriptive name of the entity. Do not include any JSON formatting in the Entity name such as {}.",
            ),
            integer_field(
                "duplicate_candidate_id",
                Some("candidate_id of the matching EXISTING ENTITY, or -1 if no duplicate exists."),
            ),
        ],
        &["id", "name", "duplicate_candidate_id"],
        false,
        Vec::new(),
    );
    model(
        "NodeResolutions",
        vec![model_list_field(
            "entity_resolutions",
            "NodeDuplicate",
            Some("List of resolved nodes"),
        )],
        &["entity_resolutions"],
        false,
        vec![("NodeDuplicate".into(), duplicate)],
    )
}

pub(crate) fn extracted_edges() -> Py {
    let edge = model(
        "Edge",
        vec![
            string_field(
                "source_entity_name",
                "The name of the source entity from the ENTITIES list",
            ),
            string_field(
                "target_entity_name",
                "The name of the target entity from the ENTITIES list",
            ),
            string_field(
                "relation_type",
                "The type of relationship between the entities, in SCREAMING_SNAKE_CASE (e.g., WORKS_AT, LIVES_IN, IS_FRIENDS_WITH)",
            ),
            string_field(
                "fact",
                "A natural language description of the relationship between the entities, paraphrased from the source text",
            ),
            optional_string_field(
                "valid_at",
                "The date and time when the relationship described by the edge fact became true or was established. Use ISO 8601 format (YYYY-MM-DDTHH:MM:SS.SSSSSSZ)",
            ),
            optional_string_field(
                "invalid_at",
                "The date and time when the relationship described by the edge fact stopped being true or ended. Use ISO 8601 format (YYYY-MM-DDTHH:MM:SS.SSSSSSZ)",
            ),
            integer_list_field(
                "episode_indices",
                "List of episode numbers (0-indexed) that this fact was derived from. When processing a single episode, this should be [0].",
            ),
        ],
        &[
            "source_entity_name",
            "target_entity_name",
            "relation_type",
            "fact",
        ],
        false,
        Vec::new(),
    );
    model(
        "ExtractedEdges",
        vec![model_list_field("edges", "Edge", None)],
        &["edges"],
        false,
        vec![("Edge".into(), edge)],
    )
}

fn edge_duplicate_model() -> Py {
    model(
        "EdgeDuplicate",
        vec![
            integer_list_field(
                "duplicate_facts",
                "List of idx values of duplicate facts (only from EXISTING FACTS range). Empty list if none.",
            ),
            integer_list_field(
                "contradicted_facts",
                "List of idx values of contradicted facts (from full idx range). Empty list if none.",
            ),
        ],
        &["duplicate_facts", "contradicted_facts"],
        false,
        Vec::new(),
    )
}

pub(crate) fn edge_duplicate() -> Py {
    edge_duplicate_model()
}

/// The worker's `EdgeResolutionBatch`: one strict `case_N` per batched request.
pub(crate) fn edge_resolution_batch(cases: usize) -> Py {
    let keys: Vec<String> = (0..cases).map(|index| format!("case_{index}")).collect();
    model(
        "EdgeResolutionBatch",
        keys.iter()
            .map(|key| {
                (
                    key.clone(),
                    Py::dict([("$ref", Py::str("#/$defs/EdgeDuplicate"))]),
                )
            })
            .collect(),
        &keys.iter().map(String::as_str).collect::<Vec<_>>(),
        true,
        vec![("EdgeDuplicate".into(), edge_duplicate_model())],
    )
}

pub(crate) fn summarized_entities() -> Py {
    let summary = model(
        "SummarizedEntity",
        vec![
            string_field("name", "Name of the entity being summarized"),
            string_field("summary", "Updated summary for the entity"),
        ],
        &["name", "summary"],
        false,
        Vec::new(),
    );
    model(
        "SummarizedEntities",
        vec![model_list_field(
            "summaries",
            "SummarizedEntity",
            Some("List of entity summaries. Only include entities that need summary updates."),
        )],
        &["summaries"],
        false,
        vec![("SummarizedEntity".into(), summary)],
    )
}

/// The worker's `CoreAttributeBatch` over the union of one batch's type fields.
pub(crate) fn attribute_batch(fields: &[(String, String)]) -> Py {
    let values = model(
        "CoreAttributeValues",
        fields
            .iter()
            .map(|(name, description)| optional_string_field(name, description))
            .collect(),
        &[],
        false,
        Vec::new(),
    );
    let row = model(
        "CoreAttributeRow",
        vec![
            integer_field("index", None),
            (
                "attributes".into(),
                Py::dict([("$ref", Py::str("#/$defs/CoreAttributeValues"))]),
            ),
        ],
        &["index", "attributes"],
        true,
        Vec::new(),
    );
    model(
        "CoreAttributeBatch",
        vec![model_list_field("entities", "CoreAttributeRow", None)],
        &["entities"],
        true,
        vec![
            ("CoreAttributeRow".into(), row),
            ("CoreAttributeValues".into(), values),
        ],
    )
}

// --- Parsing ------------------------------------------------------------------

/// A model result Pydantic would reject. The worker reported it as a failed
/// ingest; no retry is attempted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invalid(pub &'static str);

/// Pydantic's lax `int` in Python mode: ints, bools, integral floats and
/// integer strings.
pub(crate) fn lax_int(value: &Value) -> Result<i64, Invalid> {
    match value {
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                return Ok(value);
            }
            let float = number.as_f64().ok_or(Invalid("integer is invalid"))?;
            if float.fract() == 0.0 && float.is_finite() && float.abs() < 9.2e18 {
                Ok(float as i64)
            } else {
                Err(Invalid("integer is invalid"))
            }
        }
        Value::Bool(value) => Ok(i64::from(*value)),
        Value::String(text) => text
            .trim()
            .replace('_', "")
            .parse::<i64>()
            .map_err(|_| Invalid("integer is invalid")),
        _ => Err(Invalid("integer is invalid")),
    }
}

pub(crate) fn strict_int(value: &Value) -> Result<i64, Invalid> {
    value.as_i64().ok_or(Invalid("integer is invalid"))
}

pub(crate) fn lax_str(value: &Value) -> Result<String, Invalid> {
    value
        .as_str()
        .map(str::to_string)
        .ok_or(Invalid("string is invalid"))
}

pub(crate) fn field<'a>(object: &'a Value, key: &str) -> Result<&'a Value, Invalid> {
    object
        .as_object()
        .ok_or(Invalid("object is invalid"))?
        .get(key)
        .ok_or(Invalid("required field is missing"))
}

pub(crate) fn lax_int_list(value: &Value) -> Result<Vec<i64>, Invalid> {
    value
        .as_array()
        .ok_or(Invalid("list is invalid"))?
        .iter()
        .map(lax_int)
        .collect()
}

pub(crate) fn strict_int_list(value: &Value) -> Result<Vec<i64>, Invalid> {
    value
        .as_array()
        .ok_or(Invalid("list is invalid"))?
        .iter()
        .map(strict_int)
        .collect()
}

pub(crate) fn objects(value: &Value) -> Result<&[Value], Invalid> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or(Invalid("list is invalid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_follow_python_title_case() {
        assert_eq!(field_title("entity_type_id"), "Entity Type Id");
        assert_eq!(field_title("price2x_ratio"), "Price2X Ratio");
        assert_eq!(field_title("original_name"), "Original Name");
    }
}
