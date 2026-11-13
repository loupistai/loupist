//! Scripted resolver replies exercise the temporal rule, without a provider.
use super::*;

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
    let start = old.valid_at;
    let end = new.invalid_at;
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
                content.contains(&text::isoformat(&t)),
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
