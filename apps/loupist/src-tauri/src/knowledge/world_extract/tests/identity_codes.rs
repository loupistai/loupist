//! Entity resolution under identifying codes, replayed from a tester's World:
//! a CompaniesMarketCap "largest automakers by market cap" page whose rows 24
//! to 35 the model resolved one row late. Each name got the candidate meant for
//! the row above it, so Seres Group was merged into Subaru, Chongqing Changan
//! into Seres Group, Li Auto into Chongqing Changan, Stellantis into Dongfeng
//! and so on down to XPeng, while Subaru and Dongfeng took the previous row's
//! "no duplicate" and became second nodes. The stored World kept exactly those
//! aliases and duplicates.

use super::*;
use crate::knowledge::world_extract::pipeline::ResolutionTally;

/// The automakers the World held before the page, with their stored ids and
/// tickers, in the order the candidate pool lists them.
const STORED: [(&str, &str, &str); 8] = [
    (
        "nod_2d69b68255b1f7fe81fd7ae614cbeb1c",
        "Subaru 7270.T",
        "7270.T",
    ),
    (
        "nod_d7a8adf7c5ab79fcbf4ae89d21e26125",
        "Seres Group 601127.SS",
        "601127.SS",
    ),
    (
        "nod_096aac54ee74c6d69c08b0c99240e57d",
        "Chongqing Changan 000625.SZ",
        "000625.SZ",
    ),
    (
        "nod_19a6f236445d44088da76e89d4b5b536",
        "Dongfeng Motor 0489.HK",
        "0489.HK",
    ),
    (
        "nod_1a6c4793e29dacf6841f3257e2ba11f9",
        "Stellantis STLA",
        "STLA",
    ),
    (
        "nod_a39cf94f4f7ced38fa2d44d8241d7355",
        "Isuzu 7202.T",
        "7202.T",
    ),
    (
        "nod_473676aed5a24c0f2370c783875405ad",
        "Hotai Motor 2207.TW",
        "2207.TW",
    ),
    (
        "nod_f765ca96fd6ec5cd1cb531968f6d6291",
        "Renault RNO.PA",
        "RNO.PA",
    ),
];

/// The page's rows 24 to 35 as extracted, and the candidate id the model
/// answered for each: the right answer for the row above it.
const ROWS: [(&str, i64); 12] = [
    ("Rivian RIVN", -1),
    ("Subaru 7270.T", -1),
    ("Seres Group 601127.SS", 0),
    ("Chongqing Changan 000625.SZ", 1),
    ("Li Auto LI", 2),
    ("Dongfeng Motor 0489.HK", -1),
    ("Stellantis STLA", 3),
    ("Isuzu 7202.T", 4),
    ("Hotai Motor 2207.TW", 5),
    ("Renault RNO.PA", 6),
    ("XPeng XPEV", 7),
    ("GAC (Guangzhou Automobile Group) 601238.SS", -1),
];

/// The aliases the stored World carries from that reply, as (node, alias).
const STORED_ALIASES: [(&str, &str); 8] = [
    ("Subaru 7270.T", "Seres Group 601127.SS"),
    ("Seres Group 601127.SS", "Chongqing Changan 000625.SZ"),
    ("Chongqing Changan 000625.SZ", "Li Auto LI"),
    ("Dongfeng Motor 0489.HK", "Stellantis STLA"),
    ("Stellantis STLA", "Isuzu 7202.T"),
    ("Isuzu 7202.T", "Hotai Motor 2207.TW"),
    ("Hotai Motor 2207.TW", "Renault RNO.PA"),
    ("Renault RNO.PA", "XPeng XPEV"),
];

fn organization(uuid: &str, name: &str, ticker: Option<&str>) -> Node {
    Node {
        uuid: uuid.into(),
        name: name.into(),
        labels: vec!["Entity".into(), "Organization".into()],
        summary: String::new(),
        attributes: ticker
            .map(|ticker| vec![("ticker".to_string(), json!(ticker))])
            .unwrap_or_default(),
        name_embedding: None,
        spellings: Vec::new(),
    }
}

fn stored() -> Vec<Node> {
    STORED
        .iter()
        .map(|(id, name, ticker)| organization(id, name, Some(ticker)))
        .collect()
}

#[test]
fn the_reply_as_given_reproduces_the_stored_merges() {
    // Read the reply the way resolution did before codes were held: each
    // answered candidate absorbs the row's name as an alias, and a row
    // answered -1 whose name is already stored becomes a second node.
    let mut aliases = Vec::new();
    let mut duplicates = Vec::new();
    for (name, candidate) in ROWS {
        match usize::try_from(candidate) {
            Ok(candidate) => aliases.push((STORED[candidate].1, name)),
            Err(_) if STORED.iter().any(|(_, stored, _)| *stored == name) => duplicates.push(name),
            Err(_) => {}
        }
    }
    assert_eq!(aliases, STORED_ALIASES);
    assert_eq!(duplicates, ["Subaru 7270.T", "Dongfeng Motor 0489.HK"]);
}

#[test]
fn codes_overrule_a_reply_that_resolves_every_row_one_row_late() {
    let case = case("resolve_nodes");
    let (schema, instructions) = schema_and_instructions(&case);
    let (episode, previous) = episode(&case);
    let reply = json!({"entity_resolutions": ROWS
        .iter()
        .enumerate()
        .map(|(id, (name, candidate))| json!({
            "id": id, "name": name, "duplicate_candidate_id": candidate,
        }))
        .collect::<Vec<_>>()});
    let mut host = Host::new(&json!({"responses": [reply]}));
    let mut infer = |request| host.reply(request);
    let mut bridge = Bridge::new(config(&case), &mut infer);
    let text = empty_text();
    let mut harness = Harness::new(
        &mut bridge,
        &schema,
        episode,
        previous,
        instructions,
        Graph {
            nodes: stored(),
            edges: Vec::new(),
            links: Vec::new(),
        },
        &text,
    );
    let extracted: Vec<usize> = ROWS
        .iter()
        .enumerate()
        .map(|(row, (name, _))| harness.add_node(organization(&format!("new-{row}"), name, None)))
        .collect();
    // Every row had the stored automakers as its name-vector candidates.
    let candidates = vec![stored(); ROWS.len()];
    let (resolved, uuid_map) = harness
        .resolve_with_candidates(&extracted, candidates)
        .unwrap();
    let winners: Vec<(&str, &str)> = resolved
        .iter()
        .map(|index| {
            let node = harness.node(*index);
            (node.uuid.as_str(), node.name.as_str())
        })
        .collect();
    let stored_id = |name: &str| STORED.iter().find(|(_, n, _)| *n == name).unwrap().0;
    let expected: Vec<(String, &str)> = ROWS
        .iter()
        .enumerate()
        .map(|(row, (name, _))| {
            let uuid = if STORED.iter().any(|(_, stored, _)| stored == name) {
                stored_id(name).to_string()
            } else {
                format!("new-{row}")
            };
            (uuid, *name)
        })
        .collect();
    assert_eq!(
        winners,
        expected
            .iter()
            .map(|(uuid, name)| (uuid.as_str(), *name))
            .collect::<Vec<_>>()
    );
    // No stored automaker took another row's name as its spelling.
    for index in &resolved {
        assert!(harness.node(*index).spellings.is_empty());
    }
    assert_eq!(uuid_map["new-1"], stored_id("Subaru 7270.T"));
    assert_eq!(uuid_map["new-5"], stored_id("Dongfeng Motor 0489.HK"));
    assert_eq!(uuid_map["new-4"], "new-4");
    assert_eq!(
        harness.tally(),
        ResolutionTally {
            proposed: 8,
            refused: 8,
            by_code: 8,
        }
    );
}

#[test]
fn a_name_without_a_code_is_still_the_models_decision() {
    let case = case("resolve_nodes");
    let (schema, instructions) = schema_and_instructions(&case);
    let (episode, previous) = episode(&case);
    // "Subaru" names no code: the model's merge stands. "Subaru Corporation
    // (7270)" carries one equal to the stored node's, so it joins that node
    // although the model answered -1 and no name vector found it.
    let reply = json!({"entity_resolutions": [
        {"id": 0, "name": "Subaru", "duplicate_candidate_id": 0},
    ]});
    let mut host = Host::new(&json!({"responses": [reply]}));
    let mut infer = |request| host.reply(request);
    let mut bridge = Bridge::new(config(&case), &mut infer);
    let text = empty_text();
    let mut harness = Harness::new(
        &mut bridge,
        &schema,
        episode,
        previous,
        instructions,
        Graph {
            nodes: stored(),
            edges: Vec::new(),
            links: Vec::new(),
        },
        &text,
    );
    let extracted = vec![
        harness.add_node(organization("new-0", "Subaru", None)),
        harness.add_node(organization("new-1", "Subaru Corporation (7270)", None)),
    ];
    let candidates = vec![vec![stored()[0].clone()], Vec::new()];
    let (resolved, _) = harness
        .resolve_with_candidates(&extracted, candidates)
        .unwrap();
    let subaru = STORED[0].0;
    assert_eq!(harness.node(resolved[0]).uuid, subaru);
    assert_eq!(harness.node(resolved[1]).uuid, subaru);
    assert_eq!(
        harness.node(resolved[0]).spellings,
        ["Subaru", "Subaru Corporation (7270)"]
    );
    assert_eq!(
        harness.tally(),
        ResolutionTally {
            proposed: 1,
            refused: 0,
            by_code: 1,
        }
    );
}

#[test]
fn codes_that_only_look_alike_leave_the_decision_to_the_model() {
    let case = case("resolve_nodes");
    let (schema, instructions) = schema_and_instructions(&case);
    let (episode, previous) = episode(&case);
    let stored = vec![
        organization("stored-google", "Google AI", None),
        organization("stored-apple", "Apple TV", None),
        organization("stored-sdi", "Samsung SDI", None),
        organization("stored-toyota", "Toyota TM", Some("TM")),
        organization("stored-tesla", "Tesla TSLA", Some("TSLA")),
    ];
    // The model keeps the three look-alikes apart, merges the Tokyo listing
    // into the stored Toyota and does not find Tesla.
    let rows: [(&str, i64); 5] = [
        ("Meta AI", -1),
        ("Samsung TV", -1),
        ("Hanwha SDI", -1),
        ("Toyota 7203.T", 3),
        ("Tesla Inc TSLA", -1),
    ];
    let reply = json!({"entity_resolutions": rows
        .iter()
        .enumerate()
        .map(|(id, (name, candidate))| json!({
            "id": id, "name": name, "duplicate_candidate_id": candidate,
        }))
        .collect::<Vec<_>>()});
    let mut host = Host::new(&json!({"responses": [reply]}));
    let mut infer = |request| host.reply(request);
    let mut bridge = Bridge::new(config(&case), &mut infer);
    let text = empty_text();
    let mut harness = Harness::new(
        &mut bridge,
        &schema,
        episode,
        previous,
        instructions,
        Graph {
            nodes: stored.clone(),
            edges: Vec::new(),
            links: Vec::new(),
        },
        &text,
    );
    let extracted: Vec<usize> = rows
        .iter()
        .enumerate()
        .map(|(row, (name, _))| harness.add_node(organization(&format!("new-{row}"), name, None)))
        .collect();
    let (resolved, _) = harness
        .resolve_with_candidates(&extracted, vec![stored; rows.len()])
        .unwrap();
    let winners: Vec<&str> = resolved
        .iter()
        .map(|index| harness.node(*index).uuid.as_str())
        .collect();
    // An equal bare "AI", "TV" or "SDI" under names that do not agree joins
    // nothing; codes that differ under agreeing names leave the model's merge.
    assert_eq!(
        winners,
        ["new-0", "new-1", "new-2", "stored-toyota", "stored-tesla"]
    );
    assert_eq!(harness.node(resolved[3]).spellings, ["Toyota 7203.T"]);
    assert_eq!(
        harness.tally(),
        ResolutionTally {
            proposed: 1,
            refused: 0,
            by_code: 1,
        }
    );
}

#[test]
fn a_model_merge_across_a_market_split_is_refused() {
    let berkshire = |uuid: &str, ticker: &str| Node {
        uuid: uuid.into(),
        name: "Berkshire Hathaway".into(),
        labels: vec!["Entity".into(), "Organization".into()],
        summary: String::new(),
        attributes: vec![
            ("ticker".into(), json!(ticker)),
            ("exchange".into(), json!("NYSE")),
            ("cik".into(), json!("0001067983")),
        ],
        name_embedding: None,
        spellings: Vec::new(),
    };
    let case = case("resolve_nodes");
    let (schema, instructions) = schema_and_instructions(&case);
    let (episode, previous) = episode(&case);
    let stored = berkshire("stored-b", "BRK.B");
    let reply = json!({"entity_resolutions": [
        {"id": 0, "name": "Berkshire Hathaway", "duplicate_candidate_id": 0},
    ]});
    let mut host = Host::new(&json!({"responses": [reply]}));
    let mut infer = |request| host.reply(request);
    let mut bridge = Bridge::new(config(&case), &mut infer);
    let text = empty_text();
    let mut harness = Harness::new(
        &mut bridge,
        &schema,
        episode,
        previous,
        instructions,
        Graph {
            nodes: vec![stored.clone()],
            edges: Vec::new(),
            links: Vec::new(),
        },
        &text,
    );
    let extracted = vec![harness.add_node(berkshire("new-a", "BRK.A"))];
    let (resolved, _) = harness
        .resolve_with_candidates(&extracted, vec![vec![stored]])
        .unwrap();
    assert_eq!(harness.node(resolved[0]).uuid, "new-a");
    assert!(harness.node(resolved[0]).spellings.is_empty());
    assert_eq!(
        harness.tally(),
        ResolutionTally {
            proposed: 1,
            refused: 1,
            by_code: 0,
        }
    );
}
