//! Figure checks at publication (worlds.md "Figure checks").

use super::*;
use crate::knowledge::{EvidenceFigure, PassageRange};

const SOURCE: &str = "한빛소재의 2025년 매출액은 1,234억 원이고 영업이익은 98억 원이다.";
const REVENUE: &str = "Hanbit Materials reported 2025 revenue of KRW 123.4 billion.";
const MARGIN: &str = "Hanbit Materials reported a 2025 operating margin of 7.9%.";

/// Nine wrong magnitudes of the Source's two amounts.
const WRONG: [&str; 9] = [
    "KRW 12.34 billion",
    "KRW 1.234 billion",
    "KRW 1,234 billion",
    "KRW 1.234 trillion",
    "KRW 12.34 trillion",
    "KRW 0.98 billion",
    "KRW 98 billion",
    "KRW 980 million",
    "KRW 9.8 trillion",
];

fn reported(amount: &str) -> String {
    format!("Hanbit Materials reported a 2025 figure of {amount}.")
}

#[test]
fn a_figure_that_differs_is_left_out_and_counted_with_eight_examples() {
    let mut fixture = Fixture::new();
    let mut script = Script::default();
    script.reply("ExtractedEntities", entities(&["Hanbit Materials"]));
    let mut edges: Vec<Value> = WRONG
        .iter()
        .map(|amount| {
            relation(
                "Hanbit Materials",
                "Hanbit Materials",
                "REPORTS",
                &reported(amount),
            )
        })
        .collect();
    edges.push(relation(
        "Hanbit Materials",
        "Hanbit Materials",
        "REPORTS",
        REVENUE,
    ));
    edges.push(relation(
        "Hanbit Materials",
        "Hanbit Materials",
        "REPORTS",
        MARGIN,
    ));
    script.reply("ExtractedEdges", json!({ "edges": edges }));
    script.reply("CoreAttributeBatch", json!({"entities": []}));
    let script = Arc::new(Mutex::new(script));
    let job = fixture.queue("Hanbit Materials 2025 results", SOURCE);
    let finished = drive(&mut fixture, &job, &script, "fixture");
    assert_eq!(finished.status, graph_jobs::JOB_COMPLETED);
    assert_eq!(finished.document.dropped.figure_mismatch, 9);
    assert_eq!(finished.document.dropped_figures.len(), 8);
    // The job view carries both to the Library.
    let view = serde_json::to_value(&finished).unwrap();
    assert_eq!(view["dropped"], json!({"figureMismatch": 9}));
    assert_eq!(view["droppedFigures"].as_array().unwrap().len(), 8);
    let chunk = PassageRange {
        chunk_index: 0,
        char_start: 0,
        char_end: SOURCE.chars().count() as u32,
    };
    for (example, amount) in finished.document.dropped_figures.iter().zip(WRONG) {
        assert_eq!(example.proposition, reported(amount));
        assert_eq!(example.stated, amount.trim_start_matches("KRW "));
        assert_eq!(example.passage, Some(chunk));
    }
    let body = fixture.body();
    let published: Vec<&str> = body
        .claims
        .iter()
        .map(|claim| claim.proposition.as_str())
        .collect();
    assert_eq!(published.len(), 2, "{published:?}");
    let evidence = |fact: &str| {
        body.claims
            .iter()
            .find(|claim| claim.proposition == fact)
            .unwrap()
            .evidence[0]
            .clone()
    };
    assert_eq!(
        evidence(REVENUE).figure_check,
        Some(EvidenceFigure::Matched)
    );
    assert_eq!(evidence(REVENUE).passage, Some(chunk));
    // A margin derived from two amounts is published, marked as not found.
    assert_eq!(
        evidence(MARGIN).figure_check,
        Some(EvidenceFigure::NotFound)
    );
}

#[test]
fn a_differing_figure_adds_no_support_to_the_claim_it_repeats() {
    let mut fixture = Fixture::new();
    let fact = reported("KRW 12.34 billion");
    let script = Arc::new(Mutex::new(Script::default()));
    for source in [
        "Hanbit Materials reported a 2025 figure of KRW 12.34 billion.",
        SOURCE,
    ] {
        {
            let mut script = script.lock().unwrap();
            script.reply("ExtractedEntities", entities(&["Hanbit Materials"]));
            script.reply(
                "ExtractedEdges",
                json!({"edges": [relation("Hanbit Materials", "Hanbit Materials", "REPORTS", &fact)]}),
            );
            script.reply("CoreAttributeBatch", json!({"entities": []}));
        }
        let job = fixture.queue("Hanbit Materials", source);
        assert_eq!(
            drive(&mut fixture, &job, &script, "fixture").status,
            graph_jobs::JOB_COMPLETED
        );
    }
    let body = fixture.body();
    assert_eq!(body.claims.len(), 1);
    assert_eq!(body.claims[0].evidence.len(), 1);
    assert_eq!(
        body.claims[0].evidence[0].figure_check,
        Some(EvidenceFigure::Matched)
    );
}

#[test]
fn an_as_of_read_hides_the_figure_check_with_the_evidence_it_hides() {
    use crate::commands::knowledge::as_of::{claim_as_of, Cutoff};
    use crate::knowledge::GraphClaim;
    let mut claim: GraphClaim = serde_json::from_value(serde_json::json!({
        "id": "clm", "proposition": "Revenue was KRW 123.4 billion.",
        "attribution": "publisher", "relation": "REPORTS", "endpoints": [], "status": "active",
        "qualifiers": [],
        "evidence": [
            {"spanId": "early", "role": "extracted_from", "attribution": "publisher",
             "figureCheck": "matched", "passage": {"chunkIndex": 0, "charStart": 0, "charEnd": 40}},
            {"spanId": "later", "role": "supports", "attribution": "publisher",
             "figureCheck": "not_found", "passage": {"chunkIndex": 2, "charStart": 9000, "charEnd": 12000}}
        ],
    }))
    .unwrap();
    let spans = std::collections::BTreeSet::from(["early".to_string()]);
    let cutoff = Cutoff::parse(Some("2026-03-10")).unwrap();
    claim_as_of(&mut claim, &spans, &Default::default(), cutoff);
    let view = serde_json::to_string(&claim).unwrap();
    assert!(view.contains(r#""figureCheck":"matched""#), "{view}");
    assert!(
        !view.contains("not_found") && !view.contains("9000"),
        "{view}"
    );
}
