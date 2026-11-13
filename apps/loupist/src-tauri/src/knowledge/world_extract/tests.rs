//! Frozen golden replays (`golden/stages.json`): the same stage inputs and model replies must
//! produce byte-identical model calls and the same stage outcome.

use std::collections::{HashMap, HashSet, VecDeque};

use serde_json::{json, Value};

use super::bridge::{Bridge, BridgeConfig};
use super::ontology::{extraction_instructions, merged_schema, Hints};
use super::pipeline::{Edge, Episode, Graph, Harness, Node, PreviousEpisode};
use super::pyjson::Py;
use super::search::MemoryClaimText;
use super::text;
use super::*;
use crate::knowledge::graph_inference::{InferenceDisposition, InferenceReply};

mod indexing;
mod jobs;
mod temporal;

const GOLDEN: &str = include_str!("golden/stages.json");
const EPISODE_KEY: &str = "ep_golden";

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("golden fixture")
}

fn case(id: &str) -> Value {
    golden()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == id)
        .cloned()
        .unwrap_or_else(|| panic!("golden case {id}"))
}

/// The host side of one stage: canned replies, recorded model calls.
struct Host {
    replies: VecDeque<Value>,
    calls: Vec<Value>,
    embedded: Vec<Vec<String>>,
}

impl Host {
    fn new(case: &Value) -> Self {
        Self {
            replies: case["responses"]
                .as_array()
                .unwrap()
                .iter()
                .cloned()
                .collect(),
            calls: Vec::new(),
            embedded: Vec::new(),
        }
    }

    fn reply(&mut self, request: Value) -> InferenceReply {
        if request["kind"] == "embedding" {
            let inputs: Vec<String> = request["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item.as_str().unwrap().to_string())
                .collect();
            let vectors: Vec<Value> = inputs.iter().map(|_| json!(vec![0.0; 128])).collect();
            self.embedded.push(inputs);
            return InferenceReply {
                disposition: InferenceDisposition::Completed,
                body: json!({"status": "ok", "json_object": null, "vectors": vectors}),
            };
        }
        self.calls.push(json!({
            "kind": request["kind"],
            "model_profile": request["model_profile"],
            "max_tokens": request["max_tokens"],
            "messages": request["messages"],
            "response_schema": request["response_schema"],
        }));
        InferenceReply {
            disposition: InferenceDisposition::Completed,
            body: json!({
                "status": "ok",
                "json_object": self.replies.pop_front().expect("a canned reply"),
                "usage": {"input_tokens": 10, "output_tokens": 5, "certainty": "accounted"},
            }),
        }
    }
}

fn config(case: &Value) -> BridgeConfig {
    let fixture = golden();
    BridgeConfig {
        scope: fixture["scope"].clone(),
        operation_id: "job_golden".into(),
        profile: fixture["profile"].as_str().unwrap().into(),
        fast_profile: case["fast_model"].as_str().unwrap_or("").into(),
        timeout_ms: 60_000,
        max_llm_calls: 16,
        max_embedding_calls: 64,
    }
}

fn time(value: &Value) -> Option<pipeline::Time> {
    value.as_str().and_then(text::from_isoformat)
}

fn episode(case: &Value) -> (Episode, Vec<PreviousEpisode>) {
    (
        Episode {
            key: EPISODE_KEY.into(),
            content: case["episode"]["content"].as_str().unwrap().into(),
            valid_at: time(&case["episode"]["valid_at"]).unwrap(),
        },
        case["previous"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| PreviousEpisode {
                content: item["content"].as_str().unwrap().into(),
                valid_at: time(&item["valid_at"]),
            })
            .collect(),
    )
}

fn node(value: &Value) -> Node {
    Node {
        uuid: value["uuid"].as_str().unwrap().into(),
        name: value["name"].as_str().unwrap().into(),
        labels: value["labels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|label| label.as_str().unwrap().to_string())
            .collect(),
        summary: value["summary"].as_str().unwrap_or("").into(),
        attributes: match &value["attributes"] {
            Value::Array(pairs) => pairs
                .iter()
                .map(|pair| (pair[0].as_str().unwrap().to_string(), pair[1].clone()))
                .collect(),
            Value::Object(items) => items
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            _ => Vec::new(),
        },
        name_embedding: None,
        spellings: Vec::new(),
    }
}

fn edge(value: &Value) -> Edge {
    Edge {
        uuid: value["uuid"].as_str().unwrap().into(),
        source: value["source"].as_str().unwrap().into(),
        target: value["target"].as_str().unwrap().into(),
        name: value["name"].as_str().unwrap().into(),
        fact: value["fact"].as_str().unwrap().into(),
        episodes: value["episodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap().to_string())
            .collect(),
        valid_at: time(&value["valid_at"]),
        invalid_at: time(&value["invalid_at"]),
        expired_at: value["expired_at"]
            .as_str()
            .and_then(text::from_isoformat)
            .or_else(|| (value["expired"] == true).then(|| chrono::Utc::now().fixed_offset())),
        fact_embedding: None,
    }
}

fn node_json(node: &Node) -> Value {
    json!({
        "uuid": node.uuid,
        "name": node.name,
        "labels": node.labels,
        "summary": node.summary,
        "attributes": node
            .attributes
            .iter()
            .map(|(key, value)| json!([key, value]))
            .collect::<Vec<_>>(),
    })
}

fn edge_json(edge: &Edge) -> Value {
    json!({
        "uuid": edge.uuid,
        "source": edge.source,
        "target": edge.target,
        "name": edge.name,
        "fact": edge.fact,
        "episodes": edge.episodes,
        "valid_at": edge.valid_at.as_ref().map(text::isoformat),
        "invalid_at": edge.invalid_at.as_ref().map(text::isoformat),
        "expired": edge.expired_at.is_some(),
    })
}

fn python_edge(value: &Value, known: &HashSet<String>) -> Value {
    let mut value = value.clone();
    if let Some(episodes) = value["episodes"].as_array_mut() {
        for item in episodes.iter_mut() {
            if !known.contains(item.as_str().unwrap()) {
                *item = json!(EPISODE_KEY);
            }
        }
    }
    value
}

fn assert_calls(case: &Value, host: &Host) {
    let expected = case["calls"].as_array().unwrap();
    assert_eq!(
        host.calls.len(),
        expected.len(),
        "{}: model call count",
        case["id"]
    );
    for (index, (actual, expected)) in host.calls.iter().zip(expected).enumerate() {
        for key in ["kind", "model_profile", "max_tokens", "response_schema"] {
            assert_eq!(
                actual[key], expected[key],
                "{} call {index}: {key}",
                case["id"]
            );
        }
        let actual_messages = actual["messages"].as_array().unwrap();
        let expected_messages = expected["messages"].as_array().unwrap();
        assert_eq!(
            actual_messages.len(),
            expected_messages.len(),
            "{} call {index}: messages",
            case["id"]
        );
        for (position, (left, right)) in actual_messages.iter().zip(expected_messages).enumerate() {
            assert_eq!(
                left["role"], right["role"],
                "{} call {index} message {position} role",
                case["id"]
            );
            let (left, right) = (
                left["content"].as_str().unwrap(),
                right["content"].as_str().unwrap(),
            );
            if left != right {
                let at = left
                    .char_indices()
                    .zip(right.chars())
                    .find(|((_, a), b)| a != b)
                    .map(|((at, _), _)| at)
                    .unwrap_or(left.len().min(right.len()));
                let start = at.saturating_sub(80);
                panic!(
                    "{} call {index} message {position} differs at byte {at}:\n rust: {:?}\n python: {:?}",
                    case["id"],
                    &left[start..(at + 80).min(left.len())],
                    right.get(start..(at + 80).min(right.len())).unwrap_or(""),
                );
            }
        }
    }
}

fn schema_and_instructions(case: &Value) -> (ontology::Schema, String) {
    let hints = Hints::from_value(&case["hints"]);
    (
        merged_schema(&hints.extension),
        extraction_instructions(&hints),
    )
}

fn empty_text() -> MemoryClaimText {
    MemoryClaimText::new(std::iter::empty()).unwrap()
}

#[test]
fn golden_extract_nodes() {
    let case = case("extract_nodes");
    let (schema, instructions) = schema_and_instructions(&case);
    let (episode, previous) = episode(&case);
    let mut host = Host::new(&case);
    let mut infer = |request| host.reply(request);
    let mut bridge = Bridge::new(config(&case), &mut infer);
    let text = empty_text();
    let mut harness = Harness::new(
        &mut bridge,
        &schema,
        episode,
        previous,
        instructions,
        Graph::default(),
        &text,
    );
    let extracted = harness.extract_nodes().unwrap();
    let nodes: Vec<Value> = extracted
        .iter()
        .map(|index| {
            let node = harness.node(*index);
            json!({"name": node.name, "labels": node.labels})
        })
        .collect();
    drop(harness);
    drop(bridge);
    assert_calls(&case, &host);
    assert_eq!(Value::Array(nodes), case["result"]["nodes"]);
}

#[test]
fn golden_resolve_nodes() {
    let case = case("resolve_nodes");
    let (schema, instructions) = schema_and_instructions(&case);
    let (episode, previous) = episode(&case);
    let mut host = Host::new(&case);
    let mut infer = |request| host.reply(request);
    let mut bridge = Bridge::new(config(&case), &mut infer);
    let text = empty_text();
    let mut harness = Harness::new(
        &mut bridge,
        &schema,
        episode,
        previous,
        instructions,
        Graph::default(),
        &text,
    );
    let extracted: Vec<usize> = case["input"]["extracted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| harness.add_node(node(value)))
        .collect();
    let candidates: Vec<Vec<Node>> = case["input"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|found| found.as_array().unwrap().iter().map(node).collect())
        .collect();
    let (resolved, uuid_map) = harness
        .resolve_with_candidates(&extracted, candidates)
        .unwrap();
    let resolved: Vec<Value> = resolved
        .iter()
        .map(|index| node_json(harness.node(*index)))
        .collect();
    drop(harness);
    drop(bridge);
    assert_calls(&case, &host);
    assert_eq!(Value::Array(resolved), case["result"]["resolved"]);
    let expected: HashMap<String, String> =
        serde_json::from_value(case["result"]["uuid_map"].clone()).unwrap();
    assert_eq!(uuid_map, expected);
}

#[test]
fn golden_extract_edges() {
    let case = case("extract_edges");
    let (schema, instructions) = schema_and_instructions(&case);
    let (episode, previous) = episode(&case);
    let mut host = Host::new(&case);
    let mut infer = |request| host.reply(request);
    let mut bridge = Bridge::new(config(&case), &mut infer);
    let text = empty_text();
    let mut harness = Harness::new(
        &mut bridge,
        &schema,
        episode,
        previous,
        instructions,
        Graph::default(),
        &text,
    );
    let nodes: Vec<usize> = case["input"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| harness.add_node(node(value)))
        .collect();
    let edges = harness.extract_edges(&nodes).unwrap();
    let known = HashSet::new();
    let actual: Vec<Value> = edges
        .iter()
        .map(|index| {
            let mut value = edge_json(harness.edge(*index));
            value.as_object_mut().unwrap().remove("uuid");
            value
        })
        .collect();
    drop(harness);
    drop(bridge);
    assert_calls(&case, &host);
    let expected: Vec<Value> = case["result"]["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| {
            let mut value = python_edge(value, &known);
            value.as_object_mut().unwrap().remove("uuid");
            value
        })
        .collect();
    assert_eq!(actual, expected);
}

fn golden_resolution(id: &str) {
    let case = case(id);
    let (schema, instructions) = schema_and_instructions(&case);
    let (episode, previous) = episode(&case);
    let mut host = Host::new(&case);
    let mut infer = |request| host.reply(request);
    let mut bridge = Bridge::new(config(&case), &mut infer);
    let text = empty_text();
    let mut harness = Harness::new(
        &mut bridge,
        &schema,
        episode,
        previous,
        instructions,
        Graph::default(),
        &text,
    );
    let extracted: Vec<usize> = case["input"]["extracted"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| harness.add_edge(edge(value)))
        .collect();
    let lists = |key: &str, harness: &mut Harness<'_, '_>| -> Vec<Vec<usize>> {
        case["input"][key]
            .as_array()
            .unwrap()
            .iter()
            .map(|found| {
                found
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| harness.add_edge(edge(value)))
                    .collect()
            })
            .collect()
    };
    let related = lists("related", &mut harness);
    let others = lists("others", &mut harness);
    let (resolved, invalidated, new) = harness.decide_edges(&extracted, related, others).unwrap();
    let mut known: HashSet<String> = HashSet::new();
    for key in ["extracted", "related", "others"] {
        let flat: Vec<&Value> = if key == "extracted" {
            case["input"][key].as_array().unwrap().iter().collect()
        } else {
            case["input"][key]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|found| found.as_array().unwrap())
                .collect()
        };
        for value in flat {
            for item in value["episodes"].as_array().unwrap() {
                known.insert(item.as_str().unwrap().to_string());
            }
        }
    }
    let project = |indices: &[usize], harness: &Harness<'_, '_>| -> Vec<Value> {
        indices
            .iter()
            .map(|index| edge_json(harness.edge(*index)))
            .collect()
    };
    let actual_resolved = project(&resolved, &harness);
    let actual_invalidated = project(&invalidated, &harness);
    let actual_new: Vec<String> = new
        .iter()
        .map(|index| harness.edge(*index).uuid.clone())
        .collect();
    drop(harness);
    drop(bridge);
    assert_calls(&case, &host);
    let expected = |key: &str| -> Vec<Value> {
        case["result"][key]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| python_edge(value, &known))
            .collect()
    };
    assert_eq!(actual_resolved, expected("resolved"), "{id}: resolved");
    assert_eq!(
        actual_invalidated,
        expected("invalidated"),
        "{id}: invalidated"
    );
    let expected_new: Vec<String> = serde_json::from_value(case["result"]["new"].clone()).unwrap();
    assert_eq!(actual_new, expected_new);
}

#[test]
fn golden_resolve_edges_batch() {
    golden_resolution("resolve_edges_batch");
}

#[test]
fn golden_resolve_edges_single() {
    golden_resolution("resolve_edges_single");
}

#[test]
fn golden_attributes_and_summaries() {
    let case = case("attributes_summaries");
    let (schema, instructions) = schema_and_instructions(&case);
    let (episode, previous) = episode(&case);
    let mut host = Host::new(&case);
    let mut infer = |request| host.reply(request);
    let mut bridge = Bridge::new(config(&case), &mut infer);
    let text = empty_text();
    let mut harness = Harness::new(
        &mut bridge,
        &schema,
        episode,
        previous,
        instructions,
        Graph::default(),
        &text,
    );
    let mut by_uuid = HashMap::new();
    for value in case["input"]["nodes"].as_array().unwrap() {
        let index = harness.add_node(node(value));
        by_uuid.insert(value["uuid"].as_str().unwrap().to_string(), index);
    }
    let order: Vec<usize> = case["input"]["order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|uuid| by_uuid[uuid.as_str().unwrap()])
        .collect();
    let edges: Vec<usize> = case["input"]["new_edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| harness.add_edge(edge(value)))
        .collect();
    harness.attributes_and_summaries(&order, &edges).unwrap();
    let actual: Vec<Value> = order
        .iter()
        .map(|index| node_json(harness.node(*index)))
        .collect();
    drop(harness);
    drop(bridge);
    assert_calls(&case, &host);
    assert_eq!(Value::Array(actual), case["result"]["nodes"]);
    assert_eq!(json!(host.embedded), case["embedding_inputs"]);
}

#[test]
fn golden_projection() {
    let fixture = golden()["projection"].clone();
    let prior_nodes: HashMap<String, Value> =
        serde_json::from_value(fixture["prior_nodes"].clone()).unwrap();
    let prior_claims: HashMap<String, Value> =
        serde_json::from_value(fixture["prior_claims"].clone()).unwrap();
    let nodes: Vec<Node> = fixture["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(node)
        .collect();
    let edges: Vec<Edge> = fixture["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(edge)
        .collect();
    let binding = &fixture["binding"];
    let projected = projection::project_extraction(
        &projection::Context {
            source_role: "source",
            binding: projection::Binding {
                span_id: binding["span_id"].as_str().unwrap(),
                binding_role: binding["binding_role"].as_str().unwrap(),
                attribution: binding["attribution"].as_str().unwrap(),
            },
            publication_time: &fixture["publication_time"],
            effective_time: None,
            recorded_at_ms: fixture["recorded_at_ms"].as_i64().unwrap(),
            prior_nodes: &prior_nodes,
            prior_claims: &prior_claims,
            episode_runtime: EPISODE_KEY,
        },
        &nodes,
        &edges,
    );
    assert_eq!(Value::Array(projected.nodes), fixture["projected_nodes"]);
    assert_eq!(Value::Array(projected.claims), fixture["claims"]);
}

#[test]
fn golden_scalars() {
    let scalars = golden()["scalars"].clone();
    for item in scalars["repr"].as_array().unwrap() {
        let value = Py::str(item["value"].as_str().unwrap());
        assert_eq!(
            value.repr(),
            item["repr"].as_str().unwrap(),
            "repr of {}",
            item["value"]
        );
        assert_eq!(value.dumps(true), item["json"].as_str().unwrap());
        assert_eq!(value.dumps(false), item["json_unicode"].as_str().unwrap());
    }
    for item in scalars["labels"].as_array().unwrap() {
        assert_eq!(
            json!(text::new_node_labels(item["type"].as_str().unwrap())),
            item["labels"],
            "labels for {}",
            item["type"]
        );
    }
    for item in scalars["fallback"].as_array().unwrap() {
        let actual = text::lexical_fallback_vector(item["text"].as_str().unwrap(), 128);
        let expected: Vec<f64> = serde_json::from_value(item["vector"].clone()).unwrap();
        assert_eq!(actual.len(), expected.len());
        for (left, right) in actual.iter().zip(&expected) {
            assert!(
                (f64::from(*left) - right).abs() < 1e-6,
                "fallback for {}",
                item["text"]
            );
        }
    }
    for item in scalars["instructions"].as_array().unwrap() {
        let hints = Hints::from_value(&item["hints"]);
        assert_eq!(
            extraction_instructions(&hints),
            item["text"].as_str().unwrap()
        );
    }
    for item in scalars["truncate"].as_array().unwrap() {
        assert_eq!(
            text::truncate_at_sentence(
                item["text"].as_str().unwrap(),
                item["limit"].as_u64().unwrap() as usize
            ),
            item["result"].as_str().unwrap()
        );
    }
}
