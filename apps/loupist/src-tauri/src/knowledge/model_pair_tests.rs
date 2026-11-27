//! `decide_pair`: one row per branch, and a seeded property over day sets.
use super::*;

fn time(value: &str) -> TimePoint {
    TimePoint {
        precision: match value.len() {
            4 => TimePrecision::Year,
            7 => TimePrecision::Month,
            10 => TimePrecision::Date,
            _ => TimePrecision::Instant,
        },
        value: Some(value.into()),
        approximate: false,
    }
}

fn approx(value: &str) -> TimePoint {
    TimePoint {
        approximate: true,
        ..time(value)
    }
}

fn witness(span: &str, day: Option<&str>) -> AssertionWitness {
    AssertionWitness {
        span: span.into(),
        publication: day.map(time),
    }
}

/// A claim asserted by one Source per day, `{span}@{day}`.
fn sources(span: &str, days: &[&str]) -> AssertionSources {
    let mut sources = AssertionSources::default();
    for day in days {
        let candidate = witness(&format!("{span}@{day}"), Some(day));
        sources.source_ids.insert(candidate.span.clone());
        if sources
            .witness
            .as_ref()
            .is_none_or(|old| witness_order(&candidate, old).is_lt())
        {
            sources.witness = Some(candidate);
        }
        sources.add_day(&time(day));
    }
    sources
}

struct Side {
    relation: &'static str,
    endpoints: [&'static str; 2],
    start: Option<TimePoint>,
    sources: AssertionSources,
}

fn side(span: &str, days: &[&str]) -> Side {
    Side {
        relation: "LEADS",
        endpoints: ["officer", "company"],
        start: None,
        sources: sources(span, days),
    }
}

impl Side {
    fn assertion(&self) -> Assertion<'_> {
        Assertion {
            relation: self.relation,
            endpoints: self.endpoints,
            valid_at: self.start.as_ref(),
            assertions: &self.sources,
        }
    }

    fn starting(mut self, start: &str) -> Self {
        self.start = Some(time(start));
        self
    }

    fn stating_end(mut self, end: &str) -> Self {
        let witness = self.sources.witness.clone().unwrap();
        self.sources.stated = Some(StatedEnd {
            time: time(end),
            witness,
        });
        self
    }
}

fn decide(left: &Side, right: &Side, same: bool, reaffirmed: &[AssertionWitness]) -> PairOutcome {
    decide_pair(
        left.assertion(),
        right.assertion(),
        PairSelection {
            same_assertion: same,
            reaffirmed_by: reaffirmed,
        },
    )
}

fn reference(left: bool, at: &str) -> PairOutcome {
    PairOutcome::End {
        left,
        time: approx(at),
        basis: EndBasis::Reference,
        restated: None,
    }
}

fn stated(left: bool, at: &str) -> PairOutcome {
    PairOutcome::End {
        left,
        time: time(at),
        basis: EndBasis::Stated,
        restated: None,
    }
}

/// Name, the two sides, same assertion, reaffirming witnesses, outcome.
type Row<'a> = (
    &'a str,
    Side,
    Side,
    bool,
    &'a [AssertionWitness],
    PairOutcome,
);

#[test]
fn every_branch_of_the_pair_rule() {
    use PairOutcome::{Conflict, Independent};
    let x = |days: &[&str]| side("filing-x", days);
    let y = |days: &[&str]| side("filing-y", days);
    let mar = [witness("informe-mar", Some("2026-03-19"))];
    let feb = [witness("bericht-feb", Some("2026-02-12"))];
    let restated_x = PairOutcome::End {
        left: false,
        time: approx("2026-03-19"),
        basis: EndBasis::Reference,
        restated: Some(Restated {
            witness: mar[0].clone(),
            lapsed: vec![Lapse {
                from: approx("2026-02-12"),
                to: approx("2026-03-19"),
            }],
        }),
    };
    let one_source = {
        let mut right = y(&["2026-02-12"]);
        right.sources.source_ids = x(&["2026-01-08"]).sources.source_ids;
        right
    };
    let other_relation = Side {
        relation: "DIRECTS",
        ..y(&["2026-02-12"])
    };
    let other_endpoints = Side {
        endpoints: ["auditor", "fund"],
        ..y(&["2026-02-12"])
    };
    let received = Side {
        sources: AssertionSources {
            source_ids: ["inbox".to_owned()].into(),
            witness: Some(witness("inbox", None)),
            ..Default::default()
        },
        ..y(&[])
    };
    #[rustfmt::skip]
    let rows: Vec<Row<'_>> = vec![
        ("one common Source", x(&["2026-01-08"]), one_source, true, &[], Independent),
        ("stated end before the other start",
            x(&["2026-01-08"]).stating_end("2025-06"), y(&["2026-02-12"]).starting("2025-09"),
            true, &[], Independent),
        ("the other side states the end",
            x(&["2026-01-08"]).starting("2024"), y(&["2026-02-12"]).stating_end("2025-11"),
            false, &[], stated(true, "2025-11")),
        ("this side states the end",
            x(&["2026-01-08"]).stating_end("2025-11"), y(&["2026-02-12"]).starting("2024"),
            false, &[], stated(false, "2025-11")),
        ("two explicit starts", x(&["2026-01-08"]).starting("2024"),
            y(&["2026-01-08"]).starting("2025-08"), false, &[], stated(true, "2025-08")),
        ("two equal explicit starts", x(&["2026-01-08"]).starting("2025"),
            y(&["2026-02-12"]).starting("2025"), true, &[], Conflict),
        ("a stated end is never reopened",
            x(&["2026-01-08", "2026-03-19"]).stating_end("2026-02"), y(&["2026-02-12"]),
            true, &mar, Conflict),
        ("not the same assertion", x(&["2026-01-08"]), y(&["2026-02-12"]), false, &[], Conflict),
        ("another relation", x(&["2026-01-08"]), other_relation, true, &[], Conflict),
        ("no shared endpoint", x(&["2026-01-08"]), other_endpoints, true, &[], Conflict),
        ("received only", x(&["2026-01-08"]), received, true, &[], Conflict),
        ("ordered days", x(&["2026-01-08"]), y(&["2026-02-12"]), true, &[],
            reference(true, "2026-02-12")),
        ("ordered days, reversed", x(&["2026-02-12", "2026-03-19"]), y(&["2026-01-08"]), true,
            &[], reference(false, "2026-02-12")),
        ("same day", x(&["2026-01-08"]), y(&["2026-01-08T21:30:00+09:00"]), true, &mar, Conflict),
        ("a month contains the day", x(&["2026-01"]), y(&["2026-01-08"]), true, &[], Conflict),
        ("interleaved, same last day", x(&["2026-01-08", "2026-03-19"]),
            y(&["2026-02-12", "2026-03-19"]), true, &mar, Conflict),
        ("interleaved, not witnessed", x(&["2026-01-08", "2026-03-19"]), y(&["2026-02-12"]),
            true, &[], Conflict),
        ("interleaved, witnessed only by the other side's day",
            x(&["2026-01-08", "2026-03-19"]), y(&["2026-02-12"]), true,
            &feb, Conflict),
        ("interleaved, witnessed", x(&["2026-01-08", "2026-03-19"]), y(&["2026-02-12"]), true,
            &mar, restated_x),
    ];
    for (name, left, right, same, reaffirmed, expected) in rows {
        assert_eq!(decide(&left, &right, same, reaffirmed), expected, "{name}");
    }
}

#[test]
fn the_source_that_ended_a_claim_does_not_also_disagree_with_it() {
    let ending = Ending::paired(
        approx("2026-03-19"),
        EndBasis::Reference,
        "corrected",
        &sources("restatement", &["2026-03-19"]),
    )
    .unwrap();
    let also = {
        let mut both = sources("restatement", &["2026-03-19"]);
        both.source_ids
            .extend(sources("wire", &["2026-04-02"]).source_ids);
        both
    };
    let received = AssertionSources {
        source_ids: ["restatement@2026-03-19".to_owned()].into(),
        witness: Some(witness("restatement@2026-03-19", None)),
        ..Default::default()
    };
    #[rustfmt::skip]
    let rows = [
        ("only the ending's Source", sources("restatement", &["2026-03-19"]), true),
        ("that Source, received", received, true),
        ("that Source and another", also, false),
        ("another Source", sources("wire", &["2026-03-19"]), false),
        ("no Source", AssertionSources::default(), false),
    ];
    for (name, other, expected) in rows {
        assert_eq!(ending.witnessed_only_by(&other), expected, "{name}");
    }
}

#[test]
fn an_older_claim_starts_from_its_witness_day() {
    let mut legacy = AssertionSources {
        witness: Some(witness("kvartal-jan", Some("2026-01-08"))),
        ..Default::default()
    };
    assert_eq!(legacy.days(), [&time("2026-01-08")]);
    legacy.add_day(&time("2026-03-19T08:00:00+01:00"));
    legacy.add_day(&time("2026-03-19T07:00:00+01:00"));
    assert_eq!(
        legacy.days,
        [time("2026-01-08"), time("2026-03-19T07:00:00+01:00")]
    );
}

/// SplitMix64: a dependency-free generator whose seed reproduces a case.
struct Seeded(u64);

impl Seeded {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn shuffled<T: Clone>(&mut self, items: &[T]) -> Vec<T> {
        let mut items = items.to_vec();
        for index in (1..items.len()).rev() {
            items.swap(index, self.below(index + 1));
        }
        items
    }
}

const DAYS: [&str; 9] = [
    "2025",
    "2026-01",
    "2026-01-08",
    "2026-01-08T23:30:00-05:00",
    "2026-02-12",
    "2026-03-19",
    "2026-03-19T06:00:00+09:00",
    "2026-05-02",
    "2026-07-30",
];

fn mirrored(outcome: PairOutcome) -> PairOutcome {
    match outcome {
        PairOutcome::End {
            left,
            time,
            basis,
            restated,
        } => PairOutcome::End {
            left: !left,
            time,
            basis,
            restated,
        },
        other => other,
    }
}

#[test]
fn the_pair_rule_is_symmetric_and_ignores_the_order_days_arrive() {
    let mut seen = BTreeSet::new();
    for seed in 0..2000u64 {
        let mut rng = Seeded(seed);
        let pick = |rng: &mut Seeded| -> Vec<&'static str> {
            (0..1 + rng.below(4))
                .map(|_| DAYS[rng.below(DAYS.len())])
                .collect()
        };
        let left_days = pick(&mut rng);
        let right_days = pick(&mut rng);
        let reaffirmed_days = pick(&mut rng);
        let mut reaffirmed = Vec::new();
        let mut link = AssertionLink {
            left: "x".into(),
            right: "y".into(),
            same_assertion: true,
            selected_by: None,
            same_assertion_by: None,
            reaffirmed_by: Vec::new(),
        };
        for day in reaffirmed_days.iter().take(rng.below(4)) {
            reaffirmed.push(witness(&format!("restated@{day}"), Some(day)));
        }
        for item in rng.shuffled(&reaffirmed) {
            link.reaffirm(&item);
        }
        let mut again = link.clone();
        again.reaffirmed_by.clear();
        for item in rng.shuffled(&reaffirmed) {
            again.reaffirm(&item);
        }
        assert_eq!(again.reaffirmed_by, link.reaffirmed_by, "seed {seed}");

        let left = side("filing-x", &left_days);
        let shuffled = side("filing-x", &rng.shuffled(&left_days));
        assert_eq!(left.sources.days, shuffled.sources.days, "seed {seed}");
        let right = side("filing-y", &right_days);
        let forward = decide(&left, &right, true, &link.reaffirmed_by);
        assert_eq!(
            decide(
                &shuffled,
                &side("filing-y", &rng.shuffled(&right_days)),
                true,
                &link.reaffirmed_by
            ),
            forward,
            "seed {seed}: {left_days:?} {right_days:?}"
        );
        assert_eq!(
            mirrored(decide(&right, &left, true, &link.reaffirmed_by)),
            forward,
            "seed {seed}: {left_days:?} {right_days:?} {reaffirmed_days:?}"
        );
        seen.insert(match &forward {
            PairOutcome::End {
                restated: Some(_), ..
            } => "restated",
            PairOutcome::End { .. } => "ordered",
            PairOutcome::Conflict => "conflict",
            PairOutcome::Independent => "independent",
        });
        // An ending by reference never lands before the ended side's last day.
        if let PairOutcome::End {
            left: ended_left,
            time,
            ..
        } = &forward
        {
            let (ended, ender) = if *ended_left {
                (&left, &right)
            } else {
                (&right, &left)
            };
            let last = *ended.sources.days().last().unwrap();
            assert_eq!(day_order(last, time), Some(Ordering::Less), "seed {seed}");
            // The Source witnessing the ending, alone on a third claim, drops
            // that claim's conflict link; with any other Source it keeps it.
            let PairOutcome::End {
                time,
                basis,
                restated,
                ..
            } = &forward
            else {
                unreachable!()
            };
            let mut ending = Ending::paired(time.clone(), *basis, "ender", &ender.sources).unwrap();
            if let Some(restated) = restated {
                ending.span = restated.witness.span.clone();
            }
            let witness_day = ending.span.split_once('@').unwrap().1;
            let third = sources(ending.span.split_once('@').unwrap().0, &[witness_day]);
            assert!(ending.witnessed_only_by(&third), "seed {seed}");
            let mut shared = third.clone();
            shared.source_ids.insert("wire@2026-07-30".into());
            assert!(!ending.witnessed_only_by(&shared), "seed {seed}");
            let other_day = DAYS[rng.below(DAYS.len())];
            let other = sources("filing-z", &[other_day]);
            assert!(!ending.witnessed_only_by(&other), "seed {seed}");
        }
    }
    assert_eq!(
        seen,
        BTreeSet::from(["conflict", "ordered", "restated"]),
        "the generator reaches every reference branch"
    );
}
