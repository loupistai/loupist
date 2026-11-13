use super::*;

#[test]
fn extraction_admits_named_entities_but_not_measurements_or_sentences_in_any_type() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../../../evals/world/entity-name-corpus-v1.json"
    ))
    .unwrap();
    let rows = corpus.as_array().unwrap();
    let mut schema = merged_schema(&Default::default());
    for row in rows {
        let kind = row["kind"].as_str().unwrap();
        if kind != "Entity" && schema.entity_type(kind).is_none() {
            schema.entity_types.push(ontology::EntityType {
                name: kind.into(),
                doc: "User-defined tracked kind".into(),
                fields: vec![],
            });
        }
    }
    let candidates: Vec<Value> = rows
        .iter()
        .map(|row| {
            let kind = row["kind"].as_str().unwrap();
            let id = schema
                .entity_types
                .iter()
                .position(|item| item.name == kind)
                .map_or(0, |i| i + 1);
            json!({"name": row["name"], "entity_type_id": id})
        })
        .collect();
    let case = case("extract_nodes");
    let (episode, previous) = episode(&case);
    let mut host = Host::new(&json!({"responses": [{"extracted_entities": candidates}]}));
    let mut infer = |request| host.reply(request);
    let mut bridge = Bridge::new(config(&case), &mut infer);
    let text = empty_text();
    let mut harness = Harness::new(
        &mut bridge,
        &schema,
        episode,
        previous,
        String::new(),
        Graph::default(),
        &text,
    );
    let indices = harness.extract_nodes().unwrap();
    let names: HashSet<&str> = indices
        .iter()
        .map(|i| harness.node(*i).name.as_str())
        .collect();
    let expected: HashSet<&str> = rows
        .iter()
        .filter(|row| row["entity"] == true)
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, expected);
    assert!(indices
        .iter()
        .all(|i| harness.node(*i).kind() != Some("Metric")));
}

#[test]
fn a_single_subject_retains_reported_and_forecast_values_as_claims() {
    let case = case("extract_edges");
    let (schema, instructions) = schema_and_instructions(&case);
    let (episode, previous) = episode(&case);
    let reported = "Cedar reported Q4 FY2026 GAAP net income USD 42,960 million.";
    let forecast =
        "An analyst forecasts Cedar Q1 FY2027 revenue USD 99 billion; this is an estimate.";
    let responses = json!({"responses": [{"edges": [
        {"source_entity_name": "Cedar", "target_entity_name": "Cedar", "relation_type": "REPORTS", "fact": reported},
        {"source_entity_name": "Cedar", "target_entity_name": "Cedar", "relation_type": "REPORTS", "fact": forecast},
        {"source_entity_name": "Cedar", "target_entity_name": "Cedar", "relation_type": "OWNS", "fact": "Cedar owns itself."}
    ]}]});
    let mut host = Host::new(&responses);
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
    let subject = harness.add_node(node(
        &json!({"uuid":"cedar", "name":"Cedar", "labels":["Entity","Organization"]}),
    ));
    let indices = harness.extract_edges(&[subject]).unwrap();
    assert_eq!(indices.len(), 2);
    for (index, fact) in indices.iter().zip([reported, forecast]) {
        let edge = harness.edge(*index);
        assert_eq!(edge.source, "cedar");
        assert_eq!(edge.target, "cedar");
        assert_eq!(edge.fact, fact);
        assert!(edge.valid_at.is_none() && edge.invalid_at.is_none());
    }
}
