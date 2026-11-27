//! Scripted resolver replies exercise the temporal rule, without a provider.
use super::*;
use std::collections::BTreeMap;

fn instant(value: &str) -> Option<pipeline::Time> {
    text::from_isoformat(value)
}

fn assertion(id: &str, fact: &str, start: Option<&str>, end: Option<&str>) -> Edge {
    Edge {
        uuid: id.into(),
        source: "person".into(),
        target: "company".into(),
        name: "LEADS".into(),
        fact: fact.into(),
        episodes: vec![if id == "new" {
            EPISODE_KEY.into()
        } else {
            "old_source".into()
        }],
        valid_at: start.and_then(instant),
        invalid_at: end.and_then(instant),
        expired_at: None,
        fact_embedding: None,
        assertions: Default::default(),
        ending: None,
        conflicts: Vec::new(),
    }
}

fn resolve_reply(
    old: Edge,
    new: Edge,
    contradicted: bool,
    other: bool,
    duplicate: bool,
) -> Vec<Edge> {
    let ending = new.invalid_at.is_some();
    let start = old.valid_at.clone();
    let end = new.invalid_at.clone();
    let incompatible_end = old.invalid_at != new.invalid_at;
    let case = case("resolve_edges_single");
    let (schema, instructions) = schema_and_instructions(&case);
    let (episode, previous) = episode(&case);
    let mut host = Host::new(&json!({"responses": [{
        "duplicate_facts": if duplicate { vec![0] } else { vec![] }, "contradicted_facts": if contradicted { vec![0] } else { vec![] }
    }]}));
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
    let old_index = harness.add_edge(old);
    let new_index = harness.add_edge(new);
    let (related, others) = if other {
        (vec![vec![]], vec![vec![old_index]])
    } else {
        (vec![vec![old_index]], vec![vec![]])
    };
    let (resolved, invalidated, _) = harness.decide_edges(&[new_index], related, others).unwrap();
    if duplicate && ending && incompatible_end {
        assert_eq!(harness.edge(resolved[0]).uuid, "new");
        assert_eq!(harness.edge(resolved[0]).invalid_at, end);
        assert_eq!(harness.edge(old_index).episodes, ["old_source"]);
    }
    let result = invalidated
        .iter()
        .map(|i| harness.edge(*i).clone())
        .collect();
    drop(harness);
    drop(bridge);
    if ending {
        let messages = host.calls[0]["messages"].as_array().unwrap();
        let content = messages[1]["content"].as_str().unwrap();
        for key in [
            "relation_type",
            "source_id",
            "target_id",
            "valid_at",
            "invalid_at",
            "expired_at",
        ] {
            assert!(content.contains(key), "missing validity context {key}");
        }
        for t in [start, end].into_iter().flatten() {
            assert!(
                content.contains(text::isoformat(&t)),
                "missing stated boundary"
            );
        }
    }
    result
}

fn resolve(old: Edge, new: Edge, contradicted: bool, other: bool) -> Vec<Edge> {
    resolve_reply(old, new, contradicted, other, false)
}

const START: &str = "2026-02-02T10:00:00Z";
const END: &str = "2026-07-07T14:00:00Z";

#[test]
fn explicit_termination_closes_selected_old_assertion_without_a_new_start() {
    for (old_fact, new_fact, other) in [
        (
            "Ari became chief executive of Cedar at 10:00 UTC on February 2, 2026.",
            "Ari stepped down from Cedar effective July 7, 2026 at 14:00 UTC.",
            false,
        ),
        (
            "Cedar began supplying Elm under a contract effective February 2, 2026 at 10:00 UTC.",
            "Elm terminated that contract effective July 7, 2026 at 14:00 UTC.",
            true,
        ),
        (
            "아리는 2026년 2월 2일 10:00 UTC부터 시더의 대표이사로 재직했다.",
            "아리는 2026년 7월 7일 14:00 UTC에 시더 대표이사직에서 물러났다.",
            false,
        ),
    ] {
        let old = assertion("old", old_fact, Some(START), None);
        let mut new = assertion("new", new_fact, None, Some(END));
        if other {
            // Reversed roles and an open relation do not prevent comparison.
            new.source = "company".into();
            new.target = "person".into();
            new.name = "TERMINATED_CONTRACT_WITH".into();
        }
        let invalidated = resolve(old, new, true, other);
        assert_eq!(invalidated.len(), 1, "{new_fact}");
        assert_eq!(invalidated[0].uuid, "old");
        assert_eq!(invalidated[0].valid_at, instant(START));
        assert_eq!(invalidated[0].invalid_at, instant(END));
        assert!(invalidated[0].expired_at.is_some());
        assert_eq!(invalidated[0].episodes, ["old_source"]);
    }
}

#[test]
fn termination_requires_an_ordered_instant_and_a_resolver_selection() {
    for (start, end) in [
        (None, Some(END)),
        (Some(END), Some(END)),
        (Some(END), Some(START)),
        (Some(START), None),
    ] {
        let old = assertion("old", "Ari leads Cedar.", start, None);
        let new = assertion("new", "Ari left Cedar.", None, end);
        assert!(resolve(old, new, true, false).is_empty());
    }
    for fact in [
        "Cedar denied an allegation that its contract ended on July 7, 2026 at 14:00 UTC.",
        "A rumour says Ari might step down on July 7, 2026 at 14:00 UTC.",
        "An analyst alleges Cedar's contract will end on July 7, 2026 at 14:00 UTC.",
        "Ari joined Cedar's unrelated advisory committee on July 7, 2026 at 14:00 UTC.",
        "시더는 2026년 7월 7일 14:00 UTC에 계약이 종료됐다는 의혹을 부인했다.",
        "아리가 2026년 7월 7일 14:00 UTC에 사임한다는 소문이 있다.",
    ] {
        let old = assertion("old", "Ari leads Cedar.", Some(START), None);
        // Even if extraction dates a disputed event, only a confirmed resolver
        // selection may end the old assertion.
        let new = assertion("new", fact, None, Some(END));
        assert!(resolve(old, new, false, false).is_empty(), "{fact}");
    }
}

#[test]
fn a_duplicate_selection_cannot_discard_a_new_explicit_end() {
    for selected in [false, true] {
        let old = assertion("old", "Cedar began supplying Elm.", Some(START), None);
        let new = assertion("new", "Cedar stopped supplying Elm.", None, Some(END));
        let invalidated = resolve_reply(old, new, selected, false, true);
        assert_eq!(invalidated.len(), usize::from(selected));
        if selected {
            assert_eq!(invalidated[0].invalid_at, instant(END));
        }
    }
}

#[test]
fn a_verbatim_fact_cannot_hide_a_new_explicit_end() {
    let fact = "Cedar supplies Elm.";
    let old = assertion("old", fact, Some(START), None);
    let new = assertion("new", fact, None, Some(END));
    let invalidated = resolve(old, new, true, false);
    assert_eq!(invalidated.len(), 1);
    assert_eq!(invalidated[0].invalid_at, instant(END));
}

#[test]
fn a_restated_start_with_an_end_closes_the_existing_interval() {
    let old = assertion(
        "old",
        "Cedar supplies Elm from its contract start.",
        Some(START),
        None,
    );
    let new = assertion(
        "new",
        "Cedar supplied Elm for the stated interval.",
        Some(START),
        Some(END),
    );
    let invalidated = resolve_reply(old, new, true, false, true);
    assert_eq!(invalidated.len(), 1);
    assert_eq!(invalidated[0].valid_at, instant(START));
    assert_eq!(invalidated[0].invalid_at, instant(END));
}

#[derive(Clone, Copy)]
struct Filing {
    source: &'static str,
    claim: &'static str,
    publication: Option<&'static str>,
    start: Option<&'static str>,
    end: Option<&'static str>,
    relation: &'static str,
}

impl Filing {
    fn edge(self) -> Edge {
        use crate::knowledge::model::{AssertionSources, AssertionWitness, StatedEnd};
        let mut edge = assertion(self.claim, self.claim, self.start, self.end);
        edge.name = self.relation.into();
        edge.episodes = vec![self.source.into()];
        let witness = AssertionWitness {
            span: self.source.into(),
            publication: self.publication.and_then(instant),
        };
        edge.assertions = AssertionSources {
            source_ids: [self.source.into()].into_iter().collect(),
            stated: edge.invalid_at.clone().map(|time| StatedEnd {
                time,
                witness: witness.clone(),
            }),
            witness: Some(witness),
            ..Default::default()
        };
        edge
    }
}

fn filed(source: &'static str, claim: &'static str, publication: Option<&'static str>) -> Filing {
    Filing {
        source,
        claim,
        publication,
        start: None,
        end: None,
        relation: "LEADS",
    }
}

fn scripted_order(filings: &[Filing], marked: bool) -> Graph {
    scripted_replay(filings, |_| None, |_, _| marked)
}

/// Replays `filings` in order. `fact` rewords a filing, so the resolver rather
/// than the verbatim match settles it; the resolver marks a stored claim as a
/// duplicate when it is the same claim, and as the same assertion when
/// `marks(filing, stored claim)`.
fn scripted_replay(
    filings: &[Filing],
    fact: impl Fn(&Filing) -> Option<String>,
    marks: impl Fn(&Filing, &str) -> bool,
) -> Graph {
    use crate::knowledge::model::AssertionWitness;
    use crate::knowledge::world_episodes::ReferenceBasis;
    let case = case("resolve_edges_single");
    let (schema, instructions) = schema_and_instructions(&case);
    let mut graph = Graph::default();
    for filing in filings {
        let (mut episode, previous) = episode(&case);
        episode.key = filing.source.into();
        episode.source = Some(pipeline::EpisodeSource {
            source_id: filing.source.into(),
            witness: AssertionWitness {
                span: filing.source.into(),
                publication: filing.publication.and_then(instant),
            },
            basis: if filing.publication.is_some() {
                ReferenceBasis::Published
            } else {
                ReferenceBasis::Received
            },
        });
        let mut incoming = filing.edge();
        if let Some(fact) = fact(filing) {
            incoming.fact = fact;
        }
        // Use a separate arena identity until the resolver settles this filing.
        incoming.uuid = format!("{}:{}", filing.claim, filing.source);
        let claims: Vec<&str> = graph
            .edges
            .iter()
            .map(|edge| canonical_id(&edge.uuid))
            .collect();
        let positions = |keep: &dyn Fn(&str) -> bool| -> Vec<usize> {
            (0..claims.len())
                .filter(|index| keep(claims[*index]))
                .collect()
        };
        let duplicates = positions(&|claim| claim == filing.claim);
        let contradicted = positions(&|claim| claim != filing.claim);
        let same = positions(&|claim| claim != filing.claim && marks(filing, claim));
        let mut host = Host::new(&json!({"responses": [{
            "duplicate_facts": duplicates, "contradicted_facts": contradicted,
            "same_assertion_facts": same,
        }]}));
        let mut infer = |request| host.reply(request);
        let mut bridge = Bridge::new(config(&case), &mut infer);
        let text = empty_text();
        let mut harness = Harness::new(
            &mut bridge,
            &schema,
            episode,
            previous,
            instructions.clone(),
            graph.clone(),
            &text,
        );
        let stored: Vec<_> = graph
            .edges
            .iter()
            .cloned()
            .map(|edge| harness.add_edge(edge))
            .collect();
        let new = harness.add_edge(incoming);
        let (resolved, _, _) = harness
            .decide_edges(&[new], vec![stored.clone()], vec![vec![]])
            .unwrap();
        let mut indices = stored;
        if resolved[0] == new {
            indices.push(new);
        }
        graph = Graph {
            edges: indices
                .into_iter()
                .map(|index| harness.edge(index).clone())
                .collect(),
            links: harness.assertion_links().to_vec(),
            nodes: Vec::new(),
        };
    }
    graph
}

fn canonical_id(uuid: &str) -> &str {
    uuid.split_once(':').map_or(uuid, |(fact, _)| fact)
}

type Interval<'a> = Option<(
    &'a pipeline::Time,
    Option<&'a str>,
    &'a str,
    crate::knowledge::world_episodes::EndBasis,
)>;

fn intervals(graph: &Graph) -> BTreeMap<&str, (Interval<'_>, Vec<&str>)> {
    graph
        .edges
        .iter()
        .map(|edge| {
            let ending = edge.ending.as_ref().map(|end| {
                (
                    &end.time,
                    end.by.as_deref().map(canonical_id),
                    end.span.as_str(),
                    end.basis,
                )
            });
            (
                edge.fact.as_str(),
                (
                    ending,
                    edge.conflicts.iter().map(|id| canonical_id(id)).collect(),
                ),
            )
        })
        .collect()
}

#[test]
fn selected_pairs_follow_the_temporal_rules_and_keep_both_conflict_links() {
    use crate::knowledge::world_episodes::EndBasis;
    let a = filed("jan", "original", Some("2026-01-02"));
    let b = filed("feb", "replacement", Some("2026-02-06"));
    let ordered = scripted_order(&[a, b], true);
    let reverse = scripted_order(&[b, a], true);
    assert_eq!(intervals(&ordered), intervals(&reverse));
    let end = ordered.edges[0].ending.as_ref().unwrap();
    assert_eq!(end.time.value.as_deref(), b.publication);
    assert!(end.time.approximate);
    assert_eq!(end.basis, EndBasis::Reference);
    assert_eq!(end.span, b.source);

    for (name, second, marked) in [
        (
            "Q1 versus Q2",
            Filing {
                claim: "Q2 sales",
                ..b
            },
            false,
        ),
        (
            "guidance versus actual",
            Filing {
                claim: "FY actual",
                ..b
            },
            false,
        ),
        (
            "received only",
            Filing {
                publication: None,
                ..b
            },
            true,
        ),
        (
            "different relation label",
            Filing {
                relation: "DIRECTS",
                ..b
            },
            true,
        ),
        (
            "month contains the day",
            Filing {
                publication: Some("2026-01"),
                ..b
            },
            true,
        ),
        (
            "same day, one explicit",
            Filing {
                publication: a.publication,
                start: Some("2025-08"),
                ..b
            },
            true,
        ),
        (
            "same day, two instants",
            Filing {
                publication: Some("2026-01-02T15:00:00+00:00"),
                ..b
            },
            true,
        ),
    ] {
        let world = scripted_order(&[a, second], marked);
        assert!(
            world
                .edges
                .iter()
                .all(|edge| edge.invalid_at.is_none() && edge.conflicts.len() == 1),
            "{name}"
        );
        assert_eq!(
            canonical_id(&world.edges[0].conflicts[0]),
            second.claim,
            "{name}"
        );
        assert_eq!(
            canonical_id(&world.edges[1].conflicts[0]),
            a.claim,
            "{name}"
        );
    }
    let one_source = scripted_order(
        &[
            a,
            Filing {
                source: a.source,
                ..b
            },
        ],
        true,
    );
    assert!(one_source
        .edges
        .iter()
        .all(|edge| edge.invalid_at.is_none() && edge.conflicts.is_empty()));

    let explicit = scripted_order(
        &[
            Filing {
                start: Some("2024"),
                ..a
            },
            Filing {
                start: Some("2025-08"),
                publication: a.publication,
                ..b
            },
        ],
        false,
    );
    let end = explicit.edges[0].ending.as_ref().unwrap();
    assert_eq!(end.time.value.as_deref(), Some("2025-08"));
    assert_eq!(end.time.precision, crate::knowledge::TimePrecision::Month);
    assert_eq!(end.basis, EndBasis::Stated);
    assert!(!end.time.approximate);
    assert_eq!(end.span, b.source);

    let future = scripted_order(
        &[Filing {
            end: Some("2099-01"),
            ..a
        }],
        false,
    );
    assert!(future.edges[0].invalid_at.is_some());
    assert!(future.edges[0].expired_at.is_none());
}

#[test]
fn scripted_replays_converge_and_the_cutoff_matches_a_publication_prefix() {
    use crate::commands::knowledge::as_of::{claim_as_of, Cutoff};
    use crate::knowledge::model::{
        GraphClaim, CONFLICTS_WITH_CLAIM, END_BASIS, SUPERSEDED_BY_CLAIM, SUPERSEDED_BY_SPAN,
    };

    const ORDERS: [[usize; 3]; 6] = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for explicit in [false, true] {
        for marked in [false, true] {
            for middle in ["2026-01-02", "2026-02-17", "2026-04-02"] {
                // Interleaved days end by reference only with a restating
                // Source's own selection, so "mark whatever is shown" is a
                // different set of selections in each order. The fixed-set
                // replay is a_restatement_reopens_only_when_its_source_marked_the_pair.
                if marked && !explicit && middle != "2026-04-02" {
                    continue;
                }
                let sources = [
                    Filing {
                        start: explicit.then_some("2024"),
                        ..filed("jan", "old", Some("2026-01-02"))
                    },
                    Filing {
                        start: explicit.then_some("2025"),
                        ..filed("middle", "new", Some(middle))
                    },
                    Filing {
                        start: explicit.then_some("2024"),
                        ..filed("mar", "old", Some("2026-03-01"))
                    },
                ];
                let expected = scripted_order(&sources, marked);
                for order in ORDERS {
                    let ordered = order.map(|index| sources[index]);
                    let actual = scripted_order(&ordered, marked);
                    assert_eq!(
                        intervals(&actual),
                        intervals(&expected),
                        "{explicit} {marked} {middle} {order:?}"
                    );
                    assert_eq!(
                        actual
                            .edges
                            .iter()
                            .find(|edge| edge.fact == "old")
                            .unwrap()
                            .publication()
                            .unwrap()
                            .value
                            .as_deref(),
                        sources[0].publication
                    );
                    for day in ["2025-12-31", "2026-01-29", "2026-02-28", "2026-04-30"] {
                        let cutoff = Cutoff::parse(Some(day)).unwrap();
                        let selected: Vec<_> = ordered
                            .iter()
                            .copied()
                            .filter(|source| {
                                source.publication.is_some_and(|published| published <= day)
                            })
                            .collect();
                        let prefix = scripted_order(&selected, marked);
                        let spans: std::collections::BTreeSet<_> = selected
                            .iter()
                            .map(|source| source.source.to_owned())
                            .collect();
                        let visible: std::collections::BTreeSet<_> = selected
                            .iter()
                            .map(|source| source.claim.to_owned())
                            .collect();
                        let claims_at = |graph: &Graph| {
                            let mut rows = BTreeMap::new();
                            for edge in &graph.edges {
                                let mut qualifiers = Vec::new();
                                if let Some(end) = &edge.ending {
                                    qualifiers.push(json!({"key":END_BASIS, "value":end.basis}));
                                    qualifiers
                                        .push(json!({"key":SUPERSEDED_BY_SPAN, "value":end.span}));
                                    if let Some(by) = &end.by {
                                        qualifiers.push(json!({"key":SUPERSEDED_BY_CLAIM, "value":canonical_id(by)}));
                                    }
                                }
                                for id in &edge.conflicts {
                                    qualifiers.push(json!({"key":CONFLICTS_WITH_CLAIM,"value":canonical_id(id)}));
                                }
                                let mut claim: GraphClaim = serde_json::from_value(json!({
                                    "id": edge.fact, "proposition":edge.fact, "attribution":"publisher",
                                    "relation":edge.name, "endpoints":[], "qualifiers":qualifiers, "status":"active",
                                    "evidence":edge.episodes.iter().map(|span| json!({
                                        "spanId":span, "role":"supports", "attribution":"publisher",
                                    })).collect::<Vec<_>>(), "invalidAt":edge.invalid_at, "validAt":edge.valid_at,
                                    "expiredAt":edge.expired_at,
                                })).unwrap();
                                claim_as_of(&mut claim, &spans, &visible, cutoff);
                                if !claim.evidence.is_empty() {
                                    claim.evidence.sort_by(|a, b| a.span_id.cmp(&b.span_id));
                                    rows.insert(claim.id.clone(), claim);
                                }
                            }
                            rows
                        };
                        assert_eq!(
                            claims_at(&actual),
                            claims_at(&prefix),
                            "{explicit} {marked} {middle} {order:?} {day}"
                        );
                    }
                }
            }
        }
    }
}

/// Each claim's ending, conflicts and reopening, by claim rather than by the
/// arena identity the first filing gave it.
fn outcomes(graph: &Graph) -> BTreeMap<String, Value> {
    graph
        .edges
        .iter()
        .map(|edge| {
            let ending = edge.ending.as_ref().map(|end| {
                json!({
                    "time": end.time, "by": end.by.as_deref().map(canonical_id),
                    "span": end.span, "basis": end.basis,
                })
            });
            let conflicts: Vec<_> = edge.conflicts.iter().map(|id| canonical_id(id)).collect();
            (
                canonical_id(&edge.uuid).to_owned(),
                json!({
                    "ending": ending, "invalid_at": edge.invalid_at, "conflicts": conflicts,
                    "days": edge.assertions.days, "reopened": edge.assertions.reopened,
                    "lapsed": edge.assertions.lapsed,
                }),
            )
        })
        .collect()
}

#[test]
fn a_restatement_reopens_only_when_its_source_marked_the_pair() {
    use crate::knowledge::world_episodes::EndBasis;
    // X: an officer leads the company; Y: an incompatible successor.
    let jan = filed("kvartal-jan", "X", Some("2026-01-08"));
    let feb = filed("bericht-feb", "Y", Some("2026-02-12"));
    let mar = filed("informe-mar", "X", Some("2026-03-19"));
    // Reworded, so the resolver settles each filing.
    let fact = |filing: &Filing| Some(format!("{} per {}", filing.claim, filing.source));
    let day = |value: &str| {
        let mut time = instant(value).unwrap();
        time.approximate = true;
        time
    };

    // March marked Y again: X stays current and Y ends in March.
    let witnessed = |filing: &Filing, _: &str| filing.source != jan.source;
    let mut results = Vec::new();
    for order in [[jan, feb, mar], [feb, jan, mar], [feb, mar, jan]] {
        let world = scripted_replay(&order, fact, witnessed);
        let names = order.map(|filing| filing.source);
        results.push((names, outcomes(&world)));
    }
    let (_, expected) = &results[0];
    for (names, actual) in &results {
        assert_eq!(actual, expected, "{names:?}");
    }
    let x = &expected["X"];
    let y = &expected["Y"];
    assert!(x["ending"].is_null() && x["invalid_at"].is_null(), "{x}");
    assert_eq!(x["conflicts"], json!([]));
    assert_eq!(
        x["reopened"],
        json!({"at": day("2026-03-19"), "bySpan": mar.source})
    );
    assert_eq!(
        x["lapsed"],
        json!([{"from": day("2026-02-12"), "to": day("2026-03-19")}])
    );
    assert_eq!(
        y["ending"],
        json!({
            "time": day("2026-03-19"), "by": "X", "span": mar.source,
            "basis": EndBasis::Reference,
        })
    );
    assert!(y["reopened"].is_null());

    // February alone ends X; March's restatement then withdraws that ending.
    let before = scripted_replay(&[jan, feb], fact, witnessed);
    let ended = &outcomes(&before)["X"];
    assert_eq!(ended["ending"]["time"], json!(day("2026-02-12")));
    assert!(ended["reopened"].is_null());

    // March did not mark Y: the two conflict and neither ends.
    let unwitnessed = |filing: &Filing, _: &str| filing.source == feb.source;
    let mut results = Vec::new();
    for order in [[jan, feb, mar], [jan, mar, feb], [mar, jan, feb]] {
        let world = scripted_replay(&order, fact, unwitnessed);
        results.push((order.map(|filing| filing.source), outcomes(&world)));
    }
    let (_, expected) = &results[0];
    for (names, actual) in &results {
        assert_eq!(actual, expected, "{names:?}");
    }
    for (claim, other) in [("X", "Y"), ("Y", "X")] {
        let row = &expected[claim];
        assert!(
            row["ending"].is_null() && row["reopened"].is_null(),
            "{row}"
        );
        assert_eq!(row["conflicts"], json!([other]));
    }

    // A received-only restatement adds no day and reopens nothing.
    let received = Filing {
        publication: None,
        ..mar
    };
    let world = scripted_replay(&[jan, feb, received], fact, witnessed);
    let rows = outcomes(&world);
    assert_eq!(rows["X"]["ending"]["time"], json!(day("2026-02-12")));
    assert_eq!(rows["X"]["days"], json!([instant("2026-01-08")]));
}

#[test]
fn a_correcting_source_ends_the_old_figure_without_also_disagreeing_with_it() {
    use crate::knowledge::world_episodes::EndBasis;
    // T-RESTATE: P is the old output figure; the restatement yields the
    // corrected figure Q and a fact E about the correction itself.
    let old = filed("annual-jan", "P", Some("2026-01-08"));
    let corrected = filed("restatement-mar", "Q", Some("2026-03-19"));
    let correction = Filing {
        relation: "CORRECTED",
        ..filed("restatement-mar", "E", Some("2026-03-19"))
    };
    let fact = |filing: &Filing| Some(format!("{} per {}", filing.claim, filing.source));
    let same =
        |filing: &Filing, claim: &str| matches!((filing.claim, claim), ("P", "Q") | ("Q", "P"));
    let mut results = Vec::new();
    for order in [
        [old, corrected, correction],
        [old, correction, corrected],
        [correction, corrected, old],
    ] {
        let world = scripted_replay(&order, fact, same);
        results.push((order.map(|filing| filing.claim), outcomes(&world)));
    }
    let (_, expected) = &results[0];
    for (names, actual) in &results {
        assert_eq!(actual, expected, "{names:?}");
    }
    let mut day = instant("2026-03-19").unwrap();
    day.approximate = true;
    assert_eq!(
        expected["P"]["ending"],
        json!({"time": day, "by": "Q", "span": corrected.source, "basis": EndBasis::Reference})
    );
    for claim in ["P", "Q", "E"] {
        assert_eq!(expected[claim]["conflicts"], json!([]), "{claim}");
    }

    // A second Source also reports the correction: that claim keeps the link.
    let wire = Filing {
        source: "wire-apr",
        publication: Some("2026-04-02"),
        ..correction
    };
    let world = scripted_replay(&[old, corrected, correction, wire], fact, same);
    let rows = outcomes(&world);
    assert_eq!(rows["P"]["ending"]["by"], json!("Q"));
    assert_eq!(rows["P"]["conflicts"], json!(["E"]));
    // The scripted resolver also selects E against Q, which no longer shares
    // one Source with it; only the link to the ended P is in question here.
    assert!(rows["E"]["conflicts"]
        .as_array()
        .unwrap()
        .contains(&json!("P")));
}
