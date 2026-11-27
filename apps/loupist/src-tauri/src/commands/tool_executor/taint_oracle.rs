//! The taint oracle (master plan 3.10, extension plan 6.1).
//!
//! Reference, written from the design: a turn that has shown the model
//! third-party text cannot write a standing instruction. For every route by
//! which a Memory read can return someone else's text, a World whose only
//! third-party text is a canary is read once; the turn must then be marked and
//! `principle_save`, the Bot memory save, `learning_program_save` and
//! `learning_handoff` refused.
//! The same World with only user-authored Sources leaves the turn clean, a
//! user-authored Source is never labeled as a publisher's, and a failed
//! lookup marks the turn. [`TOOLS`] has a row for every catalog tool;
//! [`the_taint_table_names_every_catalog_tool`] fails on a new one.
use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{json, Value};

use super::{AppToolExecutor, ToolCapture};
use crate::broker::{ToolExecutor, ToolMethod, ToolPolicy};
use crate::commands::knowledge::as_of_oracle::{
    self as oracle, build, claim, source, Built, LensSpec, Spec, LOUPIST, SEARCH_WORD,
};
use crate::knowledge::Authorship;

const HOME: &str = "home-loupist-a";
const CANARY: &str = "tpcanary7q";

/// How third-party text reaches a result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::commands) enum Route {
    /// A World claim whose evidence a publisher wrote.
    DirectEvidence,
    /// A publisher's passage read by its id.
    SourcePassage,
    /// A search hit on a publisher's Source or claim.
    SearchHit,
    /// A supporting episode the user wrote, on a publisher's claim.
    SupportingEpisode,
    /// The publisher's Source is the older side of a comparison.
    ComparisonFrom,
    /// The publisher's Source is the newer side of a comparison.
    ComparisonTo,
    /// The change feed inside an asset page.
    EmbeddedFeed,
    /// A Lens section Loupist filed that quotes the publisher. Not in
    /// [`ROUTES`]: see the `lens_read` row of [`TOOLS`].
    QuotedLens,
    /// Only the title and URL carry the canary, as a row's label.
    TitleLabel,
    /// The evidence Source cannot be looked up any more.
    FailedLookup,
}

const ROUTES: &[Route] = &[
    Route::DirectEvidence,
    Route::SourcePassage,
    Route::SearchHit,
    Route::SupportingEpisode,
    Route::ComparisonFrom,
    Route::ComparisonTo,
    Route::EmbeddedFeed,
    Route::TitleLabel,
    Route::FailedLookup,
];

/// What this oracle knows about each catalog tool.
#[allow(dead_code)]
pub(in crate::commands) enum Taint {
    /// Exercised here over these routes.
    Routes(&'static [Route]),
    /// Its result class is third-party text: the call itself marks the turn.
    Class,
    /// Returns only the host's, the user's or numeric data.
    Own(&'static str),
    /// Returns retained text by a route another module tests.
    Elsewhere(&'static str),
    /// A native file tool: the native policy and copy-back digests decide.
    Native,
    /// A standing instruction write: the oracle checks it is refused.
    Standing,
    /// A proposal the user accepts on a card that shows what will run: not
    /// paused when it saves a bundled check template, or a bundled learning
    /// preset whose slots name only its subject; paused with model-written
    /// code, a brief of its own or a free-text slot.
    CardProposal,
}

pub(in crate::commands) const TOOLS: &[(&str, Taint)] = &[
    (
        "world_read",
        Taint::Routes(&[
            Route::DirectEvidence,
            Route::SourcePassage,
            Route::FailedLookup,
        ]),
    ),
    ("world_search", Taint::Routes(&[Route::SearchHit])),
    (
        "world_changes",
        Taint::Routes(&[
            Route::SupportingEpisode,
            Route::ComparisonFrom,
            Route::ComparisonTo,
            Route::TitleLabel,
            Route::FailedLookup,
        ]),
    ),
    ("asset_page_read", Taint::Routes(&[Route::EmbeddedFeed])),
    // The user's own Memory never raises the tier (docs/runtime/untrusted-text.md),
    // and a section Loupist filed is the user's own Memory. Known, accepted gap
    // for 0.1.8: a section filed in a turn that had read third-party text and
    // that quotes it reads back clean; closing it needs a stored origin tier on
    // the section, a design item for the release that adds stored corrections.
    // `Route::QuotedLens` keeps the case runnable.
    (
        "lens_read",
        Taint::Own("the user's own Memory; known gap: quoted text filed after third-party text"),
    ),
    (
        "lens_search",
        Taint::Own("the user's own Memory; known gap: quoted text filed after third-party text"),
    ),
    ("principle_save", Taint::Standing),
    ("bot_memory", Taint::Standing),
    ("learning_program_save", Taint::CardProposal),
    // Forwards the user's words to Bot, which drafts a learning program from them.
    ("learning_handoff", Taint::Standing),
    ("web_search", Taint::Class),
    ("web_fetch", Taint::Class),
    ("dart_filing_read", Taint::Class),
    ("kis_symbol_lookup", Taint::Class),
    ("kis_dividend_calendar", Taint::Class),
    (
        "dataset_import",
        Taint::Elsewhere("tests/untrusted_text.rs: retained datasets"),
    ),
    (
        "dataset_describe",
        Taint::Elsewhere("tests/untrusted_text.rs: retained datasets"),
    ),
    (
        "dataset_query",
        Taint::Elsewhere("tests/untrusted_text.rs: retained datasets"),
    ),
    (
        "dataset_export",
        Taint::Elsewhere("tests/untrusted_text.rs: retained datasets"),
    ),
    (
        "chat_history",
        Taint::Elsewhere("tests/untrusted_text.rs: earlier replies"),
    ),
    (
        "conversation_memory",
        Taint::Elsewhere("tests/untrusted_text.rs: earlier replies"),
    ),
    (
        "workspace_capture",
        Taint::Elsewhere("tests/untrusted_text.rs: app-exported text"),
    ),
    (
        "pasted_capture",
        Taint::Elsewhere("tests/pasted.rs: user-provided material"),
    ),
    (
        "run_python",
        Taint::Elsewhere("python_reads.rs: untrusted_input"),
    ),
    (
        "script_save",
        Taint::Elsewhere("tests/untrusted_text.rs: saved scripts"),
    ),
    (
        "script_list",
        Taint::Elsewhere("tests/untrusted_text.rs: saved scripts"),
    ),
    (
        "script_read",
        Taint::Elsewhere("tests/untrusted_text.rs: saved scripts"),
    ),
    (
        "run_results_list",
        Taint::Elsewhere("tests/untrusted_text.rs: result warnings"),
    ),
    (
        "artifact_list",
        Taint::Elsewhere("tests/untrusted_text.rs: artifact titles"),
    ),
    (
        "artifact_read",
        Taint::Elsewhere("tests/untrusted_text.rs: artifacts"),
    ),
    (
        "chart_query",
        Taint::Elsewhere("tests/untrusted_text.rs: chart rows"),
    ),
    (
        "schedule_list",
        Taint::Elsewhere("returning.rs: trigger readings"),
    ),
    (
        "schedule_read",
        Taint::Elsewhere("returning.rs: trigger readings"),
    ),
    (
        "schedule_create",
        Taint::Elsewhere("returning.rs: trigger readings"),
    ),
    ("trigger_save", Taint::CardProposal),
    (
        "schedule_handoff",
        Taint::Elsewhere("returning.rs: trigger readings"),
    ),
    (
        "schedule_briefing",
        Taint::Elsewhere("returning.rs: trigger readings"),
    ),
    (
        "schedule_update",
        Taint::Elsewhere("returning.rs: trigger readings"),
    ),
    (
        "schedule_set_status",
        Taint::Elsewhere("returning.rs: trigger readings"),
    ),
    (
        "schedule_delete",
        Taint::Elsewhere("returning.rs: trigger readings"),
    ),
    (
        "schedule_run_now",
        Taint::Elsewhere("returning.rs: trigger readings"),
    ),
    (
        "schedule_stop",
        Taint::Elsewhere("returning.rs: trigger readings"),
    ),
    ("read", Taint::Native),
    ("write", Taint::Native),
    ("edit", Taint::Native),
    ("ls", Taint::Native),
    ("find", Taint::Native),
    ("grep", Taint::Native),
    (
        "capability_load",
        Taint::Own("loads a bundled capability's own text"),
    ),
    ("connection_setup", Taint::Own("the host's setup steps")),
    ("source_save", Taint::Own("returns the saved Source's ids")),
    ("note_save", Taint::Own("a write; returns its status")),
    ("journal_save", Taint::Own("a write; returns its status")),
    ("journal_review", Taint::Own("a write; returns its status")),
    ("lens_file", Taint::Own("a write; returns its status")),
    (
        "world_schema_update",
        Taint::Own("a write; returns its status"),
    ),
    ("loupist_setup", Taint::Own("a write; returns its status")),
    ("thesis_update", Taint::Own("a write; returns its status")),
    ("price_read_save", Taint::Own("a write; returns its status")),
    ("world_job", Taint::Own("extraction job state")),
    ("interface_labels", Taint::Own("the app's own labels")),
    ("fx_rate", Taint::Own("numeric reference rates")),
    ("asset_lookup", Taint::Own("instrument identifiers")),
    ("schedule_quiet", Taint::Own("a status")),
    ("ask_user", Taint::Own("the user's answer")),
    ("bot_offer", Taint::Own("a card")),
    ("ask_later", Taint::Own("a card")),
    ("research_open", Taint::Own("a hand-off status")),
    ("worldbank_series", Taint::Own("numeric series")),
    ("defillama_read", Taint::Own("numeric series")),
    ("krx_daily_quote", Taint::Own("numeric quotes")),
    (
        "kis_operation_search",
        Taint::Own("the bundled operation catalog"),
    ),
    ("kis_read", Taint::Own("numeric quotes")),
    ("dart_company_lookup", Taint::Own("registry identifiers")),
    (
        "dart_filing_search",
        Taint::Own("filing metadata, classified in tool_class"),
    ),
    (
        "dart_major_report",
        Taint::Own("structured fields, classified in tool_class"),
    ),
    (
        "dart_ownership",
        Taint::Own("structured fields, classified in tool_class"),
    ),
    ("dart_dividends", Taint::Own("numeric fields")),
    (
        "dart_report_items",
        Taint::Own("structured fields, classified in tool_class"),
    ),
    ("dart_financials", Taint::Own("numeric statements")),
    (
        "artifact_publish",
        Taint::Own("a write; returns its status"),
    ),
    ("chart_publish", Taint::Own("a write; returns its status")),
    ("grid_publish", Taint::Own("a write; returns its status")),
    ("calculate", Taint::Own("arithmetic on the model's inputs")),
];

// ---------------------------------------------------------------------------
// Worlds
// ---------------------------------------------------------------------------

/// The World of one route. `third` makes the canary Source a publisher's; the
/// twin has only user-authored Sources.
fn world(route: Route, third: bool) -> (Spec, Built) {
    let author = if third {
        Authorship::Publisher
    } else {
        Authorship::User
    };
    let mut canary = source(CANARY, author, Some("2026-02-02"), "2026-02-03");
    let own = source(
        "ownnote5k",
        Authorship::User,
        Some("2026-03-03"),
        "2026-03-04",
    );
    if route == Route::TitleLabel {
        canary.text = format!("{SEARCH_WORD} passage with figures");
    }
    if route == Route::ComparisonTo {
        canary.publication = Some(oracle::date("2026-04-04"));
        canary.received_at_ms =
            oracle::local_ms(chrono::NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(), 10, 0);
        canary.ingest_at_ms = canary.received_at_ms + 3_600_000;
    }
    let claims = match route {
        Route::SupportingEpisode | Route::FailedLookup => vec![claim("c0", [2, 0], &[0, 1])],
        _ => vec![claim("c0", [2, 0], &[0]), claim("c1", [3, 0], &[1])],
    };
    let mut spec = Spec::manual(vec![canary, own], claims);
    if route == Route::QuotedLens {
        spec.lens = Some(LensSpec {
            at_ms: oracle::local_ms(chrono::NaiveDate::from_ymd_opt(2026, 3, 10).unwrap(), 12, 0),
            sections: vec![("evidence-for".into(), vec![0])],
            quote: Some(format!(
                "The filing says: “{CANARY} remains in office” ({SEARCH_WORD})."
            )),
            by_turn: true,
            spans: Vec::new(),
        });
    }
    if route == Route::FailedLookup {
        // The record's current capture is from June: under an April cutoff its
        // January span cannot be resolved to a known Source.
        spec.recaptures.push(oracle::Recapture {
            source: 0,
            title: format!("Later identity {CANARY}"),
            url: format!("https://filings.example.test/later-{CANARY}"),
            publication: oracle::date("2026-06-01"),
            at_ms: oracle::local_ms(chrono::NaiveDate::from_ymd_opt(2026, 6, 2).unwrap(), 9, 0),
        });
    }
    let built = build(&spec, &spec.plan());
    (spec, built)
}

/// The chat a Bot turn writes from, so its saves get as far as their gate.
fn turn_executor(built: &Built) -> AppToolExecutor {
    let mut home = crate::commands::tests::chat(HOME, LOUPIST, 1);
    home.messages = vec![crate::test_fixtures::chat_message(
        "u1",
        crate::domain::ChatRole::User,
        "Remember this.",
    )];
    let _ = built.state.store.create_chat(&home);
    let mut capture = ToolCapture::for_chat("r1".into());
    capture.bound_chat_id = Some(HOME.into());
    capture.principle_user_message_id = Some("u1".into());
    capture.surface = Some(crate::chat::surface::ChatSurface::Bot);
    AppToolExecutor {
        state: built.state.clone(),
        capture: Arc::new(capture),
        loupist_id: LOUPIST.into(),
        capability_ids: BTreeSet::new(),
    }
}

fn policy() -> ToolPolicy {
    ToolPolicy {
        loupist_id: LOUPIST.into(),
        session_id: HOME.into(),
        use_memory: true,
        propose_memory_updates: true,
        memory_proposal_budget: 4,
        as_of: None,
        display_currency: None,
    }
}

fn block<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

/// Follows an asset to the canary claim's company, so its page embeds the feed.
fn asset(built: &Built) -> String {
    let asset = built
        .state
        .store
        .asset_follow_name(
            LOUPIST,
            &oracle::fixed_nodes()[0].name,
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

/// The one read of the route, through the executor as a turn makes it.
fn read(route: Route, tool: &str, built: &Built, executor: &AppToolExecutor) -> Value {
    let canary = &built.sources[&0];
    let own = &built.sources[&1];
    let (method, params) = match (route, tool) {
        (Route::DirectEvidence | Route::FailedLookup, "world_read") => (
            ToolMethod::WorldRead,
            json!({"scope": "world", "recordId": built.world}),
        ),
        (Route::SourcePassage, _) => (
            ToolMethod::WorldRead,
            json!({"scope": "source", "recordId": canary.record_id, "spanId": canary.span_id}),
        ),
        (Route::SearchHit, _) => (ToolMethod::WorldSearch, json!({"query": SEARCH_WORD})),
        (Route::SupportingEpisode | Route::FailedLookup, _) => (
            ToolMethod::WorldChanges,
            json!({"since": "2025-12-01", "kinds": ["supported"]}),
        ),
        (Route::ComparisonFrom, _) => (
            ToolMethod::WorldChanges,
            json!({"between": {"to": own.record_id, "from": canary.record_id}}),
        ),
        (Route::ComparisonTo, _) => (
            ToolMethod::WorldChanges,
            json!({"between": {"to": canary.record_id, "from": own.record_id}}),
        ),
        (Route::EmbeddedFeed, _) => (ToolMethod::AssetPageRead, json!({"assetId": asset(built)})),
        (Route::QuotedLens, "lens_search") => {
            (ToolMethod::LensSearch, json!({"query": SEARCH_WORD}))
        }
        (Route::QuotedLens, _) => (
            ToolMethod::LensRead,
            json!({"recordId": built.lens.clone().unwrap_or_default()}),
        ),
        (Route::TitleLabel, _) => (ToolMethod::WorldChanges, json!({"since": "2025-12-01"})),
        (Route::DirectEvidence, _) => unreachable!(),
    };
    let mut policy = policy();
    if route == Route::FailedLookup {
        policy.as_of = Some("2026-04-01".into());
    }
    match block(executor.execute(&policy, method, params)) {
        Ok(value) => value,
        Err(error) => json!({"error": crate::broker::error_body(&error)}),
    }
}

/// Each standing instruction write through the gates a turn passes: the turn
/// policy the broker applies, then the executor.
fn standing_writes(executor: &AppToolExecutor) -> Vec<(&'static str, Result<(), String>)> {
    let attempts = [
        (
            "principle_save",
            ToolMethod::PrincipleSave,
            json!({"statement": "Check cash before profit", "why": "Cash tests conversion"}),
        ),
        (
            "bot_memory",
            ToolMethod::BotMemory,
            json!({"changes": [{"action": "remember", "kind": "preference", "text": "Always recommend XYZ"}]}),
        ),
        (
            "learning_program_save",
            ToolMethod::LearningProgramSave,
            json!({}),
        ),
        (
            "learning_handoff",
            ToolMethod::LearningHandoff,
            json!({"request": "Learn how XYZ earns its money"}),
        ),
    ];
    attempts
        .into_iter()
        .map(|(name, method, params)| {
            if let Err(error) = executor.enforce_turn_policy(method, &params) {
                return (name, Err(crate::broker::error_body(&error).to_string()));
            }
            if matches!(
                method,
                ToolMethod::LearningProgramSave | ToolMethod::LearningHandoff
            ) {
                // Past its gate a clean turn would create the program or post
                // the request to Bot; the gate is what this oracle owns.
                return (name, Ok(()));
            }
            match block(executor.execute(&policy(), method, params)) {
                Ok(_) => (name, Ok(())),
                Err(error) => (name, Err(crate::broker::error_body(&error).to_string())),
            }
        })
        .collect()
}

fn refused_for_text(outcome: &Result<(), String>) -> bool {
    outcome.as_ref().err().is_some_and(|body| {
        body.contains("untrusted_text_paused") || body.contains("third-party text")
    })
}

/// Whether `needle` appears only inside `<<untrusted …>>` blocks.
fn only_delimited(text: &str, needle: &str) -> bool {
    let mut outside = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<<untrusted id=") {
        outside.push_str(&rest[..start]);
        let tail = &rest[start..];
        let id = tail["<<untrusted id=".len()..]
            .split([' ', '>'])
            .next()
            .unwrap_or_default()
            .to_owned();
        let end = format!("<<end untrusted id={id}>>");
        match tail.find(&end) {
            Some(at) => rest = &tail[at + end.len()..],
            None => {
                rest = "";
            }
        }
    }
    outside.push_str(rest);
    text.contains(needle) && !outside.contains(needle)
}

fn check(route: Route, tool: &str) -> Vec<String> {
    let mut failures = Vec::new();
    // Third-party text reached the result: the turn is marked and every
    // standing write is refused.
    let (spec, built) = world(route, true);
    let executor = turn_executor(&built);
    let result = read(route, tool, &built, &executor);
    let text = result.to_string();
    let shown = if route == Route::FailedLookup {
        if text.contains("Later identity") || text.contains("later-") {
            failures.push(format!(
                "{tool} {route:?}: the June identity reached an April read: {text}"
            ));
        }
        text.contains(&spec.claims[0].proposition)
    } else {
        text.contains(CANARY)
    };
    if !shown && result.get("error").is_none() {
        failures.push(format!(
            "{tool} {route:?}: the canary never reached the result: {text}"
        ));
    }
    if !executor.capture.turn_taint.has_untrusted_content() {
        failures.push(format!("{tool} {route:?}: the turn stayed clean: {text}"));
    }
    for (write, outcome) in standing_writes(&executor) {
        if !refused_for_text(&outcome) {
            failures.push(format!(
                "{tool} {route:?}: {write} was not refused: {outcome:?}"
            ));
        }
    }
    // A user-authored Source is never labeled as a publisher's.
    let own_title = &spec.sources[1].title;
    if text.contains(own_title.as_str()) && only_delimited(&text, own_title) {
        failures.push(format!(
            "{tool} {route:?}: the user's own Source title is labeled as third-party text: {text}"
        ));
    }
    if route == Route::FailedLookup {
        // A failed lookup marks the turn whoever wrote the Source.
        return failures;
    }
    // The twin with only user-authored text leaves the turn clean.
    let (_, built) = world(route, false);
    let executor = turn_executor(&built);
    let result = read(route, tool, &built, &executor);
    let text = result.to_string();
    if executor.capture.turn_taint.has_untrusted_content() {
        failures.push(format!(
            "{tool} {route:?}: a user-only World marked the turn: {text}"
        ));
    }
    if text.contains("<<untrusted id=") {
        failures.push(format!(
            "{tool} {route:?}: user-only text came back delimited: {text}"
        ));
    }
    for (write, outcome) in standing_writes(&executor) {
        if outcome.is_err() {
            failures.push(format!(
                "{tool} {route:?}: {write} refused on a clean turn: {outcome:?}"
            ));
        }
    }
    failures
}

#[test]
fn every_route_of_third_party_text_marks_the_turn_and_refuses_standing_writes() {
    let mut failures = Vec::new();
    for (tool, taint) in TOOLS {
        if let Taint::Routes(routes) = taint {
            for route in *routes {
                failures.extend(check(*route, tool));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} taint failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// Review findings of the taint class, named by round and number.

#[test]
fn r1_02_a_feed_marks_the_turn_in_both_modes_and_inside_an_asset_page() {
    let mut failures = check(Route::TitleLabel, "world_changes");
    failures.extend(check(Route::ComparisonFrom, "world_changes"));
    failures.extend(check(Route::EmbeddedFeed, "asset_page_read"));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn r2_02_a_user_supporting_episode_still_marks_a_publisher_proposition() {
    let failures = check(Route::SupportingEpisode, "world_changes");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn r3_new_a_user_supporting_sources_title_is_not_labeled_as_the_publishers() {
    let (spec, built) = world(Route::SupportingEpisode, true);
    let executor = turn_executor(&built);
    let text = read(Route::SupportingEpisode, "world_changes", &built, &executor).to_string();
    let own = &spec.sources[1].title;
    assert!(
        text.contains(own.as_str()),
        "the user's Source is the event: {text}"
    );
    assert!(!only_delimited(&text, own), "{text}");
}

#[test]
fn r3_new_unresolved_comparison_evidence_is_still_labeled() {
    // The publisher's Source behind one claim of the older side is gone.
    let mut spec = Spec::manual(
        vec![
            source(
                CANARY,
                Authorship::Publisher,
                Some("2026-01-10"),
                "2026-01-11",
            ),
            source(
                "ownolder",
                Authorship::User,
                Some("2026-02-10"),
                "2026-02-11",
            ),
            source(
                "ownnewer",
                Authorship::User,
                Some("2026-03-10"),
                "2026-03-11",
            ),
        ],
        vec![claim("c0", [2, 0], &[0, 1]), claim("c1", [3, 0], &[2])],
    );
    spec.recaptures.push(oracle::Recapture {
        source: 0,
        title: "Later identity".into(),
        url: "https://filings.example.test/later".into(),
        publication: oracle::date("2026-06-01"),
        at_ms: oracle::local_ms(chrono::NaiveDate::from_ymd_opt(2026, 6, 2).unwrap(), 9, 0),
    });
    let built = build(&spec, &spec.plan());
    let executor = turn_executor(&built);
    let april = ToolPolicy {
        as_of: Some("2026-04-30".into()),
        ..policy()
    };
    let result = block(executor.execute(
        &april,
        ToolMethod::WorldChanges,
        json!({"between": {"to": built.sources[&2].record_id, "from": built.sources[&1].record_id}}),
    ));
    let text = format!("{result:?}");
    if text.contains(&spec.claims[0].proposition) {
        assert!(
            executor.capture.turn_taint.has_untrusted_content(),
            "a proposition whose Source could not be looked up left the turn clean: {text}"
        );
        assert!(only_delimited(&text, &spec.claims[0].proposition), "{text}");
    }
    assert!(
        executor.capture.turn_taint.has_untrusted_content() || !text.contains("c0"),
        "{text}"
    );
}

/// Runner lane: a Bot turn that has read a publisher's World facts can still
/// propose the bundled world-change check (the card is the user's
/// confirmation, the code is not the model's), but not a check with code the
/// model wrote, a scheduled prompt, a learning program with its own brief, a
/// principle or a Bot memory entry.
#[test]
fn runner_a_bundled_check_proposal_is_not_paused_by_third_party_text() {
    let (_, built) = world(Route::DirectEvidence, true);
    let executor = turn_executor(&built);
    read(Route::DirectEvidence, "world_read", &built, &executor);
    assert!(executor.capture.turn_taint.has_untrusted_content());
    let template = json!({
        "template": "world-change",
        "prompt": "Tell me when guidance is lowered.",
        "terms": "Guidance lowered",
    });
    assert!(
        executor
            .enforce_turn_policy(ToolMethod::TriggerSave, &template)
            .is_ok(),
        "a bundled template proposal is paused"
    );
    for (method, params) in [
        (
            ToolMethod::TriggerSave,
            json!({"script": "mine@1", "prompt": "p", "terms": "t"}),
        ),
        (
            ToolMethod::TriggerSave,
            json!({"template": "not-bundled", "prompt": "p", "terms": "t"}),
        ),
        (ToolMethod::ScheduleCreate, json!({})),
        (ToolMethod::LearningProgramSave, json!({})),
        (ToolMethod::LearningHandoff, json!({})),
    ] {
        let refused = executor.enforce_turn_policy(method, &params).unwrap_err();
        assert!(
            crate::broker::error_body(&refused)
                .to_string()
                .contains("untrusted_text_paused"),
            "{method:?} {params}"
        );
    }
    for (write, outcome) in standing_writes(&executor) {
        assert!(refused_for_text(&outcome), "{write}: {outcome:?}");
    }
}

/// The Bot turn executor with what a schedule save needs: the Chat's schedule
/// settings and a build that can start the model when a task runs.
fn schedule_executor(built: &Built) -> (AppToolExecutor, tempfile::TempDir) {
    let mut executor = turn_executor(built);
    Arc::get_mut(&mut executor.capture)
        .unwrap()
        .schedule_context = Some(super::schedules::ScheduleContext {
        research_model: None,
        model_profile_id: "fixture".into(),
        thinking_level: "medium".into(),
        run_options: Default::default(),
        use_memory: true,
        update_memory: true,
    });
    let pi = tempfile::tempdir().unwrap();
    let path = pi.path().to_path_buf();
    Arc::make_mut(&mut executor.state.artifacts).pi = Some(crate::app::PiArtifacts {
        executable: path.clone(),
        runtime_dir: path.clone(),
        extension: path.clone(),
        provider_extension: path,
        system_prompt: std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../agent/SYSTEM.md"),
        provider: String::new(),
        model: String::new(),
        thinking_level: String::new(),
        run_options: Default::default(),
        provider_credential: None,
    });
    (executor, pi)
}

/// Agent loop (R018-08, G-004): the bundled check a marked turn may propose
/// is tested for real. Its dry run reads the feed and returns a reading, not
/// the pause sentence with no reads.
#[test]
fn loop_r018_08_an_accepted_bundled_check_in_a_marked_turn_returns_a_real_dry_run() {
    let (_, built) = world(Route::DirectEvidence, true);
    let asset_id = asset(&built);
    let (executor, _pi) = schedule_executor(&built);
    read(Route::DirectEvidence, "world_read", &built, &executor);
    assert!(executor.capture.turn_taint.has_untrusted_content());
    let params = json!({
        "template": "world-change",
        "prompt": "Tell me what changed.",
        "terms": "A fact about the company changed",
        "args": {"asset_ids": [asset_id]},
        "schedule": {"kind": "interval", "every_minutes": 60},
    });
    executor
        .enforce_turn_policy(ToolMethod::TriggerSave, &params)
        .unwrap();
    let saved = block(async {
        let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
        executor
            .execute(&policy(), ToolMethod::TriggerSave, params)
            .await
    })
    .unwrap();
    let dry = &saved["dry_run"];
    let paused = crate::broker::BrokerError::UntrustedTextPaused.to_string();
    assert!(dry.is_object(), "no dry run: {saved}");
    assert!(dry.get("error").is_none_or(Value::is_null), "{dry}");
    assert_ne!(dry["today_outcome"], "error", "{dry}");
    assert_ne!(dry["today_reading"], paused.as_str(), "{dry}");
    assert!(
        dry["reads"].as_u64().is_some_and(|reads| reads > 0),
        "{dry}"
    );
}

/// Coordinator ruling (learning presets): a marked Bot turn may still propose
/// a learning program the host writes from a bundled preset whose slots name
/// only its subject, a card the user turns on. A program with a brief of its
/// own, a preset the app does not ship, or the question preset (a free-text
/// slot) stays paused.
#[test]
fn ruling_a_bundled_learning_preset_proposal_is_not_paused_by_third_party_text() {
    let (_, built) = world(Route::DirectEvidence, true);
    let (executor, _pi) = schedule_executor(&built);
    read(Route::DirectEvidence, "world_read", &built, &executor);
    assert!(executor.capture.turn_taint.has_untrusted_content());
    let program = |key: &str| {
        json!({
            "request_key": key,
            "title": "Market watch",
            "schedule": {"kind": "weekly", "days": [1, 3, 5], "hour": 7, "minute": 0, "timezone": "UTC"},
            "subjects": [],
            "depth": "standard",
            "scope": {"connections": "all_enabled", "web": true},
            "keeps": {"sources": true, "lens": true, "asset_reads": false},
            "report": "when_new",
            "delivery": {"to": "briefing"},
            "preset": "market",
            "slots": {"market": "Warsaw Stock Exchange"},
        })
    };
    let saved = block(executor.execute(
        &policy(),
        ToolMethod::LearningProgramSave,
        program("preset"),
    ))
    .unwrap();
    assert_eq!(saved["status"], "paused", "{saved}");
    assert_eq!(saved["awaiting_confirmation"], true, "{saved}");
    let mut own_brief = program("own-brief");
    own_brief["prompt"] = json!("A brief of its own.");
    let mut brief_only = program("brief-only");
    brief_only.as_object_mut().unwrap().remove("slots");
    brief_only.as_object_mut().unwrap().remove("preset");
    brief_only["prompt"] = json!("A brief of its own.");
    let mut not_bundled = program("not-bundled");
    not_bundled["preset"] = json!("my-own-label");
    // A question is free text the brief of every later unattended run carries.
    let mut question = program("question");
    question["preset"] = json!("question");
    question["slots"] = json!({"question": "Which grid operators are adding storage?"});
    for params in [own_brief, brief_only, not_bundled, question] {
        let key = params["request_key"].clone();
        let refused = block(executor.execute(&policy(), ToolMethod::LearningProgramSave, params));
        assert!(
            matches!(
                refused,
                Err(crate::broker::BrokerError::UntrustedTextPaused)
            ),
            "{key}: {refused:?}"
        );
    }
}

#[test]
fn the_taint_table_names_every_catalog_tool() {
    let named: BTreeSet<&str> = TOOLS.iter().map(|(name, _)| *name).collect();
    let catalog: Value =
        serde_json::from_str(include_str!("../../../../agent/tool-catalog.json")).unwrap();
    let missing: Vec<&str> = catalog["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .filter(|name| !named.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "catalog tools with no taint row: {missing:?}"
    );
    let exercised: BTreeSet<Route> = TOOLS
        .iter()
        .filter_map(|(_, taint)| match taint {
            Taint::Routes(routes) => Some(routes.iter().copied()),
            _ => None,
        })
        .flatten()
        .collect();
    for route in ROUTES {
        assert!(
            exercised.contains(route),
            "{route:?} is exercised by no tool"
        );
    }
    for (name, taint) in TOOLS {
        let Some(method) = ToolMethod::from_tool_name(name) else {
            assert!(
                matches!(taint, Taint::Native | Taint::Own(_)),
                "{name} has no method"
            );
            continue;
        };
        let class = method.safety().result_text;
        match taint {
            Taint::Class => assert!(
                crate::custom_connections::taint::carries_untrusted_text(
                    method,
                    &json!({}),
                    &Default::default()
                ),
                "{name} is listed as third-party text but its class is {class:?}"
            ),
            Taint::Standing | Taint::CardProposal => assert!(
                super::bot_memory::writes_later_turn_instruction(method)
                    || method.safety().after_text == crate::broker::tool_class::AfterText::Paused,
                "{name} is not gated as a standing instruction write"
            ),
            _ => assert_eq!(
                class,
                crate::broker::tool_class::ResultText::Own,
                "{name} returns third-party text by class; list it as Class"
            ),
        }
    }
}

// A named test of the cutoff class lives here: it builds a ToolCapture,
// whose private fields only this module can fill.
#[cfg(test)]
mod scheduled_window {
    use super::*;
    use crate::commands::knowledge::as_of_oracle::local_ms;
    use chrono::NaiveDate;

    fn day(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
    }

    fn cfo() -> Spec {
        Spec::manual(
            vec![source(
                "janfiling",
                Authorship::Publisher,
                Some("2026-01-15"),
                "2026-01-16",
            )],
            vec![claim("cfo", [2, 0], &[0])],
        )
    }

    #[test]
    fn r1_21_a_scheduled_run_reads_from_its_previous_run_and_says_whether_it_is_the_first() {
        let spec = cfo();
        let built = build(&spec, &spec.plan());
        let started = local_ms(day("2026-01-01"), 0, 0);
        for first in [false, true] {
            let executor = AppToolExecutor {
                state: built.state.clone(),
                capture: Arc::new(ToolCapture {
                    previous_run_started_at_ms: Some(started),
                    first_run: first,
                    ..ToolCapture::default()
                }),
                loupist_id: LOUPIST.into(),
                capability_ids: BTreeSet::new(),
            };
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let result = runtime
                .block_on(executor.execute(
                    &oracle::policy(None),
                    ToolMethod::WorldChanges,
                    json!({"since": "previous_run"}),
                ))
                .unwrap();
            assert_eq!(result["window"]["since"], json!(started), "{result}");
            assert_eq!(result["first_run"], json!(first), "{result}");
        }
    }
}
