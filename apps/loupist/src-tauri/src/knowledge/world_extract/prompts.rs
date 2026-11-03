//! Graphiti 0.30.1 prompt templates (Apache-2.0, see `prompts/NOTICE`) and the
//! worker's own batch prompts, rendered exactly as the Python worker did.

use super::pyjson::Py;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub(crate) fn new(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }
}

/// Graphiti's `prompt_library` appends this to every system message.
pub(crate) const DO_NOT_ESCAPE_UNICODE: &str = "\nDo not escape unicode characters.\n";

const EXTRACT_TEXT_SYSTEM: &str = include_str!("prompts/extract_text.system.txt");
const EXTRACT_TEXT_USER: &str = include_str!("prompts/extract_text.user.txt");
const DEDUPE_NODES_SYSTEM: &str = include_str!("prompts/dedupe_nodes.system.txt");
const DEDUPE_NODES_USER: &str = include_str!("prompts/dedupe_nodes.user.txt");
const EXTRACT_EDGES_SYSTEM: &str = include_str!("prompts/extract_edges.system.txt");
const EXTRACT_EDGES_USER: &str = include_str!("prompts/extract_edges.user.txt");
const RESOLVE_EDGE_SYSTEM: &str = include_str!("prompts/resolve_edge.system.txt");
const RESOLVE_EDGE_USER: &str = include_str!("prompts/resolve_edge.user.txt");
const SUMMARIES_SYSTEM: &str = include_str!("prompts/extract_summaries_batch.system.txt");
const SUMMARIES_USER: &str = include_str!("prompts/extract_summaries_batch.user.txt");

/// Fill `{{name}}` slots in one pass, so inserted text is never re-read.
fn render(template: &str, slots: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len() + 256);
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..end];
        match slots.iter().find(|(slot, _)| *slot == name) {
            Some((_, value)) => out.push_str(value),
            None => {
                out.push_str("{{");
                out.push_str(name);
                out.push_str("}}");
            }
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

fn system(text: &str) -> Message {
    Message::new("system", format!("{text}{DO_NOT_ESCAPE_UNICODE}"))
}

/// `extract_nodes.extract_text`.
pub(crate) fn extract_text(
    entity_types: &Py,
    episode_content: &str,
    instructions: &str,
) -> Vec<Message> {
    vec![
        system(EXTRACT_TEXT_SYSTEM),
        Message::new(
            "user",
            render(
                EXTRACT_TEXT_USER,
                &[
                    ("entity_types", &entity_types.repr()),
                    ("episode_content", episode_content),
                    ("custom_extraction_instructions", instructions),
                ],
            ),
        ),
    ]
}

/// `dedupe_nodes.nodes`.
pub(crate) fn dedupe_nodes(
    previous_episodes: &Py,
    episode_content: &str,
    extracted_nodes: &Py,
    existing_nodes: &Py,
    count: usize,
) -> Vec<Message> {
    vec![
        system(DEDUPE_NODES_SYSTEM),
        Message::new(
            "user",
            render(
                DEDUPE_NODES_USER,
                &[
                    ("previous_episodes", &previous_episodes.dumps(false)),
                    ("episode_content", episode_content),
                    ("extracted_nodes", &extracted_nodes.dumps(false)),
                    ("existing_nodes", &existing_nodes.dumps(false)),
                    ("count", &count.to_string()),
                    ("last_index", &(count as i64 - 1).to_string()),
                ],
            ),
        ),
    ]
}

/// `extract_edges.edge`. `reference_time` is Python's `str(datetime)`.
pub(crate) fn extract_edges(
    previous_episodes: &Py,
    episode_content: &str,
    nodes: &Py,
    reference_time: &str,
    edge_types: &Py,
    instructions: &str,
) -> Vec<Message> {
    vec![
        system(EXTRACT_EDGES_SYSTEM),
        Message::new(
            "user",
            render(
                EXTRACT_EDGES_USER,
                &[
                    ("previous_episodes", &previous_episodes.dumps(false)),
                    ("episode_content", episode_content),
                    ("nodes", &nodes.dumps(false)),
                    ("reference_time", reference_time),
                    ("edge_types", &edge_types.dumps(false)),
                    ("custom_extraction_instructions", instructions),
                ],
            ),
        ),
    ]
}

/// `dedupe_edges.resolve_edge`. The fact lists are interpolated with `repr()`.
pub(crate) fn resolve_edge(
    existing_edges: &Py,
    invalidation_candidates: &Py,
    new_edge: &str,
) -> Vec<Message> {
    vec![
        system(RESOLVE_EDGE_SYSTEM),
        Message::new(
            "user",
            render(
                RESOLVE_EDGE_USER,
                &[
                    ("existing_edges", &existing_edges.repr()),
                    (
                        "edge_invalidation_candidates",
                        &invalidation_candidates.repr(),
                    ),
                    ("new_edge", new_edge),
                ],
            ),
        ),
    ]
}

/// `extract_nodes.extract_summaries_batch` without type descriptions.
pub(crate) fn summaries_batch(
    previous_episodes: &Py,
    episode_content: &str,
    entities: &Py,
) -> Vec<Message> {
    vec![
        system(SUMMARIES_SYSTEM),
        Message::new(
            "user",
            render(
                SUMMARIES_USER,
                &[
                    ("previous_episodes", &previous_episodes.dumps(false)),
                    ("episode_content", &Py::str(episode_content).dumps(false)),
                    ("entities", &entities.dumps(false)),
                ],
            ),
        ),
    ]
}

/// The worker's batch instruction for independent edge resolutions.
pub(crate) const EDGE_BATCH_INSTRUCTIONS: &str = concat!(
    "Complete each independent fact-resolution task in the supplied cases. ",
    "Follow each case's task instructions. Candidate idx values are local ",
    "to that case: never compare, copy or transfer facts or indices between ",
    "cases. Return every case under its exact key, with no extra keys."
);

/// The worker's one-call attribute instruction.
pub(crate) const ATTRIBUTE_INSTRUCTIONS: &str = concat!(
    "Fill only fields the source states. ",
    "Set a field to null otherwise. ",
    "Never copy a value from another entity. ",
    "original_name is the name exactly as written in a non-English source."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_fill_every_slot_once_and_keep_inserted_braces() {
        for template in [
            EXTRACT_TEXT_USER,
            DEDUPE_NODES_USER,
            EXTRACT_EDGES_USER,
            RESOLVE_EDGE_USER,
            SUMMARIES_USER,
        ] {
            assert!(template.contains("{{"));
        }
        let rendered = render("a {{x}} b {{y}}", &[("x", "{{y}}"), ("y", "2")]);
        assert_eq!(rendered, "a {{y}} b 2");
        let messages = resolve_edge(&Py::List(vec![]), &Py::List(vec![]), "new");
        assert!(!messages[1].content.contains("{{"));
        assert!(messages[0].content.ends_with(DO_NOT_ESCAPE_UNICODE));
    }
}
