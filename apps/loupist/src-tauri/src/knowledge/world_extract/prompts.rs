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

/// Every bundled template by the file name an evaluation override replaces.
const BUNDLED: [(&str, &str); 10] = [
    ("extract_text.system.txt", EXTRACT_TEXT_SYSTEM),
    ("extract_text.user.txt", EXTRACT_TEXT_USER),
    ("dedupe_nodes.system.txt", DEDUPE_NODES_SYSTEM),
    ("dedupe_nodes.user.txt", DEDUPE_NODES_USER),
    ("extract_edges.system.txt", EXTRACT_EDGES_SYSTEM),
    ("extract_edges.user.txt", EXTRACT_EDGES_USER),
    ("resolve_edge.system.txt", RESOLVE_EDGE_SYSTEM),
    ("resolve_edge.user.txt", RESOLVE_EDGE_USER),
    ("extract_summaries_batch.system.txt", SUMMARIES_SYSTEM),
    ("extract_summaries_batch.user.txt", SUMMARIES_USER),
];

/// The template a model call renders: the bundled text, or in an `eval-harness`
/// build the text of `LOUPIST_EVAL_WORLD_PROMPTS`.
fn template(name: &str) -> &'static str {
    #[cfg(feature = "eval-harness")]
    if let Some(texts) = eval_override::active() {
        return texts.text(name);
    }
    BUNDLED
        .iter()
        .find(|(file, _)| *file == name)
        .map(|(_, text)| *text)
        .expect("a bundled template")
}

/// The fact policy that opens every World model call.
#[allow(
    dead_code,
    reason = "world_extract/bridge.rs opens each call with it once its lane wires it"
)]
pub(crate) fn fact_policy() -> &'static str {
    #[cfg(feature = "eval-harness")]
    if let Some(texts) = eval_override::active() {
        return texts.text(eval_override::POLICY_FILE);
    }
    super::ontology::WORLD_FACT_POLICY
}

/// `LOUPIST_EVAL_WORLD_PROMPTS=<dir>` replaces the bundled templates and the fact
/// policy for an evaluation. Only an `eval-harness` build has this path; the
/// release profile refuses that feature.
#[cfg(feature = "eval-harness")]
pub(crate) mod eval_override {
    use std::{collections::BTreeMap, path::Path, sync::OnceLock};

    pub(crate) const VARIABLE: &str = "LOUPIST_EVAL_WORLD_PROMPTS";
    pub(super) const POLICY_FILE: &str = "policy.txt";

    pub(crate) struct WorldPrompts {
        texts: BTreeMap<String, &'static str>,
        pub(super) sha256: String,
    }

    impl WorldPrompts {
        pub(super) fn text(&self, name: &str) -> &'static str {
            self.texts[name]
        }
    }

    /// The directory must hold exactly the bundled file names and `policy.txt`,
    /// each a regular UTF-8 file.
    pub(crate) fn load(dir: &Path) -> Result<WorldPrompts, String> {
        let expected = super::BUNDLED
            .iter()
            .map(|(name, _)| *name)
            .chain([POLICY_FILE])
            .collect::<Vec<_>>();
        let mut texts = BTreeMap::new();
        for entry in std::fs::read_dir(dir).map_err(|error| format!("{VARIABLE}: {error}"))? {
            let entry = entry.map_err(|error| format!("{VARIABLE}: {error}"))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !expected.contains(&name.as_str()) {
                return Err(format!("{VARIABLE} has an unexpected file: {name}"));
            }
            let kind = entry
                .file_type()
                .map_err(|error| format!("{VARIABLE}: {error}"))?;
            if !kind.is_file() {
                return Err(format!("{VARIABLE}/{name} is not a regular file"));
            }
            let text = std::fs::read_to_string(entry.path())
                .map_err(|error| format!("{VARIABLE}/{name}: {error}"))?;
            texts.insert(name, text);
        }
        if let Some(missing) = expected.iter().find(|name| !texts.contains_key(**name)) {
            return Err(format!("{VARIABLE} is missing {missing}"));
        }
        let mut hashed = Vec::new();
        for (name, text) in &texts {
            for part in [name.as_bytes(), text.as_bytes()] {
                hashed.extend_from_slice(&(part.len() as u64).to_be_bytes());
                hashed.extend_from_slice(part);
            }
        }
        Ok(WorldPrompts {
            sha256: crate::util::sha256(&hashed),
            // Loaded once per process for its whole life.
            texts: texts
                .into_iter()
                .map(|(name, text)| (name, &*Box::leak(text.into_boxed_str())))
                .collect(),
        })
    }

    static ACTIVE: OnceLock<Result<Option<WorldPrompts>, String>> = OnceLock::new();

    #[cfg(test)]
    thread_local! {
        /// A test's override for its own thread; the process keeps the bundled text.
        pub(super) static TEST_OVERRIDE: std::cell::Cell<Option<&'static WorldPrompts>> =
            const { std::cell::Cell::new(None) };
    }

    /// The override a value of the variable names: none without one.
    pub(super) fn from_variable(
        value: Option<std::ffi::OsString>,
    ) -> Result<Option<WorldPrompts>, String> {
        value.map(|dir| load(Path::new(&dir))).transpose()
    }

    fn resolve() -> &'static Result<Option<WorldPrompts>, String> {
        ACTIVE.get_or_init(|| from_variable(std::env::var_os(VARIABLE)))
    }

    /// The digest of the override in use, `None` without one. A runner calls
    /// this before its first World call, so a refused directory stops the run.
    #[allow(
        dead_code,
        reason = "eval_harness/world.rs records it once its lane wires it"
    )]
    pub(crate) fn world_prompts_sha256() -> Result<Option<&'static str>, String> {
        resolve()
            .as_ref()
            .map(|prompts| prompts.as_ref().map(|prompts| prompts.sha256.as_str()))
            .map_err(Clone::clone)
    }

    pub(super) fn active() -> Option<&'static WorldPrompts> {
        #[cfg(test)]
        if let Some(prompts) = TEST_OVERRIDE.get() {
            return Some(prompts);
        }
        match resolve() {
            Ok(prompts) => prompts.as_ref(),
            // Never fall back to the bundled text under a refused override.
            Err(error) => panic!("{error}"),
        }
    }
}

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
        system(template("extract_text.system.txt")),
        Message::new(
            "user",
            render(
                template("extract_text.user.txt"),
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
        system(template("dedupe_nodes.system.txt")),
        Message::new(
            "user",
            render(
                template("dedupe_nodes.user.txt"),
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
        system(template("extract_edges.system.txt")),
        Message::new(
            "user",
            render(
                template("extract_edges.user.txt"),
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
        system(template("resolve_edge.system.txt")),
        Message::new(
            "user",
            render(
                template("resolve_edge.user.txt"),
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
        system(template("extract_summaries_batch.system.txt")),
        Message::new(
            "user",
            render(
                template("extract_summaries_batch.user.txt"),
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

pub(crate) const END_VALIDITY_INSTRUCTIONS: &str = concat!(
    "\nFor this end-bounded fact, compare the represented relationships and their validity ",
    "intervals, not merely whether an earlier sentence remains historically true. ",
    "Historical wording for a start does not turn an open relationship interval into ",
    "a point event: an explicit ending of that same relationship closes its validity. ",
    "Separate historical event participation remains true. Use the supplied relation ",
    "and participant identities to distinguish those cases; labels alone prove nothing. ",
    "Bounds are proposed extraction data, not independent proof. A denial, allegation, ",
    "rumour, hypothesis or forecast still cannot establish an ending. An open fact is ",
    "not a duplicate of a fact with a different explicit end."
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

    /// A complete override whose every file names itself.
    #[cfg(feature = "eval-harness")]
    fn override_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, _) in BUNDLED {
            std::fs::write(
                dir.path().join(name),
                format!("override {name} {{{{episode_content}}}}"),
            )
            .unwrap();
        }
        std::fs::write(dir.path().join("policy.txt"), "override policy.txt").unwrap();
        dir
    }

    /// In process: a child would run whatever test binary a concurrent build of
    /// another tree left at this path in a shared target directory. The variable
    /// is read once per process, so the test loads its value as `resolve` does
    /// and renders on its own thread.
    #[test]
    fn a_build_reads_the_variable_only_with_eval_harness() {
        #[cfg(feature = "eval-harness")]
        let _dir = {
            assert_eq!(eval_override::VARIABLE, "LOUPIST_EVAL_WORLD_PROMPTS");
            assert!(eval_override::from_variable(None).unwrap().is_none());
            let dir = override_dir();
            let prompts = eval_override::from_variable(Some(dir.path().into()))
                .unwrap()
                .unwrap();
            eval_override::TEST_OVERRIDE.set(Some(Box::leak(Box::new(prompts))));
            dir
        };
        let messages = extract_text(&Py::List(vec![]), "Kraków Mills filed its report.", "");
        let policy = fact_policy();
        #[cfg(feature = "eval-harness")]
        eval_override::TEST_OVERRIDE.set(None);
        if cfg!(feature = "eval-harness") {
            assert_eq!(messages[0].content, format!("override extract_text.system.txt {{{{episode_content}}}}{DO_NOT_ESCAPE_UNICODE}"));
            assert_eq!(
                messages[1].content,
                "override extract_text.user.txt Kraków Mills filed its report."
            );
            assert_eq!(policy, "override policy.txt");
        } else {
            assert_eq!(
                messages[0].content,
                format!("{EXTRACT_TEXT_SYSTEM}{DO_NOT_ESCAPE_UNICODE}")
            );
            assert!(messages[1].content.starts_with(&EXTRACT_TEXT_USER[..40]));
            assert_eq!(policy, super::super::ontology::WORLD_FACT_POLICY);
        }
    }

    #[cfg(feature = "eval-harness")]
    #[test]
    fn an_override_holds_exactly_the_bundled_names_and_the_policy() {
        let dir = override_dir();
        let first = eval_override::load(dir.path()).unwrap();
        assert_eq!(first.text("policy.txt"), "override policy.txt");
        let again = eval_override::load(dir.path()).unwrap();
        std::fs::write(dir.path().join("policy.txt"), "another policy").unwrap();
        let changed = eval_override::load(dir.path()).unwrap();
        assert_eq!(first.sha256, again.sha256);
        assert_ne!(first.sha256, changed.sha256);

        std::fs::write(dir.path().join("notes.txt"), "extra").unwrap();
        let extra = eval_override::load(dir.path()).err().unwrap();
        assert!(extra.contains("unexpected file: notes.txt"), "{extra}");
        std::fs::remove_file(dir.path().join("notes.txt")).unwrap();

        std::fs::remove_file(dir.path().join("resolve_edge.user.txt")).unwrap();
        let missing = eval_override::load(dir.path()).err().unwrap();
        assert!(
            missing.contains("missing resolve_edge.user.txt"),
            "{missing}"
        );

        std::fs::create_dir(dir.path().join("resolve_edge.user.txt")).unwrap();
        let directory = eval_override::load(dir.path()).err().unwrap();
        assert!(directory.contains("not a regular file"), "{directory}");
    }

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
