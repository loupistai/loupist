use super::*;
use serde::Deserialize;
use std::path::Path;

/// One labelled case of `evals/world/figure-corpus-v1.json`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    profile: String,
    passage: String,
    proposition: String,
    expect: FigureVerdict,
}

const CORPUS: &str = "../evals/world/figure-corpus-v1.json";

/// Formats the corpus labels `unchecked` because plan 4.1 left them out,
/// which the reader has read since (兆, TEUR): their labels can flip.
const READ_SINCE_LABELLED: &[(&str, FigureVerdict)] = &[
    ("F-JP-05", FigureVerdict::Matched),
    ("F-DE-08", FigureVerdict::Rounded),
];

fn corpus() -> Vec<Case> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(CORPUS);
    let text = std::fs::read_to_string(&path).expect("the figure corpus is in the tree");
    serde_json::from_str(&text).expect("figure corpus parses")
}

/// Loop C's thresholds: no `differs` on a case not labelled so, at least 90%
/// of planted errors caught, and `unchecked` for every unrecognized format.
#[test]
fn figure_corpus_meets_its_thresholds() {
    let mut cases = corpus();
    let mut table = String::from("id | profile | expect | got\n");
    let (mut false_differs, mut planted, mut caught, mut misses) = (0, 0, 0, Vec::new());
    let mut unrecognized_read = Vec::new();
    for case in &mut cases {
        if let Some((_, verdict)) = READ_SINCE_LABELLED.iter().find(|(id, _)| *id == case.id) {
            case.expect = *verdict;
        }
    }
    for case in &cases {
        let got = check(&case.proposition, &[&case.passage]).verdict;
        table.push_str(&format!(
            "{} | {} | {:?} | {:?}\n",
            case.id, case.profile, case.expect, got
        ));
        if got != case.expect {
            misses.push(case.id.as_str());
        }
        if got == FigureVerdict::Differs && case.expect != FigureVerdict::Differs {
            false_differs += 1;
        }
        if case.expect == FigureVerdict::Differs {
            planted += 1;
            caught += usize::from(got == FigureVerdict::Differs);
        }
        if case.expect == FigureVerdict::Unchecked && got != FigureVerdict::Unchecked {
            unrecognized_read.push(case.id.as_str());
        }
    }
    println!("{table}");
    println!("planted {planted}, caught {caught}, misses {misses:?}");
    assert_eq!(
        false_differs, 0,
        "a case not labelled differs read as differs\n{table}"
    );
    assert!(
        unrecognized_read.is_empty(),
        "a format the reader does not know read as checked: {unrecognized_read:?}\n{table}"
    );
    assert!(
        caught * 10 >= planted * 9,
        "planted errors caught {caught}/{planted}\n{table}"
    );
    let profiles: std::collections::BTreeSet<_> = cases.iter().map(|case| &case.profile).collect();
    assert!(
        profiles.len() >= 4,
        "the corpus draws on at least four profiles"
    );
}

#[test]
fn a_power_of_ten_is_dropped_and_the_stated_magnitude_matches() {
    let passage = "한빛소재의 2025년 매출액은 1,234억 원이다.";
    let wrong = check("Revenue was KRW 12.34 billion.", &[passage]);
    assert_eq!(wrong.verdict, FigureVerdict::Differs);
    assert_eq!(wrong.figures[0].cited.as_deref(), Some("1,234억 원"));
    assert_eq!(
        check("Revenue was KRW 123.4 billion.", &[passage]).verdict,
        FigureVerdict::Matched
    );
    assert_eq!(
        check("The share price was £12.34.", &["It closed at 1,234p."]).verdict,
        FigureVerdict::Matched
    );
    assert_eq!(
        check(
            "Revenue was INR 25 million.",
            &["Revenue stood at ₹2.5 crore."]
        )
        .verdict,
        FigureVerdict::Matched
    );
}

#[test]
fn a_derived_figure_is_not_found_and_a_later_passage_can_find_it() {
    let chunk = "Revenue rose from USD 100 million to USD 120 million.";
    assert_eq!(
        check("Revenue grew 20%.", &[chunk]).verdict,
        FigureVerdict::NotFound
    );
    let source = format!("{chunk}\n\nThe 20% growth was the fastest in a decade.");
    assert_eq!(
        check("Revenue grew 20%.", &[chunk, &source]).verdict,
        FigureVerdict::Matched
    );
}

#[test]
fn years_dates_ordinals_and_codes_are_not_figures() {
    let fact = check(
        "Hanbit Materials (KRX 990221) filed its 3rd quarterly report on 2026-03-18 for FY2026.",
        &["Nothing here."],
    );
    assert_eq!(fact.verdict, FigureVerdict::Unchecked);
    assert!(fact.figures.is_empty());
}

#[test]
fn a_fact_takes_the_worst_of_its_figures() {
    let passage = "Revenue was $4.82 billion and the margin was 12.0%.";
    let fact = check(
        "Revenue was USD 482 million with a 12.0% margin.",
        &[passage],
    );
    assert_eq!(fact.verdict, FigureVerdict::Differs);
    assert_eq!(fact.figures[1].verdict, FigureVerdict::Matched);
}

#[test]
fn an_ambiguous_separator_never_differs() {
    // No other number and no scale word says whether `1.234` is a decimal.
    let fact = check("The fee was EUR 1,234.", &["Die Gebühr betrug 1.234 Euro."]);
    assert_ne!(fact.verdict, FigureVerdict::Differs);
}

#[test]
fn a_leading_zero_is_a_decimal_in_either_convention() {
    let fact = check(
        "The dividend is GBP 0.125 per share.",
        &["A dividend of 12.5p per share."],
    );
    assert_eq!(fact.verdict, FigureVerdict::Matched);
    let fact = check("The fee is EUR 0.125.", &["Die Gebühr beträgt 0,125 Euro."]);
    assert_eq!(fact.verdict, FigureVerdict::Matched);
}

#[test]
fn a_long_source_is_scanned_once_for_all_the_figures_a_chunk_misses() {
    use super::super::numbers::{FigurePassage, TEXT_SCANS};
    // About 1 MB of filing lines, none printing the checked figures.
    let mut source = String::new();
    let mut line = 0u64;
    while source.len() < 1_000_000 {
        source.push_str(&format!(
            "Segment {line}: revenue of USD {}.{} million and margin of {}.5%.\n",
            100 + line % 800,
            line % 10,
            line % 40
        ));
        line += 1;
    }
    let chunk = FigurePassage::new("Example Corp. reported its half-year results.");
    let whole = FigurePassage::new(&source);
    let passages: Vec<&dyn Passage> = vec![&chunk, &whole];
    let before = TEXT_SCANS.with(|scans| scans.get());
    for fact in 0..40 {
        let fact = format!(
            "Revenue was USD {}.25 billion and the margin was {}.75%.",
            fact, fact
        );
        let verdict = check_in(&fact, &passages).verdict;
        assert_ne!(verdict, FigureVerdict::Matched, "{fact}");
    }
    let read = TEXT_SCANS.with(|scans| scans.get()) - before;
    // One scan for amounts and one for other figures, each over chunk and Source.
    assert!(
        read <= 2 * (source.len() + 64),
        "{read} bytes read for 80 figures over a {} byte Source",
        source.len()
    );
}

#[test]
fn unit_lines_state_their_scale_in_each_profiles_words() {
    use super::super::numbers::Unit;
    for (line, unit) in [
        ("R$ milhões", Unit::Amount(1e6)),
        ("R$ bilhões", Unit::Amount(1e9)),
        ("Mio. EUR", Unit::Amount(1e6)),
        ("EUR Mrd.", Unit::Amount(1e9)),
        ("₹ crore", Unit::Amount(1e7)),
        ("₹ lakh", Unit::Amount(1e5)),
        ("GBX", Unit::Amount(0.01)),
    ] {
        assert_eq!(Unit::of_line(Some(line)), Ok(unit), "{line}");
    }
    // A thousand in Portuguese and Spanish, a million in English.
    assert!(Unit::of_line(Some("R$ mil")).is_err());
    for (passage, proposition) in [
        (
            "Receita líquida (R$ milhões) | 3.412,5",
            "Net revenue was BRL 3,412.5 million.",
        ),
        (
            "(em R$ milhões)\nReceita líquida | 3.412,5",
            "Net revenue was BRL 3,412.5 million.",
        ),
        (
            "Umsatzerlöse (Mio. EUR) | 345,6",
            "Revenue was EUR 345.6 million.",
        ),
        (
            "Revenue (₹ crore) | 1,245.6",
            "Revenue was INR 12.456 billion.",
        ),
        (
            "(₹ in crore)\nRevenue | 1,245.6",
            "Revenue was INR 12.456 billion.",
        ),
        (
            "Share price at period end (GBX) | 1,234",
            "The share price was GBP 12.34.",
        ),
    ] {
        assert_eq!(
            check(proposition, &[passage]).verdict,
            FigureVerdict::Matched,
            "{passage} / {proposition}"
        );
    }
    let thousands = check(
        "Net revenue was BRL 3.4125 billion.",
        &["Receita líquida (R$ mil) | 3.412.500"],
    );
    assert_ne!(thousands.verdict, FigureVerdict::Differs, "{thousands:?}");
}

#[test]
fn a_compound_scale_without_a_space_and_a_trillion_read_as_one_amount() {
    for (passage, proposition) in [
        (
            "営業利益は86億5,000万円だった。",
            "Operating profit was JPY 8.65 billion.",
        ),
        (
            "売上高を1兆2,000億円と予想している。",
            "Revenue is forecast at JPY 1.2 trillion.",
        ),
        ("營業收入為3兆2,000億元。", "Revenue was CNY 3.2 trillion."),
    ] {
        assert_eq!(
            check(proposition, &[passage]).verdict,
            FigureVerdict::Matched,
            "{passage} / {proposition}"
        );
    }
}

#[test]
fn thousands_of_euros_read_with_a_decimal_comma() {
    let passage = "Der Personalaufwand betrug TEUR 84.312.";
    assert_eq!(
        check("Personnel expenses were EUR 84.3 million.", &[passage]).verdict,
        FigureVerdict::Rounded
    );
    assert_eq!(
        check("Personnel expenses were EUR 8.4312 million.", &[passage]).verdict,
        FigureVerdict::Differs
    );
}

#[test]
fn an_ended_figure_reads_its_direction_only_when_the_two_compare() {
    use super::FigureDirection::*;
    let cases: &[(&str, &str, Option<super::FigureDirection>)] = &[
        (
            "Northwind guides full-year revenue of USD 64,000 million.",
            "Northwind guides full-year revenue of USD 58,000 million.",
            Some(Lower),
        ),
        (
            "한빛소재는 연간 매출 가이던스를 5,800억 원으로 제시했다.",
            "한빛소재는 연간 매출 가이던스를 6.4조 원으로 올렸다.",
            Some(Higher),
        ),
        (
            "Lindenhof erwartet einen Umsatz von 1,2 Mrd. EUR.",
            "Lindenhof erwartet einen Umsatz von 1.200 Mio. EUR.",
            Some(Same),
        ),
        (
            "The dividend is GBP 0.12 per share.",
            "The dividend is 15p per share.",
            Some(Higher),
        ),
        // Two amounts each, a percentage against an amount, two currencies,
        // per share against a total: the direction cannot be read.
        (
            "Revenue of USD 64,000 million and margin of 12%.",
            "Revenue of USD 58,000 million and margin of 11%.",
            None,
        ),
        (
            "Margin guidance is 12%.",
            "Revenue guidance is USD 58,000 million.",
            None,
        ),
        (
            "Revenue guidance is JPY 64 billion.",
            "Revenue guidance is USD 58 billion.",
            None,
        ),
        (
            "EPS guidance is USD 6.40 per share.",
            "Net income guidance is USD 640 million.",
            None,
        ),
    ];
    for (from, to, direction) in cases {
        let change = super::figure_change(from, to);
        assert_eq!(change.direction, *direction, "{from} -> {to}: {change:?}");
        assert_eq!(change.comparable, direction.is_some(), "{from} -> {to}");
    }
    let change = super::figure_change(
        "Northwind guides full-year revenue of USD 64,000 million.",
        "Northwind guides full-year revenue of USD 58,000 million.",
    );
    assert_eq!(change.from.as_deref(), Some("64,000 million"));
    assert_eq!(change.to.as_deref(), Some("58,000 million"));
}
