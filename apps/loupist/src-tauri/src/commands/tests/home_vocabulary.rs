use super::*;
use crate::chat::home::{deliver_finished_runs, home_thread_id};
use crate::knowledge::{ExtensionCategory, ExtensionLabels, ExtensionType, Producer};

fn kind(name: &str, en: &str, ko: &str) -> ExtensionType {
    ExtensionType {
        name: name.into(),
        description: "A Loupist-specific kind kept distinct from the shared core types.".into(),
        category: ExtensionCategory::Product,
        labels: ExtensionLabels {
            en: en.into(),
            ko: Some(ko.into()),
        },
        blurbs: ExtensionLabels {
            en: "A kind of thing".into(),
            ko: None,
        },
        fields: Vec::new(),
    }
}

fn add(state: &AppState, producer: Producer, type_name: &str, en: &str, ko: &str, at: i64) {
    crate::commands::assets::with_assets(state, "loupist-a", true, |tx| {
        crate::knowledge::add_kinds(
            tx,
            "loupist-a",
            vec![kind(type_name, en, ko)],
            Vec::new(),
            crate::knowledge::Provenance::new(producer, serde_json::json!({})),
            at,
        )
        .map(|_| ())
    })
    .unwrap();
}

#[test]
fn preset_and_review_changes_post_one_plain_language_card_and_chat_changes_do_not() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    seed_profile(
        state.store.as_ref(),
        &profile("loupist-a", &temporary.path().join("workspace"), 1),
    );
    add(
        &state,
        Producer::Preset,
        "ProcessNode",
        "Process node",
        "공정 노드",
        10,
    );
    add(&state, Producer::Chat, "Wafer", "Wafer", "웨이퍼", 20);
    add(&state, Producer::Review, "Fab", "Fab", "팹", 30);
    assert_eq!(deliver_finished_runs(&state, "loupist-a").unwrap(), 2);
    assert_eq!(
        deliver_finished_runs(&state, "loupist-a").unwrap(),
        0,
        "posted once"
    );
    let thread = state
        .store
        .get_chat(&home_thread_id("loupist-a"))
        .unwrap()
        .unwrap();
    let posts: Vec<_> = thread
        .messages
        .iter()
        .filter_map(|message| message.vocabulary.clone())
        .collect();
    assert_eq!(
        posts
            .iter()
            .map(|post| post.producer.as_str())
            .collect::<Vec<_>>(),
        ["preset", "review"]
    );
    assert_eq!(posts[0].labels, ["Process node"]);
    assert_eq!(posts[0].labels_ko, ["공정 노드"]);
    assert!(posts
        .iter()
        .all(|post| post.labels.iter().all(|label| label != "ProcessNode")));
}
