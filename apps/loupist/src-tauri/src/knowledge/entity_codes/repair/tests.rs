use std::collections::HashMap;

use serde_json::{json, Value};

use super::*;
use crate::knowledge::model::{ClaimEndpoint, GraphClaim, GraphNode};

const SUBARU: &str = "nod_2d69b68255b1f7fe81fd7ae614cbeb1c";
const SUBARU_AGAIN: &str = "nod_7ce2848d65a1587a792dfd22380b455f";
const DONGFENG: &str = "nod_19a6f236445d44088da76e89d4b5b536";
const DONGFENG_AGAIN: &str = "nod_8e8d35da6c8c36e8dd485304b9760881";

/// The aliases one-row-late resolution left, as (node, alias).
const WRONG_ALIASES: [(&str, &str); 8] = [
    ("Subaru 7270.T", "Seres Group 601127.SS"),
    ("Seres Group 601127.SS", "Chongqing Changan 000625.SZ"),
    ("Chongqing Changan 000625.SZ", "Li Auto LI"),
    ("Dongfeng Motor 0489.HK", "Stellantis STLA"),
    ("Stellantis STLA", "Isuzu 7202.T"),
    ("Isuzu 7202.T", "Hotai Motor 2207.TW"),
    ("Hotai Motor 2207.TW", "Renault RNO.PA"),
    ("Renault RNO.PA", "XPeng XPEV"),
];

struct Stored {
    nodes: Vec<GraphNode>,
    claims: Vec<GraphClaim>,
    first_seen: HashMap<String, i64>,
}

fn stored() -> Stored {
    let fixture: Value = serde_json::from_str(include_str!("tester_world.json")).unwrap();
    let mut claims: Vec<GraphClaim> = serde_json::from_value(fixture["claims"].clone()).unwrap();
    // Two claims the stored World does not have, so the merge has claims to
    // move: a value reported about the second Dongfeng node, and a relation
    // between the two Subaru nodes.
    let mut reported = claims[0].clone();
    reported.id = "clm_reported_on_the_second_dongfeng".into();
    reported.relation = "REPORTS".into();
    reported.proposition = "Dongfeng Motor 0489.HK ranks 29 with market cap $10.05 B.".into();
    reported.endpoints = vec![
        ClaimEndpoint {
            node_id: DONGFENG_AGAIN.into(),
            role: "source".into(),
        },
        ClaimEndpoint {
            node_id: DONGFENG_AGAIN.into(),
            role: "target".into(),
        },
    ];
    let mut between = claims[1].clone();
    between.id = "clm_between_the_two_subarus".into();
    between.relation = "COMPETES_WITH".into();
    between.proposition = "Subaru 7270.T competes with Subaru 7270.T.".into();
    between.endpoints = vec![
        ClaimEndpoint {
            node_id: SUBARU_AGAIN.into(),
            role: "source".into(),
        },
        ClaimEndpoint {
            node_id: SUBARU.into(),
            role: "target".into(),
        },
    ];
    claims.extend([reported, between]);
    Stored {
        nodes: serde_json::from_value(fixture["nodes"].clone()).unwrap(),
        claims,
        first_seen: serde_json::from_value(fixture["firstSeen"].clone()).unwrap(),
    }
}

fn run(stored: &mut Stored) -> (Repaired, BTreeMap<String, String>) {
    let first_seen = stored.first_seen.clone();
    let mut reads = 0;
    let result = repair(&mut stored.nodes, &mut stored.claims, &mut || {
        reads += 1;
        Ok(first_seen.clone())
    })
    .unwrap();
    assert!(reads <= 1, "first-seen times are read once");
    result
}

fn node<'a>(nodes: &'a [GraphNode], id: &str) -> &'a GraphNode {
    nodes.iter().find(|node| node.id == id).unwrap()
}

#[test]
fn the_stored_world_loses_its_wrong_aliases_and_its_duplicates() {
    let original = stored();
    let mut world = stored();
    let (repaired, merged) = run(&mut world);
    assert_eq!(
        repaired,
        Repaired {
            aliases_dropped: 8,
            nodes_merged: 2,
            claims_dropped: 1,
        }
    );
    // The node World saw first survives each code.
    assert_eq!(
        merged,
        BTreeMap::from([
            (SUBARU_AGAIN.to_string(), SUBARU.to_string()),
            (DONGFENG_AGAIN.to_string(), DONGFENG.to_string()),
        ])
    );
    for (name, alias) in WRONG_ALIASES {
        let holder = world.nodes.iter().find(|node| node.name == name).unwrap();
        assert!(
            !holder.aliases.iter().any(|kept| kept == alias),
            "{name}: {alias}"
        );
    }
    // Every other node and alias is as stored: an alias whose code agrees
    // ("Tesla (TSLA)", "Yulon Motor Company 2201.TW") or that has none stays.
    for before in &original.nodes {
        if [SUBARU_AGAIN, DONGFENG_AGAIN].contains(&before.id.as_str()) {
            continue;
        }
        let after = node(&world.nodes, &before.id);
        let wrong: Vec<&str> = WRONG_ALIASES
            .iter()
            .filter(|(name, _)| *name == before.name)
            .map(|(_, alias)| *alias)
            .collect();
        let kept: Vec<&String> = before
            .aliases
            .iter()
            .filter(|alias| !wrong.contains(&alias.as_str()))
            .collect();
        assert_eq!(
            after.aliases.iter().collect::<Vec<_>>(),
            kept,
            "{}",
            before.name
        );
        assert_eq!(after.properties, before.properties, "{}", before.name);
        assert_eq!(after.kind, before.kind, "{}", before.name);
    }
    assert_eq!(world.nodes.len(), original.nodes.len() - 2);
    let tesla = world
        .nodes
        .iter()
        .find(|node| node.name == "Tesla, Inc.")
        .unwrap();
    assert!(tesla.aliases.contains(&"Tesla (TSLA)".to_string()));
    // The survivor keeps both summaries and no second alias of its own name.
    let dongfeng = node(&world.nodes, DONGFENG);
    assert!(dongfeng.aliases.is_empty());
    assert_eq!(
        dongfeng.description,
        "Dongfeng Motor 0489.HK is located in China.\n\
         Dongfeng Motor 0489.HK ranks 29 with market cap $10.05 B, price $1.22 up 1.71% today, based in China."
    );
    // Claims keep their ids and evidence; the value moves to the survivor and
    // the relation between the two Subaru nodes, now one entity, goes.
    let ids: Vec<&str> = world.claims.iter().map(|claim| claim.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "clm_8382596462673e58e43c817850a8078f",
            "clm_a2742607e41125725644b981953d7f41",
            "clm_reported_on_the_second_dongfeng",
        ]
    );
    let reported = &world.claims[2];
    assert!(reported
        .endpoints
        .iter()
        .all(|endpoint| endpoint.node_id == DONGFENG));
    assert_eq!(reported.evidence, original.claims[2].evidence);
    assert_eq!(world.claims[..2], original.claims[..2]);
}

#[test]
fn a_second_repair_changes_nothing() {
    let mut world = stored();
    run(&mut world);
    let (nodes, claims) = (world.nodes.clone(), world.claims.clone());
    let (repaired, merged) = run(&mut world);
    assert_eq!(repaired, Repaired::default());
    assert!(merged.is_empty());
    assert_eq!(world.nodes, nodes);
    assert_eq!(world.claims, claims);
}

#[test]
fn a_world_without_shared_codes_reads_no_history() {
    let mut nodes: Vec<GraphNode> = serde_json::from_value(json!([
        {"id": "nod_a", "name": "Ford F", "aliases": ["Ford"], "description": "",
         "kind": "Organization", "sourceRole": "source", "properties": {"ticker": "F"}},
        {"id": "nod_b", "name": "Ford Otosan FROTO.IS", "aliases": [], "description": "",
         "kind": "Organization", "sourceRole": "source", "properties": {"ticker": "FROTO.IS"}},
        {"id": "nod_c", "name": "Ford", "aliases": [], "description": "",
         "kind": "Product", "sourceRole": "source", "properties": {}},
    ]))
    .unwrap();
    let before = nodes.clone();
    let mut claims = Vec::new();
    let (repaired, merged) = repair(&mut nodes, &mut claims, &mut || {
        panic!("no two nodes share a code")
    })
    .unwrap();
    assert_eq!(repaired, Repaired::default());
    assert!(merged.is_empty());
    assert_eq!(nodes, before);
}

#[test]
fn merges_continue_while_a_merged_node_shares_a_new_code() {
    // "Alpha Holdings" shares ticker A with "Alpha A" and an ISIN with "Alpha",
    // whose ticker B differs from A, so "Alpha" waits. Once "Alpha A" holds
    // the ISIN too, the next round joins "Alpha". A Product spelled like the
    // company with the same ticker never joins it.
    let mut nodes: Vec<GraphNode> = serde_json::from_value(json!([
        {"id": "nod_1", "name": "Alpha A", "aliases": [], "description": "",
         "kind": "Organization", "sourceRole": "source", "properties": {"ticker": "A"}},
        {"id": "nod_2", "name": "Alpha Holdings", "aliases": [], "description": "",
         "kind": "Organization", "sourceRole": "source",
         "properties": {"ticker": "A", "identifier": "US0378331005"}},
        {"id": "nod_3", "name": "Alpha", "aliases": [], "description": "",
         "kind": "Entity", "sourceRole": "source",
         "properties": {"isin": "US0378331005", "ticker": "B"}},
        {"id": "nod_4", "name": "Alpha A", "aliases": [], "description": "",
         "kind": "Product", "sourceRole": "source", "properties": {"ticker": "A"}},
    ]))
    .unwrap();
    let mut claims = Vec::new();
    let order = HashMap::from([
        ("nod_1".to_string(), 1),
        ("nod_2".to_string(), 2),
        ("nod_3".to_string(), 3),
    ]);
    let (repaired, merged) = repair(&mut nodes, &mut claims, &mut || Ok(order.clone())).unwrap();
    assert_eq!(repaired.nodes_merged, 2);
    assert_eq!(merged["nod_2"], "nod_1");
    assert_eq!(merged["nod_3"], "nod_1");
    let ids: Vec<&str> = nodes.iter().map(|node| node.id.as_str()).collect();
    assert_eq!(ids, ["nod_1", "nod_4"]);
    assert_eq!(nodes[0].aliases, ["Alpha", "Alpha Holdings"]);
    assert_eq!(nodes[0].properties["identifier"], json!("US0378331005"));
    assert_eq!(nodes[0].properties["ticker"], json!("A"));
    let (again, _) = repair(&mut nodes, &mut claims, &mut || Ok(order.clone())).unwrap();
    assert_eq!(again, Repaired::default());
}

#[test]
fn codes_that_only_look_alike_are_left_as_the_model_resolved_them() {
    let organization = |id: &str, name: &str, aliases: &[&str], properties: Value| {
        serde_json::from_value::<GraphNode>(json!({
            "id": id, "name": name, "aliases": aliases, "description": "",
            "kind": "Organization", "sourceRole": "source", "properties": properties,
        }))
        .unwrap()
    };
    let mut nodes = vec![
        organization("nod_google", "Google AI", &[], json!({})),
        organization("nod_meta", "Meta AI", &[], json!({})),
        organization("nod_apple", "Apple TV", &[], json!({})),
        organization("nod_samsung_tv", "Samsung TV", &[], json!({})),
        organization("nod_samsung_sdi", "Samsung SDI", &[], json!({})),
        organization("nod_hanwha_sdi", "Hanwha SDI", &[], json!({})),
        organization(
            "nod_toyota",
            "Toyota TM",
            &["Toyota 7203.T"],
            json!({"ticker": "TM"}),
        ),
        organization(
            "nod_alphabet",
            "Alphabet GOOG",
            &["Alphabet GOOGL"],
            json!({"ticker": "GOOG"}),
        ),
        organization("nod_tesla", "Tesla TSLA", &[], json!({"ticker": "TSLA"})),
        organization("nod_tesla_inc", "Tesla Inc TSLA", &[], json!({})),
    ];
    let before = nodes.clone();
    let mut claims = Vec::new();
    let order = HashMap::from([
        ("nod_tesla".to_string(), 1),
        ("nod_tesla_inc".to_string(), 2),
    ]);
    let (repaired, merged) = repair(&mut nodes, &mut claims, &mut || Ok(order.clone())).unwrap();
    // Only the two Tesla names, whose codes are equal and names agree, join.
    assert_eq!(
        repaired,
        Repaired {
            aliases_dropped: 0,
            nodes_merged: 1,
            claims_dropped: 0,
        }
    );
    assert_eq!(
        merged,
        BTreeMap::from([("nod_tesla_inc".to_string(), "nod_tesla".to_string())])
    );
    let tesla = nodes.iter().find(|node| node.id == "nod_tesla").unwrap();
    assert_eq!(tesla.aliases, ["Tesla Inc TSLA"]);
    let untouched: Vec<&GraphNode> = before
        .iter()
        .filter(|node| !node.id.starts_with("nod_tesla"))
        .collect();
    assert_eq!(
        nodes
            .iter()
            .filter(|node| !node.id.starts_with("nod_tesla"))
            .collect::<Vec<_>>(),
        untouched
    );
}
