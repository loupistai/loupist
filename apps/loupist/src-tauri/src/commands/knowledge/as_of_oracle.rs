//! The cutoff oracle (master plan D2-2, extension plan 6.1).
//!
//! Reference, written from the design: under a cutoff `c` a reader returns what
//! the same reader returns on the **prefix World**, a World that ingested only
//! the Sources known by `c` (published by then on the user's clock, or the
//! user's own undated statement saved by then). The generator builds small
//! Worlds from a seed and ingests each Source through the publication path the
//! extraction worker uses (`seal_world_from_source`), so episodes, growth
//! events and Lens follow-ups are the ones production writes. The resolver's
//! pair rule (`decide_pair`) is the given ingestion rule; the supersession
//! oracle in `world_extract/tests/temporal.rs` owns it. This oracle owns what
//! every reader shows of that World under a cutoff.
//!
//! Properties, for every row of [`READERS`]:
//! - P1: the reader on the full World under `c` equals the reader on the prefix
//!   World under `c`, after ids are normalized. The prefix World is read under
//!   `c` too, so a fact whose effective date is after `c` is judged by the
//!   same clock on both sides; P1b checks that clock against a reference.
//! - P2: no canary of a Source unknown by `c` (or removed by a Memory reset)
//!   appears in the serialized result or in its receipt.
//! - P3: every ingestion order gives the same World view.
//! - P4: paging a feed to its end yields each row once, in the single-page order.
//!
//! `LOUPIST_ORACLE_SEEDS` sets the seeds per property (default 200), and
//! `LOUPIST_ORACLE_SEED` runs one seed. A failure prints every failing seed
//! with the World it generated. `LOUPIST_ORACLE_DATES_ONLY=1` generates no
//! near-midnight instants, to see what a run finds beside them.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, TimeZone};
use serde_json::{json, Value};

use crate::app::AppState;
use crate::broker::{ToolExecutor, ToolMethod, ToolPolicy};
use crate::commands::tool_executor::{AckedPayload, AppToolExecutor, ToolCapture};
use crate::knowledge::world_episodes::{
    EpisodeChanges, EpisodeModel, ReferenceBasis, WorldEpisode,
};
use crate::knowledge::{
    decide_pair, ending_order, reopening_qualifiers, witness_order, Assertion, AssertionLink,
    AssertionSources, AssertionWitness, Authorship, BindingRole, ClaimEndpoint, ClaimStatus,
    Coverage, CreateRecord, DependencyRole, Ending, EvidenceBinding, EvidenceRevision, GraphClaim,
    GraphNode, KnowledgeStore, PairEffects, PairSelection, Qualifier, RecordKind,
    RepresentationRole, RetainMaterial, RetainNewSource, RetainedSource, SealWorld, Selector,
    SourceClass, StatedEnd, TimePoint, TimePrecision, WorldTemporal, CONFLICTS_WITH_CLAIM,
    END_BASIS, LAPSED, REOPENED_AT, SUPERSEDED_BY_CLAIM, SUPERSEDED_BY_SPAN,
};

pub(in crate::commands) const LOUPIST: &str = "loupist-a";
const SESSION: &str = "chat-oracle";
/// A word every proposition and Source text carries, so one search reaches all.
pub(in crate::commands) const SEARCH_WORD: &str = "ledger";

// ---------------------------------------------------------------------------
// Seeds
// ---------------------------------------------------------------------------

/// splitmix64: small, in-tree, reproducible.
#[derive(Clone)]
pub(in crate::commands) struct Rng(u64);

impl Rng {
    pub(in crate::commands) fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    pub(in crate::commands) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    pub(in crate::commands) fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next() % n as u64) as usize
    }

    pub(in crate::commands) fn chance(&mut self, numerator: u64, denominator: u64) -> bool {
        self.next() % denominator < numerator
    }

    pub(in crate::commands) fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    pub(in crate::commands) fn shuffle<T>(&mut self, items: &mut [T]) {
        for index in (1..items.len()).rev() {
            let other = self.below(index + 1);
            items.swap(index, other);
        }
    }
}

/// The seeds a property runs: `LOUPIST_ORACLE_SEED` alone, or the first
/// `LOUPIST_ORACLE_SEEDS` (default 200) of the property's own sequence.
pub(in crate::commands) fn seeds(property: &str) -> Vec<u64> {
    if let Some(seed) = std::env::var("LOUPIST_ORACLE_SEED")
        .ok()
        .and_then(|value| value.parse().ok())
    {
        return vec![seed];
    }
    let count: u64 = std::env::var("LOUPIST_ORACLE_SEEDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(200);
    let base = property
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
        % 1_000_000;
    (0..count).map(|index| base * 10_000 + index).collect()
}

/// Runs `check` for every seed and fails once with each failing seed and its
/// first difference, so a report can list them.
pub(in crate::commands) fn for_each_seed(
    property: &str,
    check: impl Fn(u64) -> Result<(), String> + Sync,
) {
    let seeds = seeds(property);
    let workers = std::thread::available_parallelism()
        .map_or(4, |count| count.get())
        .clamp(1, 8);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let found = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(seed) = seeds.get(index).copied() else {
                    break;
                };
                let outcome =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(seed)));
                let failure = match outcome {
                    Ok(Ok(())) => None,
                    Ok(Err(message)) => Some(message),
                    Err(panic) => Some(format!(
                        "panicked: {}",
                        panic
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                            .unwrap_or_else(|| "panic".into())
                    )),
                };
                if let Some(message) = failure {
                    found.lock().unwrap().push((seed, message));
                }
            });
        }
    });
    let mut failures = found.into_inner().unwrap();
    failures.sort_by_key(|(seed, _)| *seed);
    if !failures.is_empty() {
        for (seed, message) in &mut failures {
            message.push_str("\nWorld:\n");
            message.push_str(&Spec::generate(*seed).describe());
        }
        let seeds: Vec<String> = failures.iter().map(|(seed, _)| seed.to_string()).collect();
        let mut report = format!(
            "{property}: {} failing seeds: {}\n",
            failures.len(),
            seeds.join(" ")
        );
        for (seed, message) in failures.iter().take(6) {
            let message: String = message.chars().take(9_000).collect();
            report.push_str(&format!("--- seed {seed}\n{message}\n"));
        }
        panic!("{report}");
    }
}

// ---------------------------------------------------------------------------
// The generator
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub(in crate::commands) struct SourceSpec {
    pub canary: String,
    pub title: String,
    pub url: String,
    pub text: String,
    /// `None` is a received-basis Source: only a user's own statement.
    pub publication: Option<TimePoint>,
    pub authorship: Authorship,
    pub received_at_ms: i64,
    pub ingest_at_ms: i64,
    /// The publication the episode row stored, when the capture was later
    /// corrected; `None` is the capture's own.
    pub episode_publication: Option<TimePoint>,
    /// Its support of every claim stored `figure_check` not_found, so a World
    /// read and a hit name `figure: "not_found"`.
    pub figure_not_found: bool,
}

#[derive(Clone, Debug)]
pub(in crate::commands) struct ClaimSpec {
    pub id: String,
    pub relation: String,
    pub endpoints: [usize; 2],
    pub proposition: String,
    pub asserted_by: Vec<usize>,
    /// A Source that asserts the claim and states its end.
    pub stated_end: Option<(usize, TimePoint)>,
}

#[derive(Clone, Debug)]
pub(in crate::commands) struct LinkSpec {
    pub left: usize,
    pub right: usize,
    pub selected_by: usize,
    /// The Source whose episode marked the pair the same assertion.
    pub same_by: Option<usize>,
    /// Later Sources whose episodes marked it the same assertion again: a
    /// restatement after the other side's last day counts only with one.
    pub also_same_by: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(in crate::commands) struct StatusSpec {
    pub claim: usize,
    pub status: ClaimStatus,
    pub witness: usize,
}

#[derive(Clone, Debug)]
pub(in crate::commands) struct LensSpec {
    pub at_ms: i64,
    pub sections: Vec<(String, Vec<usize>)>,
    /// Section text that quotes a Source; the default is the user's own words.
    pub quote: Option<String>,
    /// Filed by an agent turn rather than typed by the user.
    pub by_turn: bool,
    /// Sources whose span a section cites beside its claims, by section key.
    pub spans: Vec<(String, usize)>,
}

/// A later capture of a Source's record: a new title, URL and publication.
#[derive(Clone, Debug)]
pub(in crate::commands) struct Recapture {
    pub source: usize,
    pub title: String,
    pub url: String,
    pub publication: TimePoint,
    pub at_ms: i64,
}

#[derive(Clone, Debug)]
pub(in crate::commands) struct Spec {
    pub seed: u64,
    pub nodes: Vec<GraphNode>,
    pub sources: Vec<SourceSpec>,
    pub claims: Vec<ClaimSpec>,
    pub links: Vec<LinkSpec>,
    pub statuses: Vec<StatusSpec>,
    pub lens: Option<LensSpec>,
    /// A Source a Memory reset removed after ingestion.
    pub reset: Option<usize>,
    pub recaptures: Vec<Recapture>,
    pub cutoffs: Vec<NaiveDate>,
}

/// Fictional companies and people from several markets and languages.
const COMPANIES: &[&str] = &[
    "Nordlicht Werke AG",
    "Maré Alta Energia S.A.",
    "Kōyō Seimitsu K.K.",
    "Sahel Agro Holdings Ltd",
    "Llanura Minera S.A.B. de C.V.",
    "Tasman Fibre Pty Ltd",
    "Hanbit Materials Co., Ltd.",
    "Vltava Rail a.s.",
];
const PEOPLE: &[&str] = &[
    "Adaeze Okafor",
    "Märta Lindqvist",
    "Ryōta Takeda",
    "Samira Haddad",
    "Joaquín Ferreyra",
    "Priya Raghunathan",
];
const TITLES: &[&str] = &[
    "Quarterly report",
    "Relatório trimestral",
    "Quartalsbericht",
    "四半期報告書",
    "Informe trimestral",
    "Rapport annuel",
    "분기보고서",
    "Press release",
];
const RELATIONS: &[&str] = &["CFO_OF", "CEO_OF", "GUIDES_REVENUE_OF"];
/// Written offsets of instants near midnight: several zones, half hours included.
const OFFSETS: &[i32] = &[
    0,
    9 * 3600,
    -5 * 3600,
    5 * 3600 + 1800,
    -3 * 3600,
    13 * 3600 + 2700,
    3600,
];

pub(in crate::commands) fn date(value: &str) -> TimePoint {
    TimePoint {
        precision: TimePrecision::Date,
        value: Some(value.into()),
        approximate: false,
    }
}

fn instant_at(at: DateTime<FixedOffset>) -> TimePoint {
    TimePoint {
        precision: TimePrecision::Instant,
        value: Some(at.to_rfc3339()),
        approximate: false,
    }
}

pub(in crate::commands) fn local_ms(day: NaiveDate, hour: u32, minute: u32) -> i64 {
    let naive = day.and_hms_opt(hour, minute, 0).unwrap();
    let offset = *crate::local_time::now().offset();
    offset
        .from_local_datetime(&naive)
        .single()
        .unwrap()
        .timestamp_millis()
}

fn base_day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 1, 5).unwrap()
}

/// Two companies and two people, for a hand-made Spec.
pub(in crate::commands) fn fixed_nodes() -> Vec<GraphNode> {
    let node = |id: &str, name: &str, kind: &str| GraphNode {
        id: id.into(),
        name: name.into(),
        aliases: Vec::new(),
        description: String::new(),
        kind: kind.into(),
        source_role: "subject".into(),
        properties: BTreeMap::new(),
    };
    vec![
        node("node-c0", COMPANIES[0], "company"),
        node("node-c1", COMPANIES[2], "company"),
        node("node-p0", PEOPLE[0], "person"),
        node("node-p1", PEOPLE[3], "person"),
    ]
}

/// A Source for a hand-made Spec, read one hour after it was received.
pub(in crate::commands) fn source(
    canary: &str,
    authorship: Authorship,
    published: Option<&str>,
    received: &str,
) -> SourceSpec {
    let received_day = NaiveDate::parse_from_str(received, "%Y-%m-%d").unwrap();
    let received_at_ms = local_ms(received_day, 10, 0);
    SourceSpec {
        canary: canary.into(),
        title: format!("{} {canary}", TITLES[0]),
        url: format!("https://filings.example.test/{canary}"),
        text: format!("{SEARCH_WORD} passage {canary} with figures"),
        publication: published.map(date),
        authorship,
        received_at_ms,
        ingest_at_ms: received_at_ms + 3_600_000,
        episode_publication: None,
        figure_not_found: false,
    }
}

/// A claim between `endpoints` (indices into [`fixed_nodes`]).
pub(in crate::commands) fn claim(
    id: &str,
    endpoints: [usize; 2],
    asserted_by: &[usize],
) -> ClaimSpec {
    let nodes = fixed_nodes();
    ClaimSpec {
        id: id.into(),
        relation: "CFO_OF".into(),
        endpoints,
        proposition: format!(
            "{} CFO_OF {} ({SEARCH_WORD} {id})",
            nodes[endpoints[0]].name, nodes[endpoints[1]].name
        ),
        asserted_by: asserted_by.to_vec(),
        stated_end: None,
    }
}

impl Spec {
    /// A hand-made World: the named tests and the taint oracle use it.
    pub(in crate::commands) fn manual(sources: Vec<SourceSpec>, claims: Vec<ClaimSpec>) -> Self {
        Self {
            seed: 0,
            nodes: fixed_nodes(),
            sources,
            claims,
            links: Vec::new(),
            statuses: Vec::new(),
            lens: None,
            reset: None,
            recaptures: Vec::new(),
            cutoffs: Vec::new(),
        }
    }

    pub(in crate::commands) fn generate(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let mut nodes = Vec::new();
        let company_count = 2 + rng.below(2);
        let mut names: Vec<&str> = COMPANIES.to_vec();
        rng.shuffle(&mut names);
        for (index, name) in names.iter().take(company_count).enumerate() {
            nodes.push(GraphNode {
                id: format!("node-c{index}"),
                name: (*name).into(),
                aliases: Vec::new(),
                description: String::new(),
                kind: "company".into(),
                source_role: "subject".into(),
                properties: BTreeMap::new(),
            });
        }
        let mut people: Vec<&str> = PEOPLE.to_vec();
        rng.shuffle(&mut people);
        for (index, name) in people.iter().take(3).enumerate() {
            nodes.push(GraphNode {
                id: format!("node-p{index}"),
                name: (*name).into(),
                aliases: Vec::new(),
                description: String::new(),
                kind: "person".into(),
                source_role: "subject".into(),
                properties: BTreeMap::new(),
            });
        }
        let source_count = 3 + rng.below(4);
        let mut sources = Vec::new();
        let mut taken: Vec<NaiveDate> = Vec::new();
        for index in 0..source_count {
            let canary = format!("cnry{seed:x}q{index}z");
            // Days at least three apart: an instant's user day can move one
            // either way, and two Sources of one day tie on their witness, whose
            // tie-break is the span id each store mints.
            let mut day = base_day() + Duration::days(rng.below(150) as i64);
            while taken
                .iter()
                .any(|other| (*other - day).num_days().abs() < 3)
            {
                day += Duration::days(3);
            }
            taken.push(day);
            let user = rng.chance(1, 5);
            // `LOUPIST_ORACLE_DATES_ONLY=1` leaves instants out, to see what a
            // run finds beside the near-midnight cases.
            let dates_only =
                std::env::var("LOUPIST_ORACLE_DATES_ONLY").is_ok_and(|value| value == "1");
            let publication = match rng.below(if user { 4 } else { 3 }) {
                // A publisher's page without a date: read, never known by a cutoff.
                _ if !user && rng.chance(1, 12) => None,
                0 => Some(date(&day.to_string())),
                1 if dates_only => Some(date(&day.to_string())),
                1 => {
                    // Near midnight in another zone, so the user's date can differ
                    // from the date written in the value.
                    let offset = FixedOffset::east_opt(*rng.pick(OFFSETS)).unwrap();
                    let minute = *rng.pick(&[5u32, 30, 45, 55]);
                    let hour = *rng.pick(&[0u32, 23]);
                    let at = offset
                        .from_local_datetime(&day.and_hms_opt(hour, minute, 0).unwrap())
                        .single()
                        .unwrap();
                    Some(instant_at(at))
                }
                2 => Some(date(&day.to_string())),
                _ => None,
            };
            let published_ms = publication
                .as_ref()
                .and_then(reference_instant_ms)
                .unwrap_or_else(|| local_ms(day, 12, 0));
            let received_at_ms = published_ms + rng.below(3) as i64 * 86_400_000 + 60_000;
            // Most are read within days; some are a backfill months later.
            let delay = if rng.chance(1, 4) {
                (60 + rng.below(120)) as i64 * 86_400_000
            } else {
                rng.below(5) as i64 * 86_400_000
            };
            let ingest_at_ms = received_at_ms + delay + rng.below(3_600_000) as i64;
            let title = format!("{} {canary}", rng.pick(TITLES));
            sources.push(SourceSpec {
                url: format!("https://filings.example.test/{canary}"),
                text: format!("{SEARCH_WORD} passage {canary} with figures"),
                title,
                canary,
                publication,
                authorship: if user {
                    Authorship::User
                } else {
                    Authorship::Publisher
                },
                received_at_ms,
                ingest_at_ms,
                episode_publication: None,
                figure_not_found: false,
            });
        }
        let claim_count = 2 + rng.below(4);
        let mut claims = Vec::new();
        for index in 0..claim_count {
            let relation = (*rng.pick(RELATIONS)).to_owned();
            let company = rng.below(company_count);
            let other = if relation == "GUIDES_REVENUE_OF" {
                (company + 1) % company_count
            } else {
                company_count + rng.below(3)
            };
            let mut asserted_by = vec![rng.below(source_count)];
            while rng.chance(1, 3) && asserted_by.len() < source_count {
                let next = rng.below(source_count);
                if !asserted_by.contains(&next) {
                    asserted_by.push(next);
                }
            }
            let stated_end = rng.chance(1, 5).then(|| {
                let by = *rng.pick(&asserted_by);
                let day = base_day() + Duration::days(rng.below(180) as i64);
                (by, date(&day.to_string()))
            });
            claims.push(ClaimSpec {
                id: format!("cl{index}"),
                proposition: format!(
                    "{} {} {} ({SEARCH_WORD} {index})",
                    nodes[other].name, relation, nodes[company].name
                ),
                relation,
                endpoints: [other, company],
                asserted_by,
                stated_end,
            });
        }
        let mut links = Vec::new();
        for left in 0..claim_count {
            for right in left + 1..claim_count {
                let shared = claims[left]
                    .endpoints
                    .iter()
                    .any(|end| claims[right].endpoints.contains(end));
                let same_relation = claims[left].relation == claims[right].relation;
                let selected = if shared && same_relation {
                    rng.chance(2, 3)
                } else {
                    rng.chance(1, 8)
                };
                if !selected {
                    continue;
                }
                let witnesses: Vec<usize> = claims[left]
                    .asserted_by
                    .iter()
                    .chain(&claims[right].asserted_by)
                    .copied()
                    .collect();
                let selected_by = *rng.pick(&witnesses);
                let same_by = rng.chance(2, 3).then(|| *rng.pick(&witnesses));
                links.push(LinkSpec {
                    left,
                    right,
                    selected_by,
                    same_by,
                    also_same_by: Vec::new(),
                });
            }
        }
        let mut statuses = Vec::new();
        if rng.chance(1, 3) {
            let claim = rng.below(claim_count);
            let first = rng.below(source_count);
            statuses.push(StatusSpec {
                claim,
                status: ClaimStatus::Disputed,
                witness: first,
            });
            if rng.chance(1, 2) {
                statuses.push(StatusSpec {
                    claim,
                    status: ClaimStatus::Withdrawn,
                    witness: rng.below(source_count),
                });
            }
        }
        // A transition is a later reading of a claim already in World. Its
        // witness is read after every Source that asserts the claim, so every
        // World that knows one of them holds the claim at that reading; else
        // whether it happened would follow which Sources a World read first.
        statuses.retain(|status| {
            claims[status.claim]
                .asserted_by
                .iter()
                .all(|by| sources[*by].ingest_at_ms < sources[status.witness].ingest_at_ms)
        });
        let mut lens = rng.chance(2, 3).then(|| {
            let day = base_day() + Duration::days(rng.below(120) as i64);
            let mut sections = Vec::new();
            for key in ["evidence-for", "checks"] {
                let mut cited = vec![rng.below(claim_count)];
                if rng.chance(1, 2) {
                    let other = rng.below(claim_count);
                    if !cited.contains(&other) {
                        cited.push(other);
                    }
                }
                sections.push((key.to_owned(), cited));
            }
            LensSpec {
                at_ms: local_ms(day, 18, 0),
                sections,
                quote: None,
                by_turn: false,
                spans: Vec::new(),
            }
        });
        // A section may cite a dated or user Source's span beside its claims.
        // From its own stream, so the Worlds of earlier seeds stay as they were.
        let mut cites = Rng::new(seed ^ 0x0c17_e500);
        if let Some(lens) = &mut lens {
            let keys: Vec<String> = lens.sections.iter().map(|(key, _)| key.clone()).collect();
            for key in keys {
                if cites.chance(1, 3) {
                    let source = cites.below(source_count);
                    if sources[source].publication.is_some()
                        || sources[source].authorship == Authorship::User
                    {
                        lens.spans.push((key, source));
                    }
                }
            }
        }
        // Two claims one section cites fall at two later readings, so the
        // section meets a second alert. From its own stream, as the figures
        // are, so the Worlds of earlier seeds stay as they were.
        let mut falls = Rng::new(seed ^ 0x0fa1_1500);
        let mut falls_seen = None;
        if let Some((_, cited)) = lens
            .as_ref()
            .and_then(|lens| lens.sections.iter().find(|(_, claims)| claims.len() >= 2))
        {
            if falls.chance(1, 2) {
                let mut used = BTreeSet::new();
                // Claims with no status yet: the case is two claims falling.
                let fresh: Vec<usize> = cited
                    .iter()
                    .copied()
                    .filter(|claim| statuses.iter().all(|status| status.claim != *claim))
                    .collect();
                for claim in fresh.iter().take(2) {
                    let asserted = claims[*claim]
                        .asserted_by
                        .iter()
                        .map(|by| sources[*by].ingest_at_ms)
                        .max()
                        .unwrap_or(i64::MIN);
                    // A witness some cutoff knows: not an undated publisher page.
                    let later: Vec<usize> = (0..source_count)
                        .filter(|index| {
                            let source = &sources[*index];
                            source.ingest_at_ms > asserted
                                && !used.contains(index)
                                && (source.publication.is_some()
                                    || source.authorship == Authorship::User)
                        })
                        .collect();
                    if later.is_empty() {
                        continue;
                    }
                    let witness = later[falls.below(later.len())];
                    used.insert(witness);
                    // A cutoff the day the later of the two was read.
                    falls_seen = falls_seen
                        .max(crate::local_time::date_at_ms(sources[witness].ingest_at_ms));
                    statuses.push(StatusSpec {
                        claim: *claim,
                        status: ClaimStatus::Disputed,
                        witness,
                    });
                }
            }
        }
        let reset = rng.chance(1, 6).then(|| rng.below(source_count));
        let mut cutoffs = BTreeSet::new();
        cutoffs.insert(base_day() + Duration::days(rng.below(160) as i64));
        // The user's date of a near-midnight instant, and the day before it.
        if let Some(day) = sources
            .iter()
            .filter(|source| {
                source
                    .publication
                    .as_ref()
                    .is_some_and(|time| time.precision == TimePrecision::Instant)
            })
            .filter_map(|source| source.publication.as_ref().and_then(reference_day))
            .next()
        {
            cutoffs.insert(day);
            cutoffs.insert(day.pred_opt().unwrap());
        } else {
            cutoffs.insert(base_day() + Duration::days(rng.below(160) as i64));
        }
        cutoffs.extend(falls_seen);
        // From its own stream, so the Worlds of earlier seeds stay as they were.
        let mut figures = Rng::new(seed ^ 0x000f_160e);
        for source in &mut sources {
            source.figure_not_found = figures.chance(1, 5);
        }
        Self {
            seed,
            nodes,
            sources,
            claims,
            links,
            statuses,
            lens,
            reset,
            recaptures: Vec::new(),
            cutoffs: cutoffs.into_iter().collect(),
        }
    }

    /// One line per Source, claim, link and status, for a failure report.
    pub(in crate::commands) fn describe(&self) -> String {
        let day = |ms: i64| {
            crate::local_time::date_at_ms(ms)
                .map(|day| day.to_string())
                .unwrap_or_default()
        };
        let mut lines = Vec::new();
        for (index, source) in self.sources.iter().enumerate() {
            lines.push(format!(
                "S{index} {:?} published {} (user day {}) read {}",
                source.authorship,
                source
                    .publication
                    .as_ref()
                    .and_then(|time| time.value.clone())
                    .unwrap_or_else(|| "-".into()),
                source
                    .publication
                    .as_ref()
                    .and_then(reference_day)
                    .map(|day| day.to_string())
                    .unwrap_or_else(|| "-".into()),
                day(source.ingest_at_ms),
            ));
            if source.figure_not_found {
                lines.push(format!("S{index} figure not found"));
            }
        }
        for claim in &self.claims {
            lines.push(format!(
                "{} {} by {:?} end {:?}",
                claim.id,
                claim.relation,
                claim.asserted_by,
                claim
                    .stated_end
                    .as_ref()
                    .map(|(by, time)| (by, time.value.clone()))
            ));
        }
        for link in &self.links {
            lines.push(format!(
                "link {}-{} selected by S{} same by {:?} again by {:?}",
                self.claims[link.left].id,
                self.claims[link.right].id,
                link.selected_by,
                link.same_by,
                link.also_same_by
            ));
        }
        for status in &self.statuses {
            lines.push(format!(
                "{} {:?} by S{}",
                self.claims[status.claim].id, status.status, status.witness
            ));
        }
        if let Some(lens) = &self.lens {
            lines.push(format!("lens {} {:?}", day(lens.at_ms), lens.sections));
        }
        if let Some(reset) = self.reset {
            lines.push(format!("reset S{reset}"));
        }
        lines.join("\n")
    }

    /// The reference rule: a Source is known by the cutoff when it was
    /// published by then on the user's clock, or, undated, when it is the
    /// user's own statement saved by then.
    pub(in crate::commands) fn known_by(&self, index: usize, cutoff: NaiveDate) -> bool {
        let source = &self.sources[index];
        match &source.publication {
            Some(time) => reference_day(time).is_some_and(|day| day <= cutoff),
            None => {
                source.authorship == Authorship::User
                    && crate::local_time::date_at_ms(source.received_at_ms)
                        .is_some_and(|day| day <= cutoff)
            }
        }
    }

    /// Ingestion in the order this device read the Sources.
    pub(in crate::commands) fn plan(&self) -> Plan {
        let mut ingest: Vec<(usize, i64)> = self
            .sources
            .iter()
            .enumerate()
            .map(|(index, source)| (index, source.ingest_at_ms))
            .collect();
        ingest.sort_by_key(|(index, at)| (*at, *index));
        Plan {
            ingest,
            lens_at: self.lens.as_ref().map(|lens| lens.at_ms),
            reset: self.reset,
        }
    }

    /// The prefix World's plan: the same reads, only Sources known by `cutoff`.
    pub(in crate::commands) fn prefix_plan(&self, cutoff: NaiveDate) -> Plan {
        let mut plan = self.plan();
        plan.ingest
            .retain(|(index, _)| self.known_by(*index, cutoff));
        plan
    }
}

/// The user's calendar day of a publication, from the design: an instant
/// counts on the user's date at that instant, a date as written, a month or a
/// year at its last day.
pub(in crate::commands) fn reference_day(time: &TimePoint) -> Option<NaiveDate> {
    let value = time.value.as_deref()?;
    match time.precision {
        TimePrecision::Instant => DateTime::parse_from_rfc3339(value)
            .ok()
            .and_then(|at| crate::local_time::date_at_ms(at.timestamp_millis())),
        TimePrecision::Date => NaiveDate::parse_from_str(value.get(..10)?, "%Y-%m-%d").ok(),
        TimePrecision::Month => {
            let first =
                NaiveDate::parse_from_str(&format!("{}-01", value.get(..7)?), "%Y-%m-%d").ok()?;
            first.checked_add_months(chrono::Months::new(1))?.pred_opt()
        }
        TimePrecision::Year => NaiveDate::from_ymd_opt(value.get(..4)?.parse().ok()?, 12, 31),
        _ => None,
    }
}

fn reference_instant_ms(time: &TimePoint) -> Option<i64> {
    match time.precision {
        TimePrecision::Instant => DateTime::parse_from_rfc3339(time.value.as_deref()?)
            .ok()
            .map(|at| at.timestamp_millis()),
        _ => reference_day(time).map(|day| local_ms(day, 9, 0)),
    }
}

#[derive(Clone, Debug)]
pub(in crate::commands) struct Plan {
    /// Source index and the instant it was read, in reading order.
    pub ingest: Vec<(usize, i64)>,
    pub lens_at: Option<i64>,
    pub reset: Option<usize>,
}

// ---------------------------------------------------------------------------
// Ingestion
// ---------------------------------------------------------------------------

pub(in crate::commands) struct Built {
    _dir: tempfile::TempDir,
    pub state: AppState,
    pub world: String,
    pub sources: BTreeMap<usize, RetainedSource>,
    pub lens: Option<String>,
    /// Stored id → stable name, longest first.
    pub names: Vec<(String, String)>,
}

pub(in crate::commands) fn knowledge(state: &AppState) -> KnowledgeStore {
    KnowledgeStore::attach(state.store.clone(), super::knowledge_blobs(state).unwrap())
}

fn retain(store: &KnowledgeStore, source: &SourceSpec) -> RetainedSource {
    let bytes = source.text.clone().into_bytes();
    let end = bytes.len() as u64;
    store
        .retain_new_source(RetainNewSource {
            origin: None,
            loupist_id: LOUPIST.into(),
            at_ms: source.received_at_ms,
            title: source.title.clone(),
            source_class: if source.authorship == Authorship::User {
                SourceClass::UserStatement
            } else {
                SourceClass::PublisherDocument
            },
            origin_locator: source.url.clone(),
            provider_identity: "fixture".into(),
            authorship: source.authorship,
            coverage: Coverage::IncludedFull,
            origin_chat_id: None,
            request_ref: None,
            declared_digest: None,
            material: Some(RetainMaterial {
                media_type: "text/plain".into(),
                role: RepresentationRole::Original,
                document_id: source.canary.clone(),
                parser_version: None,
                parent_representation_id: None,
                input_span_ids: Vec::new(),
                selector: Selector::ByteRange { start: 0, end },
                context_note: source.title.clone(),
                bytes,
            }),
            publication_time: source.publication.clone(),
        })
        .unwrap()
}

/// What one World revision holds after reading the Sources in `read`.
struct Snapshot {
    claims: Vec<GraphClaim>,
    temporal: WorldTemporal,
}

fn witness(
    spec: &Spec,
    retained: &BTreeMap<usize, RetainedSource>,
    index: usize,
) -> AssertionWitness {
    AssertionWitness {
        span: retained[&index].span_id.clone().unwrap(),
        publication: spec.sources[index].publication.clone(),
    }
}

/// The World as the pipeline stores it after reading `read`: each claim's
/// evidence and earliest-published witness, its stated end, the resolver
/// selections whose witnesses were read, endings and conflicts from the pair
/// rule, and the status the latest read transition set.
fn snapshot(
    spec: &Spec,
    retained: &BTreeMap<usize, RetainedSource>,
    read: &[usize],
    previous: &BTreeMap<String, GraphClaim>,
    at_ms: i64,
    statuses: &BTreeMap<usize, ClaimStatus>,
) -> Snapshot {
    let known: BTreeSet<usize> = read.iter().copied().collect();
    let mut assertions = BTreeMap::new();
    let mut present = BTreeMap::new();
    for (index, claim) in spec.claims.iter().enumerate() {
        let by: Vec<usize> = claim
            .asserted_by
            .iter()
            .copied()
            .filter(|source| known.contains(source))
            .collect();
        if by.is_empty() {
            continue;
        }
        let mut sources = AssertionSources::default();
        for source in &by {
            sources
                .source_ids
                .insert(retained[source].record_id.clone());
            let candidate = witness(spec, retained, *source);
            if let Some(day) = &candidate.publication {
                sources.add_day(day);
            }
            if sources
                .witness
                .as_ref()
                .is_none_or(|old| witness_order(&candidate, old).is_lt())
            {
                sources.witness = Some(candidate);
            }
        }
        if let Some((stater, time)) = &claim.stated_end {
            if known.contains(stater) {
                sources.stated = Some(StatedEnd {
                    time: time.clone(),
                    witness: witness(spec, retained, *stater),
                });
            }
        }
        assertions.insert(claim.id.clone(), sources);
        present.insert(index, by);
    }
    let mut links = Vec::new();
    for link in &spec.links {
        if !present.contains_key(&link.left) || !present.contains_key(&link.right) {
            continue;
        }
        if !known.contains(&link.selected_by) {
            continue;
        }
        let same: Vec<AssertionWitness> = link
            .same_by
            .iter()
            .chain(&link.also_same_by)
            .filter(|by| known.contains(by))
            .map(|by| witness(spec, retained, *by))
            .collect();
        let mut stored = AssertionLink {
            left: spec.claims[link.left].id.clone(),
            right: spec.claims[link.right].id.clone(),
            same_assertion: !same.is_empty(),
            selected_by: Some(witness(spec, retained, link.selected_by)),
            same_assertion_by: same.iter().min_by(|a, b| witness_order(a, b)).cloned(),
            reaffirmed_by: Vec::new(),
        };
        for by in &same {
            stored.reaffirm(by);
        }
        links.push(stored);
    }
    let mut effects = PairEffects::<String>::default();
    for (id, sources) in &assertions {
        if let Some(stated) = &sources.stated {
            effects.consider(id, Ending::stated(stated));
        }
    }
    let by_id: BTreeMap<&str, &ClaimSpec> = spec
        .claims
        .iter()
        .map(|claim| (claim.id.as_str(), claim))
        .collect();
    for link in &links {
        let left = by_id[link.left.as_str()];
        let right = by_id[link.right.as_str()];
        let endpoints = |claim: &ClaimSpec| -> [String; 2] {
            [
                spec.nodes[claim.endpoints[0]].id.clone(),
                spec.nodes[claim.endpoints[1]].id.clone(),
            ]
        };
        let (left_ends, right_ends) = (endpoints(left), endpoints(right));
        let outcome = decide_pair(
            Assertion {
                relation: &left.relation,
                endpoints: [&left_ends[0], &left_ends[1]],
                valid_at: None,
                assertions: &assertions[&link.left],
            },
            Assertion {
                relation: &right.relation,
                endpoints: [&right_ends[0], &right_ends[1]],
                valid_at: None,
                assertions: &assertions[&link.right],
            },
            PairSelection {
                same_assertion: link.same_assertion,
                reaffirmed_by: &link.reaffirmed_by,
            },
        );
        effects.record(
            (&link.left, &link.left, &assertions[&link.left]),
            (&link.right, &link.right, &assertions[&link.right]),
            outcome,
        );
    }
    let endings = std::mem::take(&mut effects.endings);
    effects.drop_witnessed_conflicts(|id| endings.get(id), |id| assertions.get(id));
    let conflicts = std::mem::take(&mut effects.conflicts);
    for (id, sources) in &mut assertions {
        (sources.reopened, sources.lapsed) = effects.take_reopening(id);
    }
    let mut claims = Vec::new();
    for (index, by) in &present {
        let spec_claim = &spec.claims[*index];
        let sources = &assertions[&spec_claim.id];
        let author = |source: usize| match spec.sources[source].authorship {
            Authorship::User => "user",
            _ => "publisher",
        };
        let mut qualifiers = Vec::new();
        let ending = endings.get(&spec_claim.id);
        if let Some(ending) = ending {
            qualifiers.push(Qualifier {
                key: END_BASIS.into(),
                value: match ending.basis {
                    crate::knowledge::world_episodes::EndBasis::Reference => "reference",
                    crate::knowledge::world_episodes::EndBasis::Stated => "stated",
                }
                .into(),
            });
            qualifiers.push(Qualifier {
                key: SUPERSEDED_BY_SPAN.into(),
                value: ending.span.clone(),
            });
            if let Some(by) = &ending.by {
                qualifiers.push(Qualifier {
                    key: SUPERSEDED_BY_CLAIM.into(),
                    value: by.clone(),
                });
            }
        }
        for partner in conflicts.get(&spec_claim.id).into_iter().flatten().take(4) {
            qualifiers.push(Qualifier {
                key: CONFLICTS_WITH_CLAIM.into(),
                value: partner.clone(),
            });
        }
        for (key, value) in reopening_qualifiers(sources.reopened.as_ref(), &sources.lapsed) {
            qualifiers.push(Qualifier {
                key: key.into(),
                value,
            });
        }
        let before = previous.get(&spec_claim.id);
        let unchanged = before.is_some_and(|old| {
            old.invalid_at.as_ref() == ending.map(|end| &end.time)
                && old.qualifier(SUPERSEDED_BY_SPAN) == ending.map(|end| end.span.as_str())
        });
        let expired_at = match ending {
            None => None,
            Some(_) if unchanged => before.and_then(|old| old.expired_at.clone()),
            // As the extraction records it: the reading's instant on the
            // user's clock.
            Some(_) => Some(TimePoint::from_unix_ms(at_ms)),
        };
        claims.push(GraphClaim {
            id: spec_claim.id.clone(),
            proposition: spec_claim.proposition.clone(),
            attribution: author(by[0]).into(),
            relation: spec_claim.relation.clone(),
            endpoints: vec![
                ClaimEndpoint {
                    node_id: spec.nodes[spec_claim.endpoints[0]].id.clone(),
                    role: "source".into(),
                },
                ClaimEndpoint {
                    node_id: spec.nodes[spec_claim.endpoints[1]].id.clone(),
                    role: "target".into(),
                },
            ],
            qualifiers,
            evidence: by
                .iter()
                .map(|source| EvidenceBinding {
                    span_id: retained[source].span_id.clone().unwrap(),
                    role: BindingRole::ExtractedFrom,
                    attribution: author(*source).into(),
                    figure_check: spec.sources[*source]
                        .figure_not_found
                        .then_some(crate::knowledge::EvidenceFigure::NotFound),
                    passage: None,
                })
                .collect(),
            status: statuses.get(index).copied().unwrap_or(ClaimStatus::Active),
            recorded_at_ms: Some(before.and_then(|old| old.recorded_at_ms).unwrap_or(at_ms)),
            publication_time: sources.publication().cloned(),
            effective_time: None,
            valid_at: None,
            invalid_at: ending.map(|end| end.time.clone()),
            expired_at,
        });
    }
    Snapshot {
        claims,
        temporal: WorldTemporal { links, assertions },
    }
}

pub(in crate::commands) fn build(spec: &Spec, plan: &Plan) -> Built {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::for_test(dir.path().join("app"));
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    crate::commands::tests::seed_profile(
        state.store.as_ref(),
        &crate::commands::tests::profile(LOUPIST, &workspace, 1),
    );
    state
        .store
        .create_chat(&crate::commands::tests::chat(SESSION, LOUPIST, 1))
        .unwrap();
    let store = knowledge(&state);
    let mut retained = BTreeMap::new();
    for (index, _) in &plan.ingest {
        retained.insert(*index, retain(&store, &spec.sources[*index]));
    }
    // A World starts sealed and empty, before it reads anything.
    let (world, empty) = store
        .transact(|write| {
            let record = write.create_record(CreateRecord {
                loupist_id: LOUPIST.into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: 2,
            })?;
            let sealed = write.seal_world(SealWorld {
                loupist_id: LOUPIST.into(),
                record_id: record.record_id.clone(),
                expected_revision_id: None,
                at_ms: 3,
                nodes: Vec::new(),
                claims: Vec::new(),
                annotations: Vec::new(),
                dependencies: Vec::new(),
            })?;
            Ok((record.record_id, sealed.revision_id))
        })
        .unwrap();
    let mut read = Vec::new();
    let mut previous: BTreeMap<String, GraphClaim> = BTreeMap::new();
    let mut head: Option<String> = Some(empty);
    let mut statuses: BTreeMap<usize, ClaimStatus> = BTreeMap::new();
    let mut lens = None;
    let mut lens_pending = plan.lens_at;
    for (index, at_ms) in &plan.ingest {
        if let Some(lens_at) = lens_pending.filter(|lens_at| lens_at < at_ms) {
            lens = file_lens(spec, &store, &previous, &read, &retained, lens_at);
            lens_pending = None;
        }
        read.push(*index);
        for status in spec
            .statuses
            .iter()
            .filter(|status| status.witness == *index)
        {
            // A transition is a later reading of a claim already in World: the
            // extraction never creates a claim disputed or withdrawn.
            if previous.contains_key(&spec.claims[status.claim].id) {
                statuses.insert(status.claim, status.status);
            }
        }
        let snapshot = snapshot(spec, &retained, &read, &previous, *at_ms, &statuses);
        let mut node_ids = BTreeSet::new();
        for claim in &snapshot.claims {
            node_ids.extend(claim.endpoints.iter().map(|end| end.node_id.clone()));
        }
        let source = &retained[index];
        let source_spec = &spec.sources[*index];
        let episode = WorldEpisode {
            episode_id: format!("eps_oracle_{index}"),
            loupist_id: LOUPIST.into(),
            world_record_id: world.clone(),
            job_id: format!("job-oracle-{index}"),
            sealed_revision_id: String::new(),
            previous_revision_id: head.clone(),
            source_record_id: source.record_id.clone(),
            source_revision_id: source.revision_id.clone(),
            span_id: source.span_id.clone().unwrap(),
            chunk_index: Some(0),
            chunk_count: Some(1),
            char_start: Some(0),
            char_end: Some(source_spec.text.chars().count() as u32),
            reference_time: Some(
                source_spec
                    .episode_publication
                    .as_ref()
                    .or(source_spec.publication.as_ref())
                    .and_then(|time| time.value.clone())
                    .unwrap_or_else(|| {
                        DateTime::from_timestamp_millis(source_spec.received_at_ms)
                            .unwrap()
                            .to_rfc3339()
                    }),
            ),
            reference_basis: Some(if source_spec.publication.is_some() {
                ReferenceBasis::Published
            } else {
                ReferenceBasis::Received
            }),
            publication_time: source_spec
                .episode_publication
                .clone()
                .or_else(|| source_spec.publication.clone()),
            model: Some(EpisodeModel {
                model_profile: "fixture".into(),
                thinking_level: "low".into(),
            }),
            changes: EpisodeChanges::default(),
            backfilled: false,
            created_at_ms: *at_ms,
        };
        let dependencies = read
            .iter()
            .map(|source| EvidenceRevision {
                record_id: retained[source].record_id.clone(),
                revision_digest: retained[source].revision_digest.clone(),
                role: DependencyRole::WorldSource,
            })
            .collect();
        let claims = snapshot.claims.clone();
        let revision = store
            .transact(|write| {
                write.publish_world_from_source(
                    SealWorld {
                        loupist_id: LOUPIST.into(),
                        record_id: world.clone(),
                        expected_revision_id: head.clone(),
                        at_ms: *at_ms,
                        nodes: spec
                            .nodes
                            .iter()
                            .filter(|node| node_ids.contains(&node.id))
                            .cloned()
                            .collect(),
                        claims: snapshot.claims,
                        annotations: Vec::new(),
                        dependencies,
                    },
                    source,
                    &source_spec.title,
                    episode,
                    snapshot.temporal,
                )
            })
            .unwrap_or_else(|error| panic!("seed {} publication failed: {error}", spec.seed));
        head = Some(revision.revision_id);
        previous = claims
            .into_iter()
            .map(|claim| (claim.id.clone(), claim))
            .collect();
    }
    if let Some(lens_at) = lens_pending {
        lens = file_lens(spec, &store, &previous, &read, &retained, lens_at);
    }
    for recapture in &spec.recaptures {
        let Some(target) = retained.get(&recapture.source) else {
            continue;
        };
        let bytes = recapture.title.clone().into_bytes();
        let end = bytes.len() as u64;
        store
            .transact(|write| {
                write.retain_source(crate::knowledge::RetainSource {
                    origin: None,
                    loupist_id: LOUPIST.into(),
                    record_id: target.record_id.clone(),
                    expected_revision_id: Some(target.revision_id.clone()),
                    at_ms: recapture.at_ms,
                    source_id: Some(target.source_id.clone()),
                    title: recapture.title.clone(),
                    source_class: SourceClass::PublisherDocument,
                    origin_locator: recapture.url.clone(),
                    provider_identity: "fixture".into(),
                    authorship: spec.sources[recapture.source].authorship,
                    coverage: Coverage::IncludedFull,
                    origin_chat_id: None,
                    request_ref: None,
                    declared_digest: None,
                    material: Some(RetainMaterial {
                        media_type: "text/plain".into(),
                        role: RepresentationRole::Original,
                        document_id: format!("recapture-{}", recapture.source),
                        parser_version: None,
                        parent_representation_id: None,
                        input_span_ids: Vec::new(),
                        selector: Selector::ByteRange { start: 0, end },
                        context_note: recapture.title.clone(),
                        bytes,
                    }),
                    publication_time: Some(recapture.publication.clone()),
                })
            })
            .unwrap_or_else(|error| panic!("seed {} recapture failed: {error}", spec.seed));
    }
    if let Some(index) = plan.reset.filter(|index| retained.contains_key(index)) {
        // A reset through the path Memory reset takes: the Source's support
        // leaves World (a claim only it supported goes), then the Source is
        // tombstoned.
        let span = retained[&index].span_id.clone().unwrap();
        let only_it: BTreeSet<String> = previous
            .values()
            .filter(|claim| claim.evidence.iter().all(|binding| binding.span_id == span))
            .map(|claim| claim.id.clone())
            .collect();
        // After every reading of the seed and the Lens filing, the same moment
        // for the full World and each prefix World: a reset is the last thing done.
        let at_ms = spec
            .sources
            .iter()
            .map(|source| source.ingest_at_ms)
            .chain(plan.ingest.iter().map(|(_, at)| *at))
            .chain(plan.lens_at)
            .max()
            .unwrap_or(0)
            + 3_600_000;
        let refs = crate::knowledge::shared_pack::ImportRefs {
            nodes: BTreeSet::new(),
            claims: only_it,
            spans: BTreeSet::from([span]),
            sources: BTreeSet::from([retained[&index].record_id.clone()]),
            preserve_other_claims: true,
        };
        super::with_loupist_knowledge(&state, LOUPIST, |tx, blobs| {
            crate::knowledge::shared_pack::remove_world_additions(
                tx,
                blobs,
                LOUPIST,
                &refs,
                at_ms,
                "user",
                Some(json!({"kind": "memory_reset"})),
            )?;
            let mut controls = crate::knowledge::memory_controls::load(tx, LOUPIST)?;
            controls.removed.extend(refs.sources.iter().cloned());
            crate::knowledge::memory_controls::save(tx, LOUPIST, &controls)
        })
        .unwrap_or_else(|error| panic!("seed {} reset failed: {error:?}", spec.seed));
        head = super::with_loupist_knowledge(&state, LOUPIST, |tx, _| {
            crate::knowledge::world_changes::world_head_revision(tx, LOUPIST)
        })
        .unwrap();
    }
    let mut names = Vec::new();
    for (index, source) in &retained {
        names.push((source.record_id.clone(), format!("S{index}")));
        names.push((source.revision_id.clone(), format!("S{index}@rev")));
        names.push((source.revision_digest.clone(), format!("S{index}@digest")));
        names.push((source.capture_id.clone(), format!("S{index}@capture")));
        names.push((source.source_id.clone(), format!("S{index}@source")));
        if let Some(span) = &source.span_id {
            names.push((span.clone(), format!("S{index}#span")));
        }
        if let Some(id) = &source.representation_id {
            names.push((id.clone(), format!("S{index}@representation")));
        }
        if let Some(digest) = &source.selected_digest {
            names.push((digest.clone(), format!("S{index}@selected")));
        }
        if let Some(digest) = &source.material_digest {
            names.push((digest.clone(), format!("S{index}@material")));
        }
    }
    names.push((world.clone(), "W".into()));
    if let Some(head) = &head {
        names.push((head.clone(), "W@head".into()));
    }
    if let Some(lens) = &lens {
        names.push((lens.clone(), "L".into()));
    }
    names.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
    Built {
        _dir: dir,
        state,
        world,
        sources: retained,
        lens,
        names,
    }
}

/// The user's Lens page at `at_ms`, citing the claims the World holds then.
fn file_lens(
    spec: &Spec,
    store: &KnowledgeStore,
    present: &BTreeMap<String, GraphClaim>,
    read: &[usize],
    retained: &BTreeMap<usize, RetainedSource>,
    at_ms: i64,
) -> Option<String> {
    use crate::knowledge::lens_page::{
        Basis, BasisRef, FilingOrigin, LensOp, PageDelta, PageKind, PageTarget, SectionKey,
    };
    let lens = spec.lens.as_ref()?;
    let mut ops = Vec::new();
    for (key, cited) in &lens.sections {
        // A page cites what the user could have seen: a claim a dated page or
        // the user's own note supports, not only an undated publisher page.
        let citable = |index: &usize| {
            spec.claims[*index].asserted_by.iter().any(|by| {
                read.contains(by)
                    && (spec.sources[*by].publication.is_some()
                        || spec.sources[*by].authorship == Authorship::User)
            })
        };
        let refs: Vec<BasisRef> = cited
            .iter()
            .filter(|index| citable(index))
            .map(|index| spec.claims[*index].id.clone())
            .filter(|id| present.contains_key(id))
            .map(|claim_id| BasisRef::Claim { claim_id })
            .collect();
        let mut refs = refs;
        refs.extend(
            lens.spans
                .iter()
                .filter(|(section, source)| section == key && read.contains(source))
                .filter_map(|(_, source)| retained.get(source))
                .map(|source| BasisRef::Span {
                    record_id: source.record_id.clone(),
                    revision_digest: source.revision_digest.clone(),
                    span_id: source.span_id.clone().unwrap(),
                }),
        );
        if refs.is_empty() {
            continue;
        }
        ops.push(LensOp::Put {
            key: SectionKey::new(key).unwrap(),
            heading: key.clone(),
            markdown: lens
                .quote
                .clone()
                .unwrap_or_else(|| format!("What the cited facts say ({SEARCH_WORD}).")),
            basis: Basis {
                refs,
                uncertainty: Some("Open until the next report.".into()),
            },
            expected: None,
        });
    }
    if ops.is_empty() {
        return None;
    }
    let origin = if lens.by_turn {
        FilingOrigin::Turn {
            chat_id: SESSION.into(),
            message_id: "msg-oracle".into(),
            user_message_id: None,
            unattended: false,
        }
    } else {
        FilingOrigin::User {
            chat_id: None,
            message_id: None,
        }
    };
    let mut record: Option<String> = None;
    for chunk in ops.chunks(12) {
        let target = match &record {
            Some(record_id) => PageTarget::Existing {
                record_id: record_id.clone(),
            },
            None => PageTarget::Create {
                kind: PageKind::Method,
                title: "Management changes".into(),
                anchors: Vec::new(),
            },
        };
        let filed = store
            .transact(|write| {
                write.file(
                    LOUPIST,
                    &origin,
                    vec![PageDelta {
                        target,
                        ops: chunk.to_vec(),
                    }],
                    at_ms,
                )
            })
            .unwrap_or_else(|error| panic!("seed {} Lens filing failed: {error}", spec.seed));
        record = record.or(filed.into_iter().next().map(|page| page.record_id));
    }
    record
}

// ---------------------------------------------------------------------------
// Readers
// ---------------------------------------------------------------------------

pub(in crate::commands) fn executor(built: &Built) -> AppToolExecutor {
    AppToolExecutor {
        state: built.state.clone(),
        capture: Arc::new(ToolCapture::default()),
        loupist_id: LOUPIST.into(),
        capability_ids: BTreeSet::new(),
    }
}

pub(in crate::commands) fn policy(cutoff: Option<NaiveDate>) -> ToolPolicy {
    ToolPolicy {
        loupist_id: LOUPIST.into(),
        session_id: SESSION.into(),
        use_memory: true,
        propose_memory_updates: false,
        memory_proposal_budget: 0,
        as_of: cutoff.map(|day| day.to_string()),
        display_currency: None,
    }
}

/// One reader's answer: what the model or renderer sees, and its receipt.
#[derive(Debug, Clone)]
pub(in crate::commands) struct Answer {
    pub result: Value,
    pub receipts: Vec<Value>,
    /// Debug text of everything staged: canary scans read it whole.
    pub staged: String,
}

pub(in crate::commands) fn run_tool(
    built: &Built,
    cutoff: Option<NaiveDate>,
    method: ToolMethod,
    params: Value,
) -> Answer {
    let mut executor = executor(built);
    // The host gives the capture the turn's date as well as the policy.
    let mut capture = ToolCapture::default();
    capture.turn_as_of = cutoff.map(|day| day.to_string());
    executor.capture = Arc::new(capture);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = runtime.block_on(executor.execute(&policy(cutoff), method, params));
    let result = match result {
        Ok(value) => value,
        Err(error) => json!({"error": crate::broker::error_body(&error)}),
    };
    let (receipts, staged) = runtime.block_on(async {
        let reads = executor.capture.staged_reads.lock().await;
        let memories = executor.capture.memories.lock().await;
        let receipts = reads
            .iter()
            .filter_map(|read| match &read.payload {
                AckedPayload::World { view_json, .. } => serde_json::from_str(view_json).ok(),
                _ => None,
            })
            .collect();
        (receipts, format!("{reads:?} {memories:?}"))
    });
    Answer {
        result,
        receipts,
        staged,
    }
}

/// Every model- and renderer-visible read of World, with the properties that
/// hold for it. A Memory read tool of the catalog or a `knowledge_*` command
/// with no row fails [`the_reader_table_names_every_memory_read`].
pub(in crate::commands) struct ReaderRow {
    pub name: &'static str,
    /// A catalog tool name or a `knowledge_*` command.
    pub entry: &'static str,
    pub how: Covered,
}

pub(in crate::commands) enum Covered {
    /// Read under a cutoff: P1, P2 (and P4 for a paged feed).
    Cutoff,
    /// A renderer read: no cutoff exists; P3 and P4 where it pages.
    Current,
    /// Not a read of World claims, Sources or Lens state; why.
    Outside(#[allow(dead_code)] &'static str),
    /// The same function as another row, which carries the properties.
    Same(&'static str),
}

pub(in crate::commands) const READERS: &[ReaderRow] = &[
    ReaderRow { name: "world_read.view", entry: "world_read", how: Covered::Cutoff },
    ReaderRow { name: "world_read.history", entry: "world_read", how: Covered::Cutoff },
    ReaderRow { name: "world_read.source", entry: "world_read", how: Covered::Cutoff },
    ReaderRow { name: "world_read.receipt", entry: "world_read", how: Covered::Cutoff },
    ReaderRow { name: "world_search.hits", entry: "world_search", how: Covered::Cutoff },
    ReaderRow { name: "world_search.hits.figure", entry: "world_search", how: Covered::Same("world_search.hits") },
    ReaderRow { name: "world_read.view.figure", entry: "world_read", how: Covered::Same("world_read.view") },
    ReaderRow { name: "world_read.view.reopened", entry: "world_read", how: Covered::Same("world_read.view") },
    ReaderRow { name: "world_read.history.reopened", entry: "world_read", how: Covered::Same("world_read.history") },
    ReaderRow { name: "world_changes.recorded", entry: "world_changes", how: Covered::Cutoff },
    ReaderRow { name: "world_changes.published", entry: "world_changes", how: Covered::Cutoff },
    ReaderRow { name: "world_changes.pages", entry: "world_changes", how: Covered::Cutoff },
    ReaderRow { name: "world_changes.between", entry: "world_changes", how: Covered::Cutoff },
    ReaderRow { name: "world_changes.receipt", entry: "world_changes", how: Covered::Cutoff },
    ReaderRow { name: "world_changes.claim_ended.figures", entry: "world_changes", how: Covered::Same("world_changes.recorded") },
    ReaderRow { name: "asset_page_read.sinceRead", entry: "asset_page_read", how: Covered::Cutoff },
    ReaderRow { name: "lens_read.sections", entry: "lens_read", how: Covered::Cutoff },
    ReaderRow { name: "lens_search.hits", entry: "lens_search", how: Covered::Cutoff },
    ReaderRow { name: "context.lens_index.stale", entry: "lens_read", how: Covered::Same("lens_read.sections") },
    ReaderRow { name: "check.world_changes.replay", entry: "world_changes", how: Covered::Same("world_changes.published") },
    ReaderRow { name: "world_job", entry: "world_job", how: Covered::Outside("job state of extraction, no World claim, Source text or Lens state") },
    ReaderRow { name: "conversation_memory", entry: "conversation_memory", how: Covered::Outside("notes, Journal and messages: recorded-time rule, not World") },
    ReaderRow { name: "knowledge_world_query", entry: "knowledge_world_query", how: Covered::Current },
    ReaderRow { name: "knowledge_world_query.asOf", entry: "knowledge_world_query", how: Covered::Cutoff },
    ReaderRow { name: "knowledge_map.asOf", entry: "knowledge_map", how: Covered::Cutoff },
    ReaderRow { name: "knowledge_world_changes.asOf", entry: "knowledge_world_changes", how: Covered::Cutoff },
    ReaderRow { name: "knowledge_world_claim_history.asOf", entry: "knowledge_world_claim_history", how: Covered::Cutoff },
    ReaderRow { name: "knowledge_world_changes.since", entry: "knowledge_world_changes", how: Covered::Current },
    ReaderRow { name: "knowledge_world_changes.between", entry: "knowledge_world_changes", how: Covered::Current },
    ReaderRow { name: "knowledge_world_claim_history", entry: "knowledge_world_claim_history", how: Covered::Current },
    ReaderRow { name: "knowledge_growth_events", entry: "knowledge_growth_events", how: Covered::Current },
    ReaderRow { name: "knowledge_growth_events.claim_reopened", entry: "knowledge_growth_events", how: Covered::Same("knowledge_growth_events") },
    ReaderRow { name: "knowledge_lens_read", entry: "knowledge_lens_read", how: Covered::Current },
    ReaderRow { name: "knowledge_lens_search", entry: "knowledge_lens_search", how: Covered::Current },
    ReaderRow { name: "knowledge_lens_pages_for_node", entry: "knowledge_lens_pages_for_node", how: Covered::Current },
    ReaderRow { name: "knowledge_map", entry: "knowledge_map", how: Covered::Current },
    ReaderRow { name: "knowledge_source_read", entry: "knowledge_source_read", how: Covered::Current },
    ReaderRow { name: "knowledge_research_search", entry: "knowledge_research_search", how: Covered::Current },
    ReaderRow { name: "knowledge_catalog", entry: "knowledge_catalog", how: Covered::Current },
    ReaderRow { name: "knowledge_journal_read", entry: "knowledge_journal_read", how: Covered::Outside("a Journal entry: recorded-time rule") },
    ReaderRow { name: "knowledge_journal_source_read", entry: "knowledge_journal_source_read", how: Covered::Same("knowledge_source_read") },
    ReaderRow { name: "knowledge_working_note_read", entry: "knowledge_working_note_read", how: Covered::Outside("a working note: recorded-time rule") },
    ReaderRow { name: "knowledge_working_notes", entry: "knowledge_working_notes", how: Covered::Outside("working notes: recorded-time rule") },
    ReaderRow { name: "knowledge_followed_status", entry: "knowledge_followed_status", how: Covered::Outside("followed names and their price state") },
    ReaderRow { name: "knowledge_memory_controls", entry: "knowledge_memory_controls", how: Covered::Outside("Memory settings") },
    ReaderRow { name: "knowledge_memory_usage", entry: "knowledge_memory_usage", how: Covered::Outside("counts of saved items") },
    ReaderRow { name: "knowledge_memory_reset_preview", entry: "knowledge_memory_reset_preview", how: Covered::Outside("counts a reset would remove") },
    ReaderRow { name: "knowledge_graph_job", entry: "knowledge_graph_job", how: Covered::Outside("job state of extraction") },
    ReaderRow { name: "knowledge_learning_state", entry: "knowledge_learning_state", how: Covered::Outside("method learning's phase and a count of cases: no World claim, Source text or Lens state") },
    ReaderRow { name: "knowledge_graph_settings_get", entry: "knowledge_graph_settings_get", how: Covered::Outside("World processing settings") },
    ReaderRow { name: "knowledge_source_selection_resolve", entry: "knowledge_source_selection_resolve", how: Covered::Same("knowledge_source_read") },
    ReaderRow { name: "knowledge_source_export", entry: "knowledge_source_export", how: Covered::Outside("writes a Source copy the user asked for") },
    ReaderRow { name: "knowledge writes", entry: "knowledge_connection_dismiss knowledge_graph_pause knowledge_graph_recover knowledge_graph_retry_source knowledge_graph_retry_sources knowledge_graph_settings_set knowledge_lens_archive knowledge_lens_pause knowledge_memory_reset knowledge_source_retain", how: Covered::Outside("writes: no read of World") },
    ReaderRow { name: "knowledge_fixture", entry: "knowledge_fixture", how: Covered::Outside("e2e seed") },
];

// ---------------------------------------------------------------------------
// Normalization
// ---------------------------------------------------------------------------

const DROPPED_KEYS: &[&str] = &[
    "readRef",
    "receiptId",
    "next",
    "revisionDigest",
    "viewDigest",
    "digest",
    "eventId",
    "snapshotId",
    "scan",
    "citation_url",
    "result_ref",
    // What a renderer read under a cutoff left out differs from the prefix
    // World by design; P6 checks it against the prefix World instead.
    "asOfReport",
];

/// The text inside `<<untrusted id=… source="…">>` delimiters. Which Source
/// labels a block is the taint oracle's question; P2 scans the label for
/// canaries before this runs.
fn undelimited(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest
        .find("<<untrusted id=")
        .or_else(|| rest.find("<<end untrusted id="))
    {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail.find(">>").map_or(tail.len(), |end| end + 2);
        rest = &tail[end..];
        if let Some(stripped) = rest.strip_prefix('\n') {
            rest = stripped;
        }
        if out.ends_with('\n') {
            out.pop();
        }
    }
    out.push_str(rest);
    out
}

/// Ids nothing in `names` maps (minted per build: Lens revisions, events).
fn unnamed_ids(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let found = [
            "rev_", "rec_", "gev_", "snp_", "eps_", "ev_", "spn_", "cap_", "src_", "lop_", "rep_",
        ]
        .iter()
        .filter_map(|prefix| rest.find(prefix).map(|at| (at, prefix.len())))
        .min();
        let Some((at, prefix)) = found else {
            break;
        };
        let tail = &rest[at + prefix..];
        let hex = tail
            .find(|ch: char| !ch.is_ascii_hexdigit())
            .unwrap_or(tail.len());
        if hex >= 16 {
            out.push_str(&rest[..at + prefix]);
            out.push('#');
            rest = &tail[hex..];
        } else {
            out.push_str(&rest[..at + prefix]);
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

pub(in crate::commands) fn normalize(value: &Value, names: &[(String, String)]) -> Value {
    match value {
        Value::String(text) => {
            let mut text = text.clone();
            for (id, name) in names {
                if !id.is_empty() && text.contains(id.as_str()) {
                    text = text.replace(id.as_str(), name);
                }
            }
            if text.starts_with("rdg_")
                || text.starts_with("wch1.")
                || text.contains("#evidence-ev_")
            {
                return Value::String("#".into());
            }
            let text = unnamed_ids(&undelimited(&text));
            // An embedded JSON text (a World view) is compared as JSON.
            if (text.starts_with('{') || text.starts_with('[')) && text.len() > 1 {
                if let Ok(inner) = serde_json::from_str::<Value>(&text) {
                    return normalize(&inner, &[]);
                }
            }
            Value::String(text)
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| normalize(item, names)).collect())
        }
        // A null field and an absent one say the same thing.
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(key, item)| !DROPPED_KEYS.contains(&key.as_str()) && !item.is_null())
                .map(|(key, item)| {
                    let key = match normalize(&Value::String(key.clone()), names) {
                        Value::String(text) => text,
                        _ => key.clone(),
                    };
                    (key, normalize(item, names))
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The first place two values differ, as a JSON pointer and both sides.
pub(in crate::commands) fn first_difference(
    left: &Value,
    right: &Value,
    at: &str,
) -> Option<String> {
    match (left, right) {
        (Value::Object(a), Value::Object(b)) => {
            let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
            for key in keys {
                let here = format!("{at}/{key}");
                match (a.get(key), b.get(key)) {
                    (Some(x), Some(y)) => {
                        if let Some(found) = first_difference(x, y, &here) {
                            return Some(found);
                        }
                    }
                    (x, y) => {
                        return Some(format!(
                            "{here}: full={} prefix={}",
                            short(x.unwrap_or(&Value::Null)),
                            short(y.unwrap_or(&Value::Null))
                        ))
                    }
                }
            }
            None
        }
        (Value::Array(a), Value::Array(b)) => {
            if let Some(found) = a
                .iter()
                .zip(b)
                .enumerate()
                .find_map(|(index, (x, y))| first_difference(x, y, &format!("{at}/{index}")))
            {
                return Some(found);
            }
            (a.len() != b.len()).then(|| {
                let extra = if a.len() > b.len() {
                    &a[b.len()..]
                } else {
                    &b[a.len()..]
                };
                format!(
                    "{at}: full has {} rows, prefix {}; the extra: {}",
                    a.len(),
                    b.len(),
                    short(&Value::Array(extra.to_vec()))
                )
            })
        }
        _ if left == right => None,
        _ => Some(format!(
            "{at}: full={} prefix={}",
            short(left),
            short(right)
        )),
    }
}

fn short(value: &Value) -> String {
    let text = value.to_string();
    if text.chars().count() > 600 {
        format!("{}…", text.chars().take(600).collect::<String>())
    } else {
        text
    }
}

// ---------------------------------------------------------------------------
// Reading one World under one cutoff
// ---------------------------------------------------------------------------

/// The ids the readers are pointed at, chosen from the spec so both Worlds
/// get the same calls.
struct Targets {
    claims: Vec<String>,
    between: Option<(usize, usize)>,
}

fn targets(spec: &Spec, cutoff: NaiveDate) -> Targets {
    let known: Vec<usize> = (0..spec.sources.len())
        .filter(|index| spec.known_by(*index, cutoff))
        .collect();
    // Claims both Worlds hold: a reset removes from the prefix World a claim
    // only the reset Source supported there, so naming it is another request.
    let claims = spec
        .claims
        .iter()
        .filter(|claim| {
            claim
                .asserted_by
                .iter()
                .any(|by| known.contains(by) && spec.reset != Some(*by))
        })
        .map(|claim| claim.id.clone())
        .take(2)
        .collect();
    let between = {
        let mut dated: Vec<(NaiveDate, usize)> = known
            .iter()
            .filter_map(|index| {
                spec.sources[*index]
                    .publication
                    .as_ref()
                    .and_then(reference_day)
                    .map(|day| (day, *index))
            })
            .collect();
        dated.sort();
        (dated.len() >= 2).then(|| (dated[0].1, dated[dated.len() - 1].1))
    };
    Targets { claims, between }
}

/// Each reader of the table that takes a cutoff, by name, on one World.
fn read_all(spec: &Spec, built: &Built, cutoff: NaiveDate) -> Vec<(String, Answer)> {
    let targets = targets(spec, cutoff);
    let mut answers = Vec::new();
    let world = built.world.clone();
    answers.push((
        "world_read.view".to_owned(),
        run_tool(
            built,
            Some(cutoff),
            ToolMethod::WorldRead,
            json!({"scope": "world", "recordId": world}),
        ),
    ));
    for claim in &targets.claims {
        answers.push((
            format!("world_read.history {claim}"),
            run_tool(
                built,
                Some(cutoff),
                ToolMethod::WorldRead,
                json!({"scope": "world", "recordId": world, "centerClaimId": claim, "history": true}),
            ),
        ));
    }
    let mut search = run_tool(
        built,
        Some(cutoff),
        ToolMethod::WorldSearch,
        json!({"query": SEARCH_WORD}),
    );
    sort_hits(&mut search.result);
    answers.push(("world_search.hits".to_owned(), search));
    for axis in ["recorded", "published"] {
        answers.push((
            format!("world_changes.{axis}"),
            run_tool(
                built,
                Some(cutoff),
                ToolMethod::WorldChanges,
                json!({"since": "2025-12-01", "axis": axis, "limit": 50}),
            ),
        ));
    }
    if let Some((from, to)) = targets.between {
        if let (Some(from), Some(to)) = (built.sources.get(&from), built.sources.get(&to)) {
            answers.push((
                "world_changes.between".to_owned(),
                run_tool(
                    built,
                    Some(cutoff),
                    ToolMethod::WorldChanges,
                    json!({"between": {"to": to.record_id, "from": from.record_id}}),
                ),
            ));
        }
    }
    answers.extend(renderer_reads(built, Some(cutoff)));
    for claim in &targets.claims {
        let history = super::world_changes::claim_history(
            &built.state,
            serde_json::from_value(json!({
                "loupistId": LOUPIST, "claimId": claim, "asOf": cutoff.to_string(),
            }))
            .unwrap(),
        );
        answers.push((
            format!("knowledge_world_claim_history.asOf {claim}"),
            Answer {
                result: history.map_or_else(
                    |error| json!({"error": error.code.as_str()}),
                    |history| serde_json::to_value(history).unwrap(),
                ),
                receipts: Vec::new(),
                staged: String::new(),
            },
        ));
    }
    if let Some(lens) = &built.lens {
        answers.push((
            "lens_read.sections".to_owned(),
            run_tool(
                built,
                Some(cutoff),
                ToolMethod::LensRead,
                json!({"recordId": lens}),
            ),
        ));
    }
    answers
}

/// World's renderer reads under its As of date: the World view, the map and
/// the change feed History reads. Under no date they read today's World.
fn renderer_reads(built: &Built, cutoff: Option<NaiveDate>) -> Vec<(String, Answer)> {
    let as_of = cutoff.map(|day| day.to_string());
    let answer = |result: Result<Value, crate::app::CommandError>| Answer {
        result: result.unwrap_or_else(|error| json!({"error": error.code.as_str()})),
        receipts: Vec::new(),
        staged: String::new(),
    };
    let query: crate::knowledge::WorldGraphQuery = serde_json::from_value(json!({
        "loupistId": LOUPIST, "recordId": built.world, "revisionId": "", "asOf": as_of,
    }))
    .unwrap();
    let view =
        super::world_query(&built.state, query).map(|view| serde_json::to_value(view).unwrap());
    let map = super::map(
        &built.state,
        crate::knowledge::KnowledgeMapQuery {
            loupist_id: LOUPIST.into(),
            max_nodes: None,
            as_of: as_of.clone(),
        },
    )
    .map(|map| {
        let mut map = serde_json::to_value(map).unwrap();
        // The stored time of the newest item, not a fact of the World.
        map.as_object_mut().unwrap().remove("generatedAtMs");
        map
    });
    let changes = super::world_changes::read_world_changes(
        &built.state,
        serde_json::from_value(json!({
            "loupistId": LOUPIST, "since": "2025-12-01", "axis": "published", "limit": 50, "asOf": as_of,
        }))
        .unwrap(),
    )
    .map(|view| serde_json::to_value(view).unwrap());
    vec![
        ("knowledge_world_query.asOf".to_owned(), answer(view)),
        ("knowledge_map.asOf".to_owned(), answer(map)),
        ("knowledge_world_changes.asOf".to_owned(), answer(changes)),
    ]
}

fn sort_hits(result: &mut Value) {
    if let Some(hits) = result.get_mut("hits").and_then(Value::as_array_mut) {
        for hit in hits.iter_mut() {
            if let Some(object) = hit.as_object_mut() {
                for key in [
                    "score",
                    "rank",
                    "rrf",
                    "bm25",
                    "semanticScore",
                    "retrieval",
                    "matched_terms",
                ] {
                    object.remove(key);
                }
            }
        }
        hits.sort_by_key(|hit| hit.to_string());
    }
}

// ---------------------------------------------------------------------------
// The properties
// ---------------------------------------------------------------------------

fn compare(spec: &Spec, cutoff: NaiveDate, full: &Built, prefix: &Built) -> Result<(), String> {
    let left = read_all(spec, full, cutoff);
    let right = read_all(spec, prefix, cutoff);
    let mut differences = Vec::new();
    for ((name, a), (_, b)) in left.iter().zip(&right) {
        let x = normalize(
            &json!({"result": a.result, "receipts": a.receipts}),
            &full.names,
        );
        let y = normalize(
            &json!({"result": b.result, "receipts": b.receipts}),
            &prefix.names,
        );
        if let Some(difference) = first_difference(&x, &y, "") {
            if std::env::var("LOUPIST_ORACLE_DUMP").is_ok_and(|value| value == "1") {
                eprintln!("FULL {name} {cutoff}: {x}\nPREFIX {name} {cutoff}: {y}");
            }
            differences.push(format!("P1 {name} at cutoff {cutoff}: {difference}"));
        }
    }
    if differences.is_empty() {
        Ok(())
    } else {
        Err(differences.join("\n"))
    }
}

#[test]
fn p1_every_reader_under_a_cutoff_equals_the_prefix_world() {
    for_each_seed("p1", |seed| {
        let spec = Spec::generate(seed);
        let full = build(&spec, &spec.plan());
        let mut differences = Vec::new();
        for cutoff in &spec.cutoffs {
            let prefix = build(&spec, &spec.prefix_plan(*cutoff));
            if let Err(found) = compare(&spec, *cutoff, &full, &prefix) {
                differences.push(found);
            }
        }
        if differences.is_empty() {
            Ok(())
        } else {
            Err(differences.join("\n"))
        }
    });
}

/// P6: what a renderer read under a cutoff reports as left out is what the
/// prefix World lacks: the relationships of today's World it does not hold,
/// and the Sources asserting a relationship that the cutoff does not know. The
/// prefix World under the same cutoff leaves nothing out.
#[test]
fn p6_an_as_of_report_counts_what_the_prefix_world_lacks() {
    let edges = |built: &Built| {
        let answers = renderer_reads(built, None);
        answers[0].1.result["edges"].as_array().map_or(0, Vec::len)
    };
    let report = |built: &Built, cutoff: NaiveDate, row: usize| {
        renderer_reads(built, Some(cutoff))[row].1.result["asOfReport"].clone()
    };
    for_each_seed("p6", |seed| {
        let spec = Spec::generate(seed);
        let full = build(&spec, &spec.plan());
        let today = edges(&full);
        for cutoff in &spec.cutoffs {
            let prefix = build(&spec, &spec.prefix_plan(*cutoff));
            let asserts = |index: usize| spec.reset != Some(index);
            let later_sources: BTreeSet<usize> = spec
                .claims
                .iter()
                .flat_map(|claim| claim.asserted_by.iter().copied())
                .filter(|by| asserts(*by) && !spec.known_by(*by, *cutoff))
                .collect();
            let expected = json!({
                "facts": today - edges(&prefix),
                "sources": later_sources.len(),
            });
            for row in [0, 1] {
                let found = report(&full, *cutoff, row);
                if found != expected {
                    return Err(format!(
                        "P6 row {row} at cutoff {cutoff}: {found}, expected {expected}"
                    ));
                }
                let none = report(&prefix, *cutoff, row);
                if none != json!({"facts": 0, "sources": 0}) {
                    return Err(format!("P6 prefix row {row} at cutoff {cutoff}: {none}"));
                }
            }
        }
        Ok(())
    });
}

/// Canaries of the Sources a cutoff has not reached and of a removed Source.
fn hidden_canaries(spec: &Spec, cutoff: NaiveDate) -> Vec<&str> {
    spec.sources
        .iter()
        .enumerate()
        .filter(|(index, _)| !spec.known_by(*index, cutoff) || spec.reset == Some(*index))
        .map(|(_, source)| source.canary.as_str())
        .collect()
}

#[test]
fn p2_no_later_or_removed_source_reaches_a_result_or_its_receipt() {
    for_each_seed("p2", |seed| {
        let spec = Spec::generate(seed);
        let full = build(&spec, &spec.plan());
        for cutoff in &spec.cutoffs {
            let hidden = hidden_canaries(&spec, *cutoff);
            let mut answers = read_all(&spec, &full, *cutoff);
            // A later Source read by its id is refused without its text.
            for (index, source) in &full.sources {
                if spec.known_by(*index, *cutoff) {
                    continue;
                }
                answers.push((
                    format!("world_read.source S{index}"),
                    run_tool(
                        &full,
                        Some(*cutoff),
                        ToolMethod::WorldRead,
                        json!({"scope": "source", "recordId": source.record_id, "spanId": source.span_id}),
                    ),
                ));
                answers.push((
                    format!("world_changes.between to S{index}"),
                    run_tool(
                        &full,
                        Some(*cutoff),
                        ToolMethod::WorldChanges,
                        json!({"between": {"to": source.record_id}}),
                    ),
                ));
            }
            for (name, answer) in answers {
                let text = format!("{} {:?} {}", answer.result, answer.receipts, answer.staged);
                if let Some(canary) = hidden.iter().find(|canary| text.contains(**canary)) {
                    return Err(format!(
                        "P2 {name} at cutoff {cutoff} shows {canary}: {}",
                        short(&answer.result)
                    ));
                }
            }
        }
        Ok(())
    });
}

/// The claims of the World view as the order-independent facts: evidence,
/// ending, conflicts and status, without system times.
fn claim_states(built: &Built, cutoff: Option<NaiveDate>, keep_status: &BTreeSet<String>) -> Value {
    let answer = run_tool(
        built,
        cutoff,
        ToolMethod::WorldRead,
        json!({"scope": "world", "recordId": built.world}),
    );
    let view = normalize(&answer.result["text"], &built.names);
    let mut rows = BTreeMap::new();
    for edge in view["edges"].as_array().into_iter().flatten() {
        let claim = &edge["claim"];
        let id = claim["id"].as_str().unwrap_or_default().to_owned();
        let mut evidence: Vec<String> = claim["evidence"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["spanId"].as_str().map(str::to_owned))
            .collect();
        evidence.sort();
        let mut qualifiers: Vec<String> = claim["qualifiers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(Value::to_string)
            .collect();
        qualifiers.sort();
        let mut row = json!({
            "evidence": evidence,
            "qualifiers": qualifiers,
            "invalidAt": claim["invalidAt"],
            "publicationTime": claim["publicationTime"],
            "ended": edge["ended"],
            "end": edge["end"],
        });
        if keep_status.contains(&id) {
            row["status"] = edge["status"].clone();
        } else {
            // `ended` also reads the status; only `end` is the ending here.
            row["ended"] = Value::Null;
        }
        rows.insert(id, row);
    }
    json!(rows)
}

#[test]
fn p3_every_ingestion_order_gives_one_world() {
    for_each_seed("p3", |seed| {
        let spec = Spec::generate(seed);
        let mut transitions: BTreeMap<usize, usize> = BTreeMap::new();
        for status in &spec.statuses {
            *transitions.entry(status.claim).or_default() += 1;
        }
        let keep: BTreeSet<String> = spec
            .claims
            .iter()
            .enumerate()
            // A status needs its witness read after the claim exists, so it
            // follows reading order; only claims without one are compared.
            .filter(|(index, _)| !transitions.contains_key(index))
            .map(|(_, claim)| claim.id.clone())
            .collect();
        let reference = build(&spec, &spec.plan());
        let mut rng = Rng::new(seed ^ 0x5eed);
        let mut order: Vec<usize> = (0..spec.sources.len()).collect();
        rng.shuffle(&mut order);
        let latest = spec
            .sources
            .iter()
            .map(|source| source.ingest_at_ms)
            .max()
            .unwrap_or(0);
        let shuffled = Plan {
            ingest: order
                .iter()
                .enumerate()
                .map(|(step, index)| (*index, latest + 3_600_000 * (step as i64 + 1)))
                .collect(),
            lens_at: None,
            reset: spec.reset,
        };
        let other = build(&spec, &shuffled);
        for cutoff in std::iter::once(None).chain(spec.cutoffs.iter().copied().map(Some)) {
            let a = claim_states(&reference, cutoff, &keep);
            let b = claim_states(&other, cutoff, &keep);
            if let Some(difference) = first_difference(&a, &b, "") {
                return Err(format!(
                    "P3 order {order:?} at cutoff {cutoff:?}: {difference}"
                ));
            }
        }
        Ok(())
    });
}

fn feed_rows(answer: &Answer) -> Vec<Value> {
    answer.result["changes"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .chain(
            answer.result["lens"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        )
        .collect()
}

#[test]
fn p4_paging_a_feed_yields_each_row_once() {
    for_each_seed("p4", |seed| {
        let spec = Spec::generate(seed);
        let full = build(&spec, &spec.plan());
        let cutoffs = std::iter::once(None).chain(spec.cutoffs.iter().copied().map(Some));
        for cutoff in cutoffs {
            for axis in ["recorded", "published"] {
                let whole = run_tool(
                    &full,
                    cutoff,
                    ToolMethod::WorldChanges,
                    json!({"since": "2025-12-01", "axis": axis, "limit": 50}),
                );
                let expected: Vec<Value> = feed_rows(&whole)
                    .iter()
                    .map(|row| normalize(row, &full.names))
                    .collect();
                let mut seen = Vec::new();
                let mut since = json!("2025-12-01");
                for _ in 0..200 {
                    let page = run_tool(
                        &full,
                        cutoff,
                        ToolMethod::WorldChanges,
                        json!({"since": since, "axis": axis, "limit": 2}),
                    );
                    if page.result.get("error").is_some() {
                        return Err(format!("P4 {axis} {cutoff:?}: {}", page.result));
                    }
                    seen.extend(
                        feed_rows(&page)
                            .iter()
                            .map(|row| normalize(row, &full.names)),
                    );
                    if page.result["capped"] != true {
                        break;
                    }
                    since = page.result["next"].clone();
                }
                let key = |rows: &[Value]| {
                    let mut keys: Vec<String> = rows.iter().map(Value::to_string).collect();
                    keys.sort();
                    keys
                };
                if key(&seen) != key(&expected) {
                    return Err(format!(
                        "P4 {axis} at {cutoff:?}: pages {} rows, one page {} rows\npages: {}\nwhole: {}",
                        seen.len(),
                        expected.len(),
                        short(&json!(seen)),
                        short(&json!(expected))
                    ));
                }
            }
        }
        Ok(())
    });
}

/// P1b: on the prefix World the cutoff only applies the effective-date rule,
/// written here from D2-2: an ending shows when its effective end is at or
/// before the cutoff, and then it shows with all of its fields; otherwise
/// `invalid_at`, `expired_at`, the ending qualifiers and the flag go together.
#[test]
fn p1b_on_the_prefix_world_a_cutoff_hides_only_endings_not_yet_effective() {
    for_each_seed("p1b", |seed| {
        let spec = Spec::generate(seed);
        if spec.reset.is_some() {
            // A reset World: P5 states what the live view keeps of it.
            return Ok(());
        }
        for cutoff in &spec.cutoffs {
            let prefix = build(&spec, &spec.prefix_plan(*cutoff));
            let everyone: BTreeSet<String> =
                spec.claims.iter().map(|claim| claim.id.clone()).collect();
            let under = claim_states(&prefix, Some(*cutoff), &everyone);
            let now = claim_states(&prefix, None, &everyone);
            let mut expected = now.clone();
            for (_, row) in expected.as_object_mut().into_iter().flatten() {
                let effective = row["invalidAt"]
                    .get("value")
                    .and_then(Value::as_str)
                    .and_then(|_| {
                        serde_json::from_value::<TimePoint>(row["invalidAt"].clone()).ok()
                    })
                    .as_ref()
                    .and_then(reference_day)
                    .is_some_and(|day| day <= *cutoff);
                if !effective {
                    row["invalidAt"] = Value::Null;
                    // A status other than active still reads as ended.
                    row["ended"] = if row["status"] == "active" {
                        Value::Null
                    } else {
                        Value::Bool(true)
                    };
                    row["end"] = Value::Null;
                    if let Some(qualifiers) = row["qualifiers"].as_array_mut() {
                        qualifiers.retain(|item| {
                            let text = item.as_str().unwrap_or_default();
                            !(text.contains(END_BASIS)
                                || text.contains(SUPERSEDED_BY_SPAN)
                                || text.contains(SUPERSEDED_BY_CLAIM))
                        });
                    }
                }
            }
            if let Some(difference) = first_difference(&under, &expected, "") {
                return Err(format!("P1b at cutoff {cutoff}: {difference}"));
            }
        }
        Ok(())
    });
}

/// P5: a Source a Memory reset removed leaves nothing it alone justified: the
/// live World after the reset holds the claims, evidence, assertion times,
/// endings and conflict links of a World that never read that Source. Status
/// is not compared: a dispute or withdrawal that Source's reading wrote stays,
/// a known limit of reset (it removes support, not transitions).
#[test]
fn p5_a_reset_source_leaves_no_ending_or_conflict_in_the_live_world() {
    for_each_seed("p5", |seed| {
        let spec = Spec::generate(seed);
        let Some(reset) = spec.reset else {
            return Ok(());
        };
        let built = build(&spec, &spec.plan());
        let mut never = spec.plan();
        never.ingest.retain(|(index, _)| *index != reset);
        never.reset = None;
        let unread = build(&spec, &never);
        let none = BTreeSet::new();
        let after = claim_states(&built, None, &none);
        let without = claim_states(&unread, None, &none);
        if let Some(difference) = first_difference(&after, &without, "") {
            return Err(format!(
                "P5 reset World against a World that never read S{reset}: {difference}"
            ));
        }
        Ok(())
    });
}

/// The Lens rows of a feed or an asset page, as (section, state, claim ids).
fn lens_alerts(rows: Option<&Value>) -> Vec<(String, String, Vec<String>)> {
    rows.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| {
            (
                format!(
                    "{}#{}",
                    row["recordId"].as_str().unwrap_or_default(),
                    row["sectionKey"].as_str().unwrap_or_default()
                ),
                row["state"].as_str().unwrap_or_default().to_owned(),
                row["claimIds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            )
        })
        .collect()
}

/// P7 (lane R, c13ca29): a Lens alert names only the claims its transition
/// newly attached. In the dated rows of the change feed and an asset page
/// under every cutoff (what a check's replay reads), each row names a claim,
/// and no claim appears in two rows of one section and state, so a second
/// claim never repeats the first one's alert. The live feed shows each
/// section once, in its current state.
#[test]
fn p7_a_lens_alert_names_each_newly_attached_claim_once() {
    for_each_seed("p7", |seed| {
        let spec = Spec::generate(seed);
        if spec.lens.is_none() {
            return Ok(());
        }
        let built = build(&spec, &spec.plan());
        let asset = findings::follow(&built);
        for cutoff in spec.cutoffs.iter().copied().map(Some) {
            let mut reads = Vec::new();
            for axis in ["recorded", "published"] {
                let feed = run_tool(
                    &built,
                    cutoff,
                    ToolMethod::WorldChanges,
                    json!({"since": "2025-12-01", "axis": axis, "limit": 50}),
                );
                reads.push((
                    format!("world_changes.{axis}"),
                    lens_alerts(feed.result.get("lens")),
                ));
            }
            let page = run_tool(
                &built,
                cutoff,
                ToolMethod::AssetPageRead,
                json!({"assetId": asset}),
            );
            reads.push((
                "asset_page_read.sinceRead".to_owned(),
                lens_alerts(page.result.pointer("/sinceRead/lens")),
            ));
            for (name, alerts) in reads {
                let mut seen = BTreeSet::new();
                for (section, state, claims) in alerts {
                    if claims.is_empty() {
                        return Err(format!(
                            "P7 {name} at {cutoff:?}: a {state} row of {section} names no claim"
                        ));
                    }
                    for claim in claims {
                        if !seen.insert((section.clone(), state.clone(), claim.clone())) {
                            return Err(format!(
                                "P7 {name} at {cutoff:?}: {claim} alerts twice as {state} in {section}"
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    });
}

/// P8 (World loop T-SAMEDAY, F3): a Lens section that cites a claim whose
/// stated end took effect before the last reading is not Current, unless it
/// answers the ending: it cites the witness's span, or a claim or span
/// published after the witness (a ref of the same day does not). Live, on the
/// full World of seeds whose endings no link, status, recapture or reset moves.
#[test]
fn p8_a_section_citing_an_ended_claim_is_not_current_unless_it_answers_the_ending() {
    for_each_seed("p8", |seed| {
        let spec = Spec::generate(seed);
        if spec.lens.is_none()
            || spec.reset.is_some()
            || !spec.links.is_empty()
            || !spec.statuses.is_empty()
            || !spec.recaptures.is_empty()
        {
            return Ok(());
        }
        let built = build(&spec, &spec.plan());
        let Some(record) = built.lens.clone() else {
            return Ok(());
        };
        let last_read = spec
            .sources
            .iter()
            .map(|source| source.ingest_at_ms)
            .max()
            .and_then(crate::local_time::date_at_ms);
        let page = run_tool(
            &built,
            None,
            ToolMethod::LensRead,
            json!({"recordId": record}),
        );
        let text: Value = page.result["text"]
            .as_str()
            .and_then(|text| serde_json::from_str(text).ok())
            .unwrap_or(Value::Null);
        let source_of = |record_id: &str| {
            built
                .sources
                .iter()
                .find(|(_, retained)| retained.record_id == record_id)
                .map(|(index, _)| *index)
        };
        let claim_publication = |id: &str| {
            spec.claims
                .iter()
                .find(|claim| claim.id == id)
                .into_iter()
                .flat_map(|claim| claim.asserted_by.iter())
                .filter_map(|by| spec.sources[*by].publication.clone())
                .min_by(|a, b| {
                    a.compare_at_common_precision(b)
                        .unwrap_or_else(|| a.value.cmp(&b.value))
                })
        };
        for section in text["body"]["sections"].as_array().into_iter().flatten() {
            if section["contested"] == true || !section["claimIds"].is_null() {
                continue;
            }
            let refs = section["basis"]["refs"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let claims: Vec<&str> = refs.iter().filter_map(|r| r["claimId"].as_str()).collect();
            let spans: Vec<usize> = refs
                .iter()
                .filter_map(|r| r["recordId"].as_str())
                .filter_map(source_of)
                .collect();
            for id in &claims {
                let Some(claim) = spec.claims.iter().find(|claim| claim.id == *id) else {
                    continue;
                };
                let Some((witness, end)) = &claim.stated_end else {
                    continue;
                };
                if reference_day(end)
                    .zip(last_read)
                    .is_none_or(|(end, last)| end >= last)
                {
                    continue;
                }
                let witnessed = spec.sources[*witness].publication.clone();
                let later = |time: Option<TimePoint>| {
                    time.zip(witnessed.as_ref())
                        .is_some_and(|(time, witnessed)| {
                            witnessed.compare_at_common_precision(&time)
                                == Some(std::cmp::Ordering::Less)
                        })
                };
                let answered = spans.contains(witness)
                    || spans
                        .iter()
                        .any(|source| later(spec.sources[*source].publication.clone()))
                    || claims
                        .iter()
                        .filter(|other| *other != id)
                        .any(|other| later(claim_publication(other)));
                if !answered {
                    return Err(format!(
                        "P8 section {} is Current though {id} ended by S{witness} and nothing it cites answers the ending",
                        section["key"]
                    ));
                }
            }
        }
        Ok(())
    });
}

// ---------------------------------------------------------------------------
// Completeness
// ---------------------------------------------------------------------------

/// Tool names whose catalog permission is `memory_read`.
pub(in crate::commands) fn catalog_memory_reads() -> BTreeSet<String> {
    let catalog: Value =
        serde_json::from_str(include_str!("../../../../agent/tool-catalog.json")).unwrap();
    catalog["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|tool| tool["permission"] == "memory_read")
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect()
}

/// `knowledge_*` commands the app registers in `generate_handler!`.
pub(in crate::commands) fn registered_knowledge_commands() -> BTreeSet<String> {
    let source = include_str!("../../lib.rs");
    let start = source
        .find("generate_handler![")
        .expect("generate_handler!");
    let end = start
        + source[start..]
            .find("])")
            .expect("end of generate_handler!");
    source[start..end]
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == ':'))
        .filter_map(|token| token.rsplit("::").next())
        .filter(|name| name.starts_with("knowledge_"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_reader_table_names_every_memory_read() {
    let named: BTreeSet<&str> = READERS
        .iter()
        .flat_map(|row| row.entry.split(' '))
        .collect();
    let missing: Vec<String> = catalog_memory_reads()
        .into_iter()
        .chain(registered_knowledge_commands())
        .filter(|name| !named.contains(name.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "reads with no row in READERS: {missing:?}"
    );
    for row in READERS {
        if let Covered::Same(other) = row.how {
            assert!(
                READERS.iter().any(|candidate| candidate.name == other),
                "{} names a missing row {other}",
                row.name
            );
        }
    }
    // A cutoff row is exercised by read_all or by a named test below.
    let exercised = [
        "world_read.view",
        "world_read.history",
        "world_read.source",
        "world_read.receipt",
        "world_search.hits",
        "world_changes.recorded",
        "world_changes.published",
        "world_changes.pages",
        "world_changes.between",
        "world_changes.receipt",
        "asset_page_read.sinceRead",
        "lens_read.sections",
        "lens_search.hits",
        "knowledge_world_query.asOf",
        "knowledge_map.asOf",
        "knowledge_world_changes.asOf",
        "knowledge_world_claim_history.asOf",
    ];
    for row in READERS
        .iter()
        .filter(|row| matches!(row.how, Covered::Cutoff))
    {
        assert!(
            exercised.contains(&row.name),
            "{} has no property run",
            row.name
        );
    }
}

// ---------------------------------------------------------------------------
// Review findings of the cutoff class, one test each, named by round and
// number (REVIEW-report.md, REVIEW-rereview-81afcb07.txt, R3.txt). Each is
// written from the finding's statement, not from its fix.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod findings {
    use super::*;

    fn day(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
    }

    fn text(answer: &Answer) -> String {
        format!("{} {:?} {}", answer.result, answer.receipts, answer.staged)
    }

    fn view(answer: &Answer) -> Value {
        let text = answer.result["text"].as_str().unwrap_or("{}");
        serde_json::from_str(&undelimited(text)).unwrap_or(Value::Null)
    }

    fn edge<'a>(view: &'a Value, id: &str) -> Option<&'a Value> {
        view["edges"]
            .as_array()?
            .iter()
            .find(|edge| edge["claim"]["id"] == id || edge["id"] == id)
    }

    fn rows<'a>(answer: &'a Answer, kind: &str) -> Vec<&'a Value> {
        answer.result["changes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|row| row["kind"] == kind)
            .collect()
    }

    fn publisher(canary: &str, published: &str, read: &str) -> SourceSpec {
        source(canary, Authorship::Publisher, Some(published), read)
    }

    /// The CFO example of D2-2: a January filing names the CFO, a May filing
    /// says she left on March 15.
    fn cfo() -> Spec {
        let mut cfo = claim("cfo", [2, 0], &[0, 1]);
        cfo.stated_end = Some((1, date("2026-03-15")));
        Spec::manual(
            vec![
                publisher("janfiling", "2026-01-15", "2026-01-16"),
                publisher("mayfiling", "2026-05-01", "2026-05-02"),
            ],
            vec![cfo],
        )
    }

    const APRIL: &str = "2026-04-01";

    /// World's As of control (extension plan 9.6): a date before the end
    /// shows the ended claim as current, and the report counts the later
    /// Source; today the same read shows it ended.
    #[test]
    fn world_as_of_shows_a_claim_ended_later_as_current_and_counts_the_later_source() {
        let spec = cfo();
        let built = build(&spec, &spec.plan());
        let edge_of = |reads: &[(String, Answer)]| reads[0].1.result["edges"][0].clone();
        let today = renderer_reads(&built, None);
        assert_eq!(edge_of(&today)["ended"], true, "{}", today[0].1.result);
        assert!(today[0].1.result.get("asOfReport").is_none());
        let april = renderer_reads(&built, Some(day(APRIL)));
        let edge = edge_of(&april);
        assert!(edge["ended"].is_null(), "{edge}");
        assert!(edge["end"].is_null(), "{edge}");
        // The May filing restated the claim: no relationship is left out, one Source is.
        assert_eq!(
            april[0].1.result["asOfReport"],
            json!({"facts": 0, "sources": 1})
        );
        assert_eq!(
            april[1].1.result["asOfReport"],
            april[0].1.result["asOfReport"]
        );
        assert!(!april
            .iter()
            .any(|(_, answer)| answer.result.to_string().contains("mayfiling")));
    }

    /// A day after today is refused with its own code by every World read
    /// that takes a date, and by a chat turn.
    #[test]
    fn a_future_as_of_is_refused_with_as_of_future() {
        let spec = cfo();
        let built = build(&spec, &spec.plan());
        let tomorrow = crate::local_time::today().succ_opt().unwrap();
        for (name, answer) in renderer_reads(&built, Some(tomorrow)) {
            assert_eq!(answer.result, json!({"error": "as_of_future"}), "{name}");
        }
        let history = super::super::world_changes::claim_history(
            &built.state,
            serde_json::from_value(
                json!({"loupistId": LOUPIST, "claimId": "cfo", "asOf": tomorrow.to_string()}),
            )
            .unwrap(),
        );
        assert_eq!(history.unwrap_err().code.as_str(), "as_of_future");
        let turn = super::super::as_of::Cutoff::requested(Some(&tomorrow.to_string()));
        assert_eq!(turn.unwrap_err().code.as_str(), "as_of_future");
        assert!(super::super::as_of::Cutoff::requested(Some("1989-12-31"))
            .is_err_and(|error| error.code.as_str() == "invalid_request"));
    }

    #[test]
    fn r1_03_april_history_has_no_may_title_passage_or_ending() {
        let spec = cfo();
        let built = build(&spec, &spec.plan());
        let answer = run_tool(
            &built,
            Some(day(APRIL)),
            ToolMethod::WorldRead,
            json!({"scope": "world", "recordId": built.world, "centerClaimId": "cfo", "history": true}),
        );
        let lineage = &answer.result["lineage"];
        assert!(lineage.is_object(), "{}", answer.result);
        assert!(!text(&answer).contains("mayfiling"), "{}", answer.result);
        assert!(lineage["ended"].is_null(), "{lineage}");
        assert!(
            !lineage.to_string().contains(&built.sources[&1].record_id),
            "{lineage}"
        );
    }

    #[test]
    fn r1_04_a_march_end_disclosed_in_may_is_not_on_the_april_edge_receipt_or_hit() {
        let spec = cfo();
        let built = build(&spec, &spec.plan());
        let read = run_tool(
            &built,
            Some(day(APRIL)),
            ToolMethod::WorldRead,
            json!({"scope": "world", "recordId": built.world}),
        );
        let shown = view(&read);
        let cfo = edge(&shown, "cfo").expect("the CFO is known in April");
        assert!(cfo["claim"]["invalidAt"].is_null(), "{cfo}");
        assert!(cfo.get("ended").is_none_or(Value::is_null), "{cfo}");
        for receipt in &read.receipts {
            let cfo = edge(receipt, "cfo").expect("the receipt has the CFO");
            assert!(cfo["claim"]["invalidAt"].is_null(), "{cfo}");
            assert!(!cfo.to_string().contains("2026-03-15"), "{cfo}");
        }
        let hits = run_tool(
            &built,
            Some(day(APRIL)),
            ToolMethod::WorldSearch,
            json!({"query": SEARCH_WORD}),
        );
        assert!(
            !hits.result.to_string().contains("2026-03-15"),
            "{}",
            hits.result
        );
        assert!(!text(&hits).contains("mayfiling"), "{}", hits.result);
    }

    #[test]
    fn r1_05_a_later_or_reset_report_is_not_compared_by_title_or_date() {
        let spec = cfo();
        let built = build(&spec, &spec.plan());
        let later = run_tool(
            &built,
            Some(day(APRIL)),
            ToolMethod::WorldChanges,
            json!({"between": {"to": built.sources[&1].record_id, "from": built.sources[&0].record_id}}),
        );
        assert!(!text(&later).contains("mayfiling"), "{}", later.result);
        assert!(
            !later.result.to_string().contains("2026-05-01"),
            "{}",
            later.result
        );
        let mut reset = cfo();
        reset.reset = Some(1);
        let built = build(&reset, &reset.plan());
        let hidden = run_tool(
            &built,
            None,
            ToolMethod::WorldChanges,
            json!({"between": {"to": built.sources[&1].record_id, "from": built.sources[&0].record_id}}),
        );
        assert!(!text(&hidden).contains("mayfiling"), "{}", hidden.result);
        assert!(
            !hidden.result.to_string().contains("2026-05-01"),
            "{}",
            hidden.result
        );
    }

    /// Two pre-May reports support the CFO; the May report ends her term in March.
    fn two_reports_then_may() -> Spec {
        let mut cfo = claim("cfo", [2, 0], &[0, 1, 2]);
        cfo.stated_end = Some((2, date("2026-03-15")));
        Spec::manual(
            vec![
                publisher("febreport", "2026-02-01", "2026-02-02"),
                publisher("marreport", "2026-03-01", "2026-03-02"),
                publisher("mayreport", "2026-05-01", "2026-05-02"),
            ],
            vec![cfo],
        )
    }

    fn april_between(built: &Built) -> Answer {
        run_tool(
            built,
            Some(day("2026-04-30")),
            ToolMethod::WorldChanges,
            json!({"between": {"to": built.sources[&1].record_id, "from": built.sources[&0].record_id}}),
        )
    }

    #[test]
    fn r1_06_an_april_comparison_of_two_earlier_reports_has_no_end_first_established_in_may() {
        let spec = two_reports_then_may();
        let built = build(&spec, &spec.plan());
        let answer = april_between(&built);
        assert!(
            answer.result["ended_by_to"]
                .as_array()
                .is_some_and(Vec::is_empty),
            "{}",
            answer.result
        );
        assert!(
            !answer.result.to_string().contains("2026-03-15"),
            "{}",
            answer.result
        );
        assert!(!text(&answer).contains("mayreport"), "{}", answer.result);
    }

    #[test]
    fn r1_07_a_link_first_selected_by_a_may_restatement_does_not_end_a_claim_in_april() {
        let mut spec = Spec::manual(
            vec![
                publisher("janlink", "2026-01-10", "2026-01-11"),
                publisher("feblink", "2026-02-10", "2026-02-11"),
                publisher("maylink", "2026-05-10", "2026-05-11"),
            ],
            vec![claim("old", [2, 0], &[0]), claim("new", [3, 0], &[1, 2])],
        );
        spec.links.push(LinkSpec {
            left: 0,
            right: 1,
            selected_by: 2,
            same_by: Some(2),
            also_same_by: Vec::new(),
        });
        let built = build(&spec, &spec.plan());
        let now = view(&run_tool(
            &built,
            None,
            ToolMethod::WorldRead,
            json!({"scope": "world", "recordId": built.world}),
        ));
        assert!(
            edge(&now, "old").is_some_and(|edge| !edge["claim"]["invalidAt"].is_null()),
            "the May selection ends it today: {now}"
        );
        let april = run_tool(
            &built,
            Some(day("2026-04-30")),
            ToolMethod::WorldRead,
            json!({"scope": "world", "recordId": built.world}),
        );
        let shown = view(&april);
        let old = edge(&shown, "old").expect("old is known in April");
        assert!(old["claim"]["invalidAt"].is_null(), "{old}");
        assert!(!old.to_string().contains(CONFLICTS_WITH_CLAIM), "{old}");
    }

    #[test]
    fn r1_08_a_backfilled_episode_does_not_name_a_conflict_partner_published_later() {
        let mut spec = Spec::manual(
            vec![
                // Published in January, read in June: a backfill.
                publisher("janbackfill", "2026-01-10", "2026-06-01"),
                publisher("maypartner", "2026-05-10", "2026-05-11"),
            ],
            vec![
                claim("early", [2, 0], &[0]),
                claim("partnerlater", [3, 0], &[1]),
            ],
        );
        spec.links.push(LinkSpec {
            left: 0,
            right: 1,
            selected_by: 0,
            same_by: None,
            also_same_by: Vec::new(),
        });
        let built = build(&spec, &spec.plan());
        for axis in ["published", "recorded"] {
            let answer = run_tool(
                &built,
                Some(day("2026-04-30")),
                ToolMethod::WorldChanges,
                json!({"since": "2025-12-01", "axis": axis}),
            );
            assert!(
                !text(&answer).contains("partnerlater"),
                "{axis}: {}",
                answer.result
            );
        }
    }

    #[test]
    fn r1_09_a_may_withdrawal_is_not_in_the_february_published_feed() {
        let mut spec = Spec::manual(
            vec![
                publisher("janclaim", "2026-01-10", "2026-01-11"),
                publisher("maywithdraw", "2026-05-10", "2026-05-11"),
            ],
            vec![claim("held", [2, 0], &[0])],
        );
        spec.statuses.push(StatusSpec {
            claim: 0,
            status: ClaimStatus::Withdrawn,
            witness: 1,
        });
        let built = build(&spec, &spec.plan());
        let answer = run_tool(
            &built,
            Some(day("2026-02-28")),
            ToolMethod::WorldChanges,
            json!({"since": "2025-12-01", "axis": "published"}),
        );
        assert!(rows(&answer, "withdrawn").is_empty(), "{}", answer.result);
        for row in answer.result["changes"].as_array().into_iter().flatten() {
            assert_eq!(row["status"], "active", "{row}");
        }
    }

    /// An asset followed on the first company, its page linked to that node.
    pub(in crate::commands) fn follow(built: &Built) -> String {
        let asset = built
            .state
            .store
            .asset_follow_name(
                LOUPIST,
                &fixed_nodes()[0].name,
                None,
                crate::commands::assets::wire::AssetReferenceOrigin::User,
                1,
            )
            .unwrap();
        crate::commands::assets::with_assets(&built.state, LOUPIST, true, |tx| {
            crate::knowledge::assets::ensure_asset_page(tx, LOUPIST, &asset, 1)
        })
        .unwrap();
        built
            .state
            .store
            .lock_loupist(LOUPIST)
            .unwrap()
            .execute(
                "UPDATE knowledge_asset_pages SET entity_id=?3 WHERE loupist_id=?1 AND asset_id=?2",
                rusqlite::params![LOUPIST, asset.asset_id, "node-c0"],
            )
            .unwrap();
        asset.asset_id
    }

    #[test]
    fn r1_10_an_april_asset_read_has_no_may_move_time() {
        let spec = cfo();
        let built = build(&spec, &spec.plan());
        let asset = follow(&built);
        let may = local_ms(day("2026-05-20"), 12, 0);
        built
            .state
            .store
            .lock_loupist(LOUPIST)
            .unwrap()
            .execute(
                "INSERT INTO knowledge_asset_moves (move_id, loupist_id, asset_id, to_verdict,
                    fact_date, reason, evidence_json, actor, at_ms)
                 VALUES ('mov_may', ?1, ?2, 'watch', '2026-05-20', 'May move', '[]', 'user', ?3)",
                rusqlite::params![LOUPIST, asset, may],
            )
            .unwrap();
        let answer = run_tool(
            &built,
            Some(day(APRIL)),
            ToolMethod::AssetPageRead,
            json!({"assetId": asset}),
        );
        let shown = answer.result.to_string();
        assert!(!shown.contains(&may.to_string()), "{shown}");
        assert!(!shown.contains("sinceMoveAtMs"), "{shown}");
        assert!(!shown.contains("mayfiling"), "{shown}");
    }

    /// A March notice of a June departure, and a Lens section citing it.
    fn june_notice() -> Spec {
        let mut cfo = claim("leaving", [2, 0], &[0]);
        cfo.stated_end = Some((0, date("2026-06-15")));
        let mut spec = Spec::manual(
            vec![publisher("marnotice", "2026-03-01", "2026-03-02")],
            vec![cfo],
        );
        spec.lens = Some(LensSpec {
            at_ms: local_ms(day("2026-03-10"), 12, 0),
            sections: vec![("evidence-for".into(), vec![0])],
            quote: None,
            by_turn: false,
            spans: Vec::new(),
        });
        spec
    }

    #[test]
    fn r1_16_a_june_end_learned_in_march_supersedes_its_section_when_june_arrives() {
        let spec = june_notice();
        let built = build(&spec, &spec.plan());
        let live = run_tool(
            &built,
            None,
            ToolMethod::WorldChanges,
            json!({"since": "2025-12-01"}),
        );
        assert!(
            live.result["lens"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|row| row["state"] == "superseded")),
            "June has passed: {}",
            live.result
        );
        assert!(
            rows(&live, "ended")
                .iter()
                .any(|row| row["claimId"] == "leaving"),
            "{}",
            live.result
        );
        let april = run_tool(
            &built,
            Some(day(APRIL)),
            ToolMethod::WorldChanges,
            json!({"since": "2025-12-01"}),
        );
        assert!(rows(&april, "ended").is_empty(), "{}", april.result);
        assert!(
            april.result["lens"].as_array().is_none_or(Vec::is_empty),
            "{}",
            april.result
        );
    }

    #[test]
    fn r1_17_repairing_a_superseded_section_keeps_the_alert_in_the_replay_of_that_day() {
        let mut ended = claim("tenure", [2, 0], &[0, 1]);
        ended.stated_end = Some((1, date("2026-02-20")));
        let mut spec = Spec::manual(
            vec![
                publisher("febfiling", "2026-02-01", "2026-02-02"),
                publisher("marfiling", "2026-03-01", "2026-03-02"),
            ],
            vec![ended],
        );
        spec.lens = Some(LensSpec {
            at_ms: local_ms(day("2026-02-05"), 12, 0),
            sections: vec![("evidence-for".into(), vec![0])],
            quote: None,
            by_turn: false,
            spans: Vec::new(),
        });
        let built = build(&spec, &spec.plan());
        let lens = built.lens.clone().expect("the section was filed");
        let superseded = |cutoff: Option<NaiveDate>| {
            run_tool(
                &built,
                cutoff,
                ToolMethod::WorldChanges,
                json!({"since": "2026-03-01"}),
            )
            .result["lens"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|row| row["state"] == "superseded"))
        };
        assert!(
            superseded(Some(day("2026-03-02"))),
            "the March 2 replay sees the alert"
        );
        knowledge(&built.state)
            .transact(|write| {
                write.restore_section(
                    LOUPIST,
                    &lens,
                    "evidence-for",
                    local_ms(day("2026-03-03"), 9, 0),
                )
            })
            .unwrap();
        assert!(
            superseded(Some(day("2026-03-02"))),
            "a repair the next day keeps it"
        );
        assert!(superseded(Some(day("2026-03-05"))));
    }

    /// One claim conflicts with two claims of one later Source.
    fn two_conflicts() -> Spec {
        let mut spec = Spec::manual(
            vec![
                publisher("janone", "2026-01-10", "2026-01-11"),
                publisher("febtwo", "2026-02-10", "2026-02-11"),
            ],
            vec![
                claim("first", [2, 0], &[0]),
                claim("second", [3, 0], &[1]),
                claim("third", [2, 1], &[1]),
            ],
        );
        for right in [1, 2] {
            spec.links.push(LinkSpec {
                left: 0,
                right,
                selected_by: 1,
                same_by: None,
                also_same_by: Vec::new(),
            });
        }
        spec
    }

    #[test]
    fn r1_18_two_conflicts_of_one_claim_in_one_episode_both_cross_a_page_boundary() {
        let spec = two_conflicts();
        let built = build(&spec, &spec.plan());
        for axis in ["recorded", "published"] {
            let whole = run_tool(
                &built,
                None,
                ToolMethod::WorldChanges,
                json!({"since": "2025-12-01", "axis": axis, "kinds": ["conflict"], "limit": 50}),
            );
            let mut expected: Vec<String> = rows(&whole, "conflict")
                .iter()
                .map(|row| format!("{}-{}", row["claimId"], row["withClaimId"]))
                .collect();
            assert!(expected.len() >= 2, "{}", whole.result);
            let mut paged = Vec::new();
            let mut since = json!("2025-12-01");
            for _ in 0..20 {
                let page = run_tool(
                    &built,
                    None,
                    ToolMethod::WorldChanges,
                    json!({"since": since, "axis": axis, "kinds": ["conflict"], "limit": 1}),
                );
                paged.extend(
                    rows(&page, "conflict")
                        .iter()
                        .map(|row| format!("{}-{}", row["claimId"], row["withClaimId"])),
                );
                if page.result["capped"] != true {
                    break;
                }
                since = page.result["next"].clone();
            }
            expected.sort();
            paged.sort();
            assert_eq!(paged, expected, "{axis}");
        }
    }

    /// `old` ends when `new` arrives in March; a January copy of `new` read in
    /// March ends `old`; an April restatement of it, read in June, leaves
    /// the days interleaved with no witness after March (master plan 9.10),
    /// so the March ending is withdrawn and the pair is a conflict.
    fn reopened() -> Spec {
        let mut spec = Spec::manual(
            vec![
                publisher("febold", "2026-02-10", "2026-02-11"),
                publisher("marnew", "2026-03-10", "2026-03-11"),
                publisher("aprcopy", "2026-04-08", "2026-06-10"),
            ],
            vec![claim("old", [2, 0], &[0, 2]), claim("new", [3, 0], &[1])],
        );
        spec.links.push(LinkSpec {
            left: 0,
            right: 1,
            selected_by: 1,
            same_by: Some(1),
            also_same_by: Vec::new(),
        });
        spec
    }

    fn lineage(built: &Built, cutoff: Option<NaiveDate>, claim: &str) -> Value {
        run_tool(
            built,
            cutoff,
            ToolMethod::WorldRead,
            json!({"scope": "world", "recordId": built.world, "centerClaimId": claim, "history": true}),
        )
        .result["lineage"]
            .clone()
    }

    #[test]
    fn r1_20_after_a_backfill_reopens_a_claim_its_lineage_names_no_current_ender() {
        let spec = reopened();
        let built = build(&spec, &spec.plan());
        let old = lineage(&built, None, "old");
        assert!(old["ended"].is_null(), "{old}");
        assert!(old["endedByClaimId"].is_null(), "{old}");
        assert!(
            old["endings"]
                .as_array()
                .is_some_and(|rows| !rows.is_empty()),
            "the March ending stays as history: {old}"
        );
    }

    #[test]
    fn r2_20_the_former_ender_no_longer_lists_the_reopened_claim() {
        let spec = reopened();
        let built = build(&spec, &spec.plan());
        let new = lineage(&built, None, "new");
        assert!(
            !new["endedClaims"]
                .as_array()
                .is_some_and(|ids| ids.iter().any(|id| id == "old")),
            "{new}"
        );
    }

    #[test]
    fn r3_20_a_replay_after_the_reopen_keeps_the_prior_ending_in_history() {
        let spec = reopened();
        let built = build(&spec, &spec.plan());
        let old = lineage(&built, Some(day("2026-04-30")), "old");
        assert!(old["ended"].is_null(), "{old}");
        assert!(
            old["endings"]
                .as_array()
                .is_some_and(|rows| !rows.is_empty()),
            "the April replay keeps the March ending in history: {old}"
        );
    }

    /// T-RESTATE (master plan 9.10): X in January, the incompatible Y in
    /// February, X again in March. Each Source that read the other side marked
    /// the pair the same assertion; `witnessed` says whether the March one did.
    /// A Lens section filed in January cites X.
    fn restated(witnessed: bool) -> Spec {
        let mut spec = Spec::manual(
            vec![
                publisher("janx", "2026-01-12", "2026-01-13"),
                publisher("feby", "2026-02-09", "2026-02-10"),
                publisher("marx", "2026-03-16", "2026-03-17"),
            ],
            vec![claim("x", [2, 0], &[0, 2]), claim("y", [3, 0], &[1])],
        );
        spec.links.push(LinkSpec {
            left: 0,
            right: 1,
            selected_by: 1,
            same_by: Some(1),
            also_same_by: if witnessed { vec![2] } else { Vec::new() },
        });
        spec.lens = Some(LensSpec {
            at_ms: local_ms(day("2026-01-20"), 12, 0),
            sections: vec![("evidence-for".into(), vec![0])],
            quote: None,
            by_turn: false,
            spans: Vec::new(),
        });
        spec
    }

    fn qualifier<'a>(edge: &'a Value, key: &str) -> Option<&'a str> {
        edge["claim"]["qualifiers"]
            .as_array()?
            .iter()
            .find(|item| item["key"] == key)?["value"]
            .as_str()
    }

    #[test]
    fn t_restate_the_april_and_february_views_equal_their_prefix_worlds() {
        for witnessed in [true, false] {
            let spec = restated(witnessed);
            let full = build(&spec, &spec.plan());
            for cutoff in ["2026-02-20", APRIL] {
                let prefix = build(&spec, &spec.prefix_plan(day(cutoff)));
                if let Err(found) = compare(&spec, day(cutoff), &full, &prefix) {
                    panic!("witnessed {witnessed} cutoff {cutoff}: {found}");
                }
            }
            let at = |cutoff: Option<&str>| {
                view(&run_tool(
                    &full,
                    cutoff.map(day),
                    ToolMethod::WorldRead,
                    json!({"scope": "world", "recordId": full.world}),
                ))
            };
            // February: Y is the newer assertion and X ended when it came.
            let february = at(Some("2026-02-20"));
            let x = edge(&february, "x").expect("X is known in February");
            assert_eq!(x["claim"]["invalidAt"]["value"], "2026-02-09", "{x}");
            assert!(qualifier(x, REOPENED_AT).is_none(), "{x}");
            assert!(edge(&february, "y").unwrap()["claim"]["invalidAt"].is_null());
            for (label, shown) in [("April", at(Some(APRIL))), ("today", at(None))] {
                let x = edge(&shown, "x").unwrap();
                let y = edge(&shown, "y").unwrap();
                assert!(
                    x["claim"]["invalidAt"].is_null(),
                    "{label}: X is current: {x}"
                );
                if witnessed {
                    assert_eq!(
                        y["claim"]["invalidAt"]["value"], "2026-03-16",
                        "{label}: {y}"
                    );
                    assert_eq!(
                        qualifier(x, REOPENED_AT),
                        Some("2026-03-16"),
                        "{label}: {x}"
                    );
                    assert_eq!(
                        qualifier(x, LAPSED),
                        Some("2026-02-09/2026-03-16"),
                        "{label}"
                    );
                    assert!(qualifier(x, CONFLICTS_WITH_CLAIM).is_none(), "{label}: {x}");
                } else {
                    assert!(y["claim"]["invalidAt"].is_null(), "{label}: {y}");
                    assert_eq!(
                        qualifier(x, CONFLICTS_WITH_CLAIM),
                        Some("y"),
                        "{label}: {x}"
                    );
                    assert!(qualifier(x, REOPENED_AT).is_none(), "{label}: {x}");
                }
            }
        }
    }

    #[test]
    fn t_restate_every_ingestion_order_gives_one_world() {
        for witnessed in [true, false] {
            let spec = restated(witnessed);
            let reference = claim_states(&build(&spec, &spec.plan()), None, &BTreeSet::new());
            for order in [[2usize, 1, 0], [1, 0, 2], [0, 2, 1]] {
                let plan = Plan {
                    ingest: order
                        .iter()
                        .enumerate()
                        .map(|(step, index)| {
                            (
                                *index,
                                local_ms(day("2026-04-02"), 9, 0) + 3_600_000 * step as i64,
                            )
                        })
                        .collect(),
                    lens_at: None,
                    reset: None,
                };
                let other = claim_states(&build(&spec, &plan), None, &BTreeSet::new());
                if let Some(difference) = first_difference(&reference, &other, "") {
                    panic!("witnessed {witnessed} order {order:?}: {difference}");
                }
            }
        }
    }

    /// World's As of control on a restated claim: between Y's contradiction
    /// (February 9) and X's restatement (March 16) X is ended; after it, X is
    /// current again with its gap. The World query shows the claim the chat
    /// as-of read shows, since both read the one cutoff projection.
    #[test]
    fn world_as_of_shows_a_restated_claim_ended_before_the_restatement_and_current_with_its_gap_after(
    ) {
        let spec = restated(true);
        let built = build(&spec, &spec.plan());
        let world = |cutoff: &str| {
            let reads = renderer_reads(&built, Some(day(cutoff)));
            let x = edge(&reads[0].1.result, "x")
                .unwrap_or_else(|| panic!("X in the {cutoff} World: {}", reads[0].1.result))
                .clone();
            let chat = view(&run_tool(
                &built,
                Some(day(cutoff)),
                ToolMethod::WorldRead,
                json!({"scope": "world", "recordId": built.world}),
            ));
            let chat_x = edge(&chat, "x").expect("X in the chat read").clone();
            for key in ["invalidAt", "qualifiers", "status"] {
                assert_eq!(x["claim"][key], chat_x["claim"][key], "{cutoff} {key}");
            }
            x
        };
        let between = world("2026-03-01");
        assert_eq!(between["ended"], true, "{between}");
        assert_eq!(between["claim"]["invalidAt"]["value"], "2026-02-09");
        assert!(qualifier(&between, REOPENED_AT).is_none(), "{between}");
        assert!(qualifier(&between, LAPSED).is_none(), "{between}");
        let after = world(APRIL);
        assert!(after["ended"].is_null(), "{after}");
        assert!(after["claim"]["invalidAt"].is_null(), "{after}");
        assert_eq!(qualifier(&after, REOPENED_AT), Some("2026-03-16"));
        assert_eq!(
            qualifier(&after, LAPSED),
            Some("2026-02-09/2026-03-16"),
            "{after}"
        );
    }

    #[test]
    fn t_restate_a_section_superseded_by_the_withdrawn_ending_reads_current_again() {
        let spec = restated(true);
        let built = build(&spec, &spec.plan());
        let state = |cutoff: Option<&str>| {
            let answer = run_tool(
                &built,
                cutoff.map(day),
                ToolMethod::LensRead,
                json!({"recordId": built.lens.clone().unwrap()}),
            );
            let text: Value =
                serde_json::from_str(answer.result["text"].as_str().unwrap_or("null")).unwrap();
            text["body"]["sections"][0].clone()
        };
        let january = state(Some("2026-01-31"));
        assert!(
            january["claimIds"].is_null(),
            "X is current in January: {january}"
        );
        let march_first = state(Some("2026-03-01"));
        assert_eq!(
            march_first["claimIds"],
            json!(["x"]),
            "X had ended: {march_first}"
        );
        let now = state(None);
        assert!(now["claimIds"].is_null(), "X is current again: {now}");
        assert!(now["sinceMs"].is_null(), "{now}");
    }

    #[test]
    fn r1_22_a_filing_comparison_stages_a_receipt_with_the_claims_it_returns() {
        let spec = two_reports_then_may();
        let built = build(&spec, &spec.plan());
        let answer = run_tool(
            &built,
            None,
            ToolMethod::WorldChanges,
            json!({"between": {"to": built.sources[&1].record_id, "from": built.sources[&0].record_id}}),
        );
        assert!(!answer.receipts.is_empty(), "{}", answer.result);
        assert!(
            answer
                .receipts
                .iter()
                .any(|receipt| edge(receipt, "cfo").is_some()),
            "{:?}",
            answer.receipts
        );
    }

    #[test]
    fn r1_26_an_asset_page_counts_every_change_and_reports_lens_and_pending() {
        let mut claims: Vec<ClaimSpec> = (0..10)
            .map(|index| claim(&format!("fact{index}"), [2, 0], &[0]))
            .collect();
        claims[0].asserted_by.push(1);
        claims[0].stated_end = Some((1, date("2026-01-31")));
        let mut spec = Spec::manual(
            vec![
                publisher("janmany", "2026-01-10", "2026-01-11"),
                publisher("febends", "2026-02-10", "2026-02-11"),
            ],
            claims,
        );
        spec.lens = Some(LensSpec {
            at_ms: local_ms(day("2026-01-20"), 12, 0),
            sections: vec![("evidence-for".into(), vec![0])],
            quote: None,
            by_turn: false,
            spans: Vec::new(),
        });
        let built = build(&spec, &spec.plan());
        let asset = follow(&built);
        built
            .state
            .store
            .lock_loupist(LOUPIST)
            .unwrap()
            .execute(
                "INSERT INTO knowledge_graph_outbox (
                    job_id, loupist_id, world_record_id, world_revision_id, status,
                    payload_json, payload_digest, created_at_ms
                 ) VALUES ('job-oracle-wait', ?1, ?2, 'rev', 'queued', ?3, 'd', 1)",
                rusqlite::params![
                    LOUPIST,
                    built.world,
                    json!({"sourceBinding": {"sourceRecordId": built.sources[&0].record_id}})
                        .to_string()
                ],
            )
            .unwrap();
        let page = run_tool(
            &built,
            None,
            ToolMethod::AssetPageRead,
            json!({"assetId": asset}),
        );
        let since = &page.result["sinceRead"];
        assert_eq!(since["counts"]["added"], 10, "{}", page.result);
        assert!(
            since["changes"]
                .as_array()
                .is_some_and(|rows| rows.len() <= 8),
            "{since}"
        );
        assert_eq!(since["pending"], 1, "{since}");
        assert!(
            since["lens"]
                .as_array()
                .is_some_and(|rows| !rows.is_empty()),
            "the superseded section is reported: {since}"
        );
    }

    #[test]
    fn r2_03_april_history_returns_the_corrected_capture_date() {
        let mut spec = cfo();
        // The capture now says January; the episode was written when it said May.
        spec.sources[0].episode_publication = Some(date("2026-05-20"));
        let built = build(&spec, &spec.plan());
        let lineage = lineage(&built, Some(day(APRIL)), "cfo");
        let created = &lineage["created"];
        assert!(created.is_object(), "{lineage}");
        assert!(!lineage.to_string().contains("2026-05-20"), "{lineage}");
        assert!(
            created["publicationTime"]
                .to_string()
                .contains("2026-01-15"),
            "{created}"
        );
    }

    #[test]
    fn r2_08_two_visible_claims_show_no_conflict_the_projection_does_not_hold() {
        let mut spec = Spec::manual(
            vec![
                publisher("janvisible", "2026-01-10", "2026-01-11"),
                publisher("febvisible", "2026-02-10", "2026-02-11"),
                publisher("mayselects", "2026-05-10", "2026-05-11"),
            ],
            vec![claim("left", [2, 0], &[0]), claim("right", [3, 0], &[1, 2])],
        );
        spec.links.push(LinkSpec {
            left: 0,
            right: 1,
            selected_by: 2,
            same_by: None,
            also_same_by: Vec::new(),
        });
        let built = build(&spec, &spec.plan());
        for axis in ["published", "recorded"] {
            let answer = run_tool(
                &built,
                Some(day("2026-04-30")),
                ToolMethod::WorldChanges,
                json!({"since": "2025-12-01", "axis": axis}),
            );
            assert!(
                rows(&answer, "conflict").is_empty(),
                "{axis}: {}",
                answer.result
            );
        }
    }

    /// World loop T-SAMEDAY (F3): a witness published the same day as the
    /// span a section already cites is not answered by that span: the
    /// section citing the ended chair claim and its own span is superseded,
    /// live and under a later cutoff.
    #[test]
    fn loop_t_sameday_a_same_day_span_does_not_answer_the_ending() {
        let mut chair = claim("chair", [2, 0], &[0, 1]);
        chair.stated_end = Some((1, date("2026-08-19")));
        let mut spec = Spec::manual(
            vec![
                publisher("chairnamed", "2026-08-19", "2026-08-19"),
                publisher("chairleft", "2026-08-19", "2026-08-25"),
            ],
            vec![chair],
        );
        spec.lens = Some(LensSpec {
            at_ms: local_ms(day("2026-08-21"), 12, 0),
            sections: vec![("evidence-for".into(), vec![0])],
            quote: None,
            by_turn: false,
            spans: vec![("evidence-for".into(), 0)],
        });
        let built = build(&spec, &spec.plan());
        for cutoff in [None, Some(day("2026-08-30"))] {
            let page = run_tool(
                &built,
                cutoff,
                ToolMethod::LensRead,
                json!({"recordId": built.lens.clone().unwrap()}),
            );
            let text: Value = serde_json::from_str(page.result["text"].as_str().unwrap()).unwrap();
            let section = &text["body"]["sections"][0];
            assert_eq!(
                section["claimIds"],
                json!(["chair"]),
                "{cutoff:?}: {section}"
            );
        }
    }

    /// Lane R survivor c13ca29: a second claim that falls in a contested
    /// section alerts once, for itself, in the dated rows a cutoff reads (and a
    /// check's replay): one row per claim after both falls, the first only
    /// between them, where the page names only the first as contested. The
    /// live feed shows each section once, in its current state.
    #[test]
    fn lane_r_c13ca29_a_second_contested_claim_alerts_once_for_itself() {
        let mut spec = Spec::manual(
            vec![
                publisher("janboth", "2026-01-10", "2026-01-11"),
                publisher("febfirst", "2026-02-10", "2026-02-11"),
                publisher("marsecond", "2026-03-10", "2026-03-11"),
            ],
            vec![claim("first", [2, 0], &[0]), claim("second", [3, 0], &[0])],
        );
        spec.lens = Some(LensSpec {
            at_ms: local_ms(day("2026-01-20"), 12, 0),
            sections: vec![("evidence-for".into(), vec![0, 1])],
            quote: None,
            by_turn: false,
            spans: Vec::new(),
        });
        for (claim, witness) in [(0, 1), (1, 2)] {
            spec.statuses.push(StatusSpec {
                claim,
                status: ClaimStatus::Disputed,
                witness,
            });
        }
        let built = build(&spec, &spec.plan());
        let contested = |cutoff: Option<NaiveDate>| -> Vec<Vec<String>> {
            let feed = run_tool(
                &built,
                cutoff,
                ToolMethod::WorldChanges,
                json!({"since": "2025-12-01", "limit": 50}),
            );
            super::lens_alerts(feed.result.get("lens"))
                .into_iter()
                .filter(|(_, state, _)| state == "contested")
                .map(|(_, _, claims)| claims)
                .collect()
        };
        let mut rows = contested(Some(day("2026-04-30")));
        rows.sort();
        assert_eq!(
            rows,
            vec![vec!["first".to_owned()], vec!["second".to_owned()]]
        );
        let between = Some(day("2026-02-28"));
        assert_eq!(contested(between), vec![vec!["first".to_owned()]]);
        let page = run_tool(
            &built,
            between,
            ToolMethod::LensRead,
            json!({"recordId": built.lens.clone().unwrap()}),
        );
        let text: Value = serde_json::from_str(page.result["text"].as_str().unwrap()).unwrap();
        let section = &text["body"]["sections"][0];
        assert_eq!(section["contestedClaimIds"], json!(["first"]), "{section}");
    }

    #[test]
    fn r2_new2_a_withdrawn_row_without_an_event_source_does_not_fail_the_feed() {
        let mut spec = Spec::manual(
            vec![
                publisher("janheld", "2026-01-10", "2026-01-11"),
                publisher("febwithdraws", "2026-02-10", "2026-02-11"),
            ],
            vec![claim("pulled", [2, 0], &[0])],
        );
        spec.statuses.push(StatusSpec {
            claim: 0,
            status: ClaimStatus::Withdrawn,
            witness: 1,
        });
        let built = build(&spec, &spec.plan());
        let answer = run_tool(
            &built,
            None,
            ToolMethod::WorldChanges,
            json!({"since": "2025-12-01"}),
        );
        assert!(answer.result.get("error").is_none(), "{}", answer.result);
        assert!(!rows(&answer, "withdrawn").is_empty(), "{}", answer.result);
    }

    #[test]
    fn r2_new3_an_april_comparison_receipt_has_no_may_ending_or_span() {
        let spec = two_reports_then_may();
        let built = build(&spec, &spec.plan());
        let answer = april_between(&built);
        let may_span = built.sources[&2].span_id.clone().unwrap();
        assert!(!answer.receipts.is_empty(), "{}", answer.result);
        for receipt in &answer.receipts {
            let shown = receipt.to_string();
            assert!(!shown.contains(&may_span), "{shown}");
            assert!(!shown.contains("2026-03-15"), "{shown}");
        }
    }

    #[test]
    fn r3_05_a_later_capture_of_the_target_does_not_label_the_visible_claim() {
        let spec = Spec::manual(
            vec![
                publisher("janmixed", "2026-01-10", "2026-01-11"),
                publisher("febother", "2026-02-10", "2026-02-11"),
            ],
            vec![claim("shared", [2, 0], &[0, 1])],
        );
        let built = build(&spec, &spec.plan());
        // The January record gets a later capture with a new title and URL.
        let mixed = &built.sources[&0];
        knowledge(&built.state)
            .transact(|write| {
                write.retain_source(crate::knowledge::RetainSource {
                    origin: None,
                    loupist_id: LOUPIST.into(),
                    record_id: mixed.record_id.clone(),
                    expected_revision_id: Some(mixed.revision_id.clone()),
                    at_ms: local_ms(day("2026-06-02"), 9, 0),
                    source_id: Some(mixed.source_id.clone()),
                    title: "Later identity hiddenjune".into(),
                    source_class: SourceClass::PublisherDocument,
                    origin_locator: "https://filings.example.test/hiddenjune".into(),
                    provider_identity: "fixture".into(),
                    authorship: Authorship::Publisher,
                    coverage: Coverage::IncludedFull,
                    origin_chat_id: None,
                    request_ref: None,
                    declared_digest: None,
                    material: Some(RetainMaterial {
                        media_type: "text/plain".into(),
                        role: RepresentationRole::Original,
                        document_id: "hiddenjune".into(),
                        parser_version: None,
                        parent_representation_id: None,
                        input_span_ids: Vec::new(),
                        selector: Selector::ByteRange { start: 0, end: 10 },
                        context_note: "hiddenjune".into(),
                        bytes: b"hiddenjune".to_vec(),
                    }),
                    publication_time: Some(date("2026-06-01")),
                })
            })
            .unwrap();
        let answer = run_tool(
            &built,
            Some(day(APRIL)),
            ToolMethod::WorldChanges,
            json!({"between": {"to": built.sources[&1].record_id, "from": mixed.record_id}}),
        );
        assert!(!text(&answer).contains("hiddenjune"), "{}", answer.result);
        let feed = run_tool(
            &built,
            Some(day(APRIL)),
            ToolMethod::WorldChanges,
            json!({"since": "2025-12-01"}),
        );
        assert!(!text(&feed).contains("hiddenjune"), "{}", feed.result);
    }

    #[test]
    fn r3_06_an_ending_the_projection_rejects_leaves_the_claim_restated() {
        let spec = two_reports_then_may();
        let built = build(&spec, &spec.plan());
        let answer = april_between(&built);
        let restated = answer.result["restated"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(
            restated.iter().any(|row| row["claimId"] == "cfo"),
            "both reports support the CFO in April: {}",
            answer.result
        );
    }

    /// A March dispute and a May withdrawal of one January claim.
    fn dispute_then_withdrawal() -> Spec {
        let mut spec = Spec::manual(
            vec![
                publisher("janfact", "2026-01-10", "2026-01-11"),
                publisher("mardispute", "2026-03-10", "2026-03-11"),
                publisher("maywithdrawal", "2026-05-10", "2026-05-11"),
            ],
            vec![claim("contested", [2, 0], &[0])],
        );
        spec.statuses.push(StatusSpec {
            claim: 0,
            status: ClaimStatus::Disputed,
            witness: 1,
        });
        spec.statuses.push(StatusSpec {
            claim: 0,
            status: ClaimStatus::Withdrawn,
            witness: 2,
        });
        spec
    }

    #[test]
    fn r3_09_a_march_dispute_under_a_may_withdrawal_reads_disputed_in_april() {
        let spec = dispute_then_withdrawal();
        let built = build(&spec, &spec.plan());
        let read = run_tool(
            &built,
            Some(day("2026-04-30")),
            ToolMethod::WorldRead,
            json!({"scope": "world", "recordId": built.world}),
        );
        let shown = view(&read);
        let contested = edge(&shown, "contested").expect("known in April");
        assert_eq!(contested["claim"]["status"], "disputed", "{contested}");
        let feed = run_tool(
            &built,
            Some(day("2026-04-30")),
            ToolMethod::WorldChanges,
            json!({"since": "2025-12-01", "axis": "published"}),
        );
        assert!(!rows(&feed, "disputed").is_empty(), "{}", feed.result);
        assert!(rows(&feed, "withdrawn").is_empty(), "{}", feed.result);
    }

    #[test]
    fn r3_new_an_as_of_edge_has_one_status_outside_and_inside_its_claim() {
        let spec = dispute_then_withdrawal();
        let built = build(&spec, &spec.plan());
        for (cutoff, status) in [("2026-02-28", "active"), ("2026-04-30", "disputed")] {
            let read = run_tool(
                &built,
                Some(day(cutoff)),
                ToolMethod::WorldRead,
                json!({"scope": "world", "recordId": built.world}),
            );
            let shown = view(&read);
            let contested = edge(&shown, "contested").expect("known");
            assert_eq!(contested["status"], status, "{cutoff}: {contested}");
            assert_eq!(
                contested["claim"]["status"], status,
                "{cutoff}: {contested}"
            );
            for receipt in &read.receipts {
                let contested = edge(receipt, "contested").expect("in the receipt");
                assert_eq!(contested["status"], status, "{cutoff} receipt: {contested}");
                assert_eq!(
                    contested["claim"]["status"], status,
                    "{cutoff} receipt: {contested}"
                );
            }
        }
    }

    #[test]
    fn r3_17_an_asset_replay_keeps_every_section_of_one_alert() {
        let mut ended = claim("tenure", [2, 0], &[0, 1]);
        ended.stated_end = Some((1, date("2026-02-20")));
        let mut spec = Spec::manual(
            vec![
                publisher("febmany", "2026-02-01", "2026-02-02"),
                publisher("marmany", "2026-03-01", "2026-03-02"),
            ],
            vec![ended],
        );
        // Twelve long keys: their names alone exceed the 240-character summary.
        let keys: Vec<String> = (0..12)
            .map(|index| format!("evidence-for-the-figure-reported-in-period-{index:02}"))
            .collect();
        spec.lens = Some(LensSpec {
            at_ms: local_ms(day("2026-02-05"), 12, 0),
            sections: keys.iter().map(|key| (key.clone(), vec![0])).collect(),
            quote: None,
            by_turn: false,
            spans: Vec::new(),
        });
        let built = build(&spec, &spec.plan());
        let asset = follow(&built);
        let replay = run_tool(
            &built,
            Some(day("2026-03-02")),
            ToolMethod::WorldChanges,
            json!({"since": "2026-03-01", "about": [asset]}),
        );
        let shown: BTreeSet<String> = replay.result["lens"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row["sectionKey"].as_str().map(str::to_owned))
            .collect();
        for key in &keys {
            assert!(shown.contains(key), "{key} is missing: {}", replay.result);
        }
    }

    /// Instants written near midnight in several zones, against the user's
    /// calendar: +13:45 (written a day ahead of most users), -10:00 (written a
    /// day behind) and both sides of midnight in UTC.
    #[test]
    fn timezones_an_instant_counts_on_the_users_date_whatever_offset_it_was_written_with() {
        for written in [
            "2026-03-10T23:30:00+13:45",
            "2026-03-11T00:15:00+13:45",
            "2026-03-10T23:30:00-10:00",
            "2026-03-10T00:30:00-10:00",
            "2026-03-10T23:59:30+00:00",
            "2026-03-11T00:00:30+00:00",
        ] {
            let at = DateTime::parse_from_rfc3339(written).unwrap();
            let users = crate::local_time::date_at_ms(at.timestamp_millis()).unwrap();
            let time = instant_at(at);
            for (cutoff, known) in [(users, true), (users.pred_opt().unwrap(), false)] {
                assert_eq!(
                    crate::knowledge::published_by(Some(&time), &cutoff.to_string()),
                    known,
                    "{written} on the user's {users}, cutoff {cutoff}"
                );
            }
            // End to end: the World read and the feed agree with the calendar.
            let mut spec = Spec::manual(
                vec![publisher("tzfiling", "2026-01-01", "2026-03-12")],
                vec![claim("tzclaim", [2, 0], &[0])],
            );
            spec.sources[0].publication = Some(time.clone());
            let built = build(&spec, &spec.plan());
            for (cutoff, known) in [(users, true), (users.pred_opt().unwrap(), false)] {
                let read = run_tool(
                    &built,
                    Some(cutoff),
                    ToolMethod::WorldRead,
                    json!({"scope": "world", "recordId": built.world}),
                );
                assert_eq!(
                    edge(&view(&read), "tzclaim").is_some(),
                    known,
                    "{written}, cutoff {cutoff}: {}",
                    read.result
                );
                let feed = run_tool(
                    &built,
                    Some(cutoff),
                    ToolMethod::WorldChanges,
                    json!({"since": "2026-01-01", "axis": "published"}),
                );
                assert_eq!(
                    !rows(&feed, "added").is_empty(),
                    known,
                    "{written}, cutoff {cutoff}"
                );
                assert_eq!(
                    text(&feed).contains("tzfiling"),
                    known,
                    "{written}, cutoff {cutoff}"
                );
            }
        }
    }

    /// Agent loop R018-08: a followed asset has no page row until something
    /// opens it, and the check passes followed asset ids as `about`.
    #[test]
    fn agent_r018_08_a_followed_asset_without_a_page_scopes_the_feed() {
        let spec = cfo();
        let built = build(&spec, &spec.plan());
        let asset = built
            .state
            .store
            .asset_follow_name(
                LOUPIST,
                &fixed_nodes()[0].name,
                None,
                crate::commands::assets::wire::AssetReferenceOrigin::User,
                1,
            )
            .unwrap();
        let answer = run_tool(
            &built,
            None,
            ToolMethod::WorldChanges,
            json!({"since": "2025-12-01", "about": [asset.asset_id]}),
        );
        assert!(answer.result.get("error").is_none(), "{}", answer.result);
        let unknown = format!("ast_{}", "0".repeat(16));
        let refused = run_tool(
            &built,
            None,
            ToolMethod::WorldChanges,
            json!({"since": "2025-12-01", "about": [unknown]}),
        );
        let message = refused.result["error"]["message"]
            .as_str()
            .unwrap_or_default();
        assert!(
            message.contains("change_about_invalid"),
            "{}",
            refused.result
        );
        assert!(
            !message.contains("record was not found"),
            "{}",
            refused.result
        );
    }

    /// World loop F2: two claims of one Source end the old claim at the same
    /// stated time, a successor and an end fact. "Replaced by" names the
    /// successor, whatever their ids.
    #[test]
    fn loop_f2_two_enders_at_one_time_name_the_open_successor() {
        use crate::knowledge::world_episodes::EndBasis;
        let witness = AssertionWitness {
            span: "spn_june".into(),
            publication: Some(date("2026-06-20")),
        };
        let open = AssertionSources {
            source_ids: BTreeSet::from(["rec_june".to_owned()]),
            witness: Some(witness.clone()),
            stated: None,
            ..Default::default()
        };
        let closed = AssertionSources {
            stated: Some(StatedEnd {
                time: date("2026-06-15"),
                witness: witness.clone(),
            }),
            ..open.clone()
        };
        for (successor, end_fact) in [
            ("aa-successor", "zz-end-fact"),
            ("zz-successor", "aa-end-fact"),
        ] {
            let by_successor =
                Ending::paired(date("2026-06-15"), EndBasis::Stated, successor, &open).unwrap();
            let by_end_fact =
                Ending::paired(date("2026-06-15"), EndBasis::Stated, end_fact, &closed).unwrap();
            assert!(
                ending_order(&by_successor, &by_end_fact).is_lt(),
                "{successor} {end_fact}"
            );
            assert!(
                ending_order(&by_end_fact, &by_successor).is_gt(),
                "{successor} {end_fact}"
            );
        }
    }

    /// World loop F3: a section cites the ended claim and a claim from a Source
    /// published after the end took effect but before the filing that reported
    /// it. That Source never saw the ending, so the section is Superseded.
    #[test]
    fn loop_f3_a_source_published_before_the_witness_does_not_answer_the_ending() {
        let mut departed = claim("departed", [2, 0], &[0, 1]);
        departed.stated_end = Some((1, date("2026-07-06")));
        let mut spec = Spec::manual(
            vec![
                publisher("janappointment", "2026-01-10", "2026-01-11"),
                publisher("sepfiling", "2026-09-01", "2026-09-02"),
                publisher("augupdate", "2026-08-19", "2026-08-20"),
            ],
            vec![departed, claim("unrelated", [3, 1], &[2])],
        );
        spec.lens = Some(LensSpec {
            at_ms: local_ms(day("2026-08-25"), 12, 0),
            sections: vec![("evidence-for".into(), vec![0, 1])],
            quote: None,
            by_turn: false,
            spans: Vec::new(),
        });
        let built = build(&spec, &spec.plan());
        let feed = run_tool(
            &built,
            None,
            ToolMethod::WorldChanges,
            json!({"since": "2026-09-01"}),
        );
        assert!(
            feed.result["lens"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|row| row["state"] == "superseded")),
            "{}",
            feed.result
        );
    }

    /// Allowance lane (e): a Source whose first chunk failed on malformed
    /// output is one failed Source, whatever its later chunk is doing.
    #[test]
    fn allowance_e_a_source_whose_first_chunk_failed_is_one_failed_source() {
        let spec = cfo();
        for later in [
            "queued",
            "running",
            "paused_budget",
            "completed",
            "cancelled_before_start",
            "failed_before_mutation",
            "reconcile_required",
            "reconciled_without_retry",
        ] {
            let built = build(&spec, &spec.plan());
            let source = &built.sources[&0];
            let payload = |chunk: u32| {
                json!({
                    "sourceBinding": {"sourceRecordId": source.record_id, "spanId": source.span_id},
                    "chunkIndex": chunk,
                    "chunkCount": 2,
                })
                .to_string()
            };
            built
                .state
                .store
                .lock_loupist(LOUPIST)
                .unwrap()
                .execute(
                    "INSERT INTO knowledge_graph_outbox (
                        job_id, loupist_id, world_record_id, world_revision_id, status,
                        payload_json, payload_digest, created_at_ms
                     ) VALUES
                     ('job-first', ?1, ?2, 'rev', 'failed_extraction', ?3, 'd1', 1),
                     ('job-later', ?1, ?2, 'rev', ?4, ?5, 'd2', 2)",
                    rusqlite::params![LOUPIST, built.world, payload(0), later, payload(1)],
                )
                .unwrap();
            let feed = run_tool(
                &built,
                None,
                ToolMethod::WorldChanges,
                json!({"since": "2025-12-01"}),
            );
            assert_eq!(
                feed.result["failed"], 1,
                "later chunk {later}: {}",
                feed.result
            );
            assert_eq!(
                feed.result["pending"], 0,
                "later chunk {later}: {}",
                feed.result
            );
        }
    }

    #[test]
    fn r3_new_a_second_contested_claim_does_not_alert_the_first_asset_again() {
        // A cited claim that falls contests its section (a dispute here): the
        // first company's claim in February, the other company's in March.
        let mut spec = Spec::manual(
            vec![
                publisher("janbase", "2026-01-10", "2026-01-11"),
                publisher("febcontest", "2026-02-10", "2026-02-11"),
                publisher("marcontest", "2026-03-10", "2026-03-11"),
            ],
            vec![claim("first", [2, 0], &[0]), claim("other", [3, 1], &[0])],
        );
        for (claim, witness) in [(0, 1), (1, 2)] {
            spec.statuses.push(StatusSpec {
                claim,
                status: ClaimStatus::Disputed,
                witness,
            });
        }
        spec.lens = Some(LensSpec {
            at_ms: local_ms(day("2026-01-20"), 12, 0),
            sections: vec![("evidence-for".into(), vec![0, 1])],
            quote: None,
            by_turn: false,
            spans: Vec::new(),
        });
        let built = build(&spec, &spec.plan());
        let asset = follow(&built);
        let february = run_tool(
            &built,
            Some(day("2026-03-31")),
            ToolMethod::WorldChanges,
            json!({"since": "2026-02-01", "about": [asset]}),
        );
        assert!(
            february.result["lens"]
                .as_array()
                .is_some_and(|rows| rows.len() == 1),
            "the first company's section alert in February: {}",
            february.result
        );
        let march = run_tool(
            &built,
            Some(day("2026-03-31")),
            ToolMethod::WorldChanges,
            json!({"since": "2026-03-01", "about": [asset]}),
        );
        assert!(
            march.result["lens"].as_array().is_none_or(Vec::is_empty),
            "the first company's section was contested in February only: {}",
            march.result
        );
    }
}
