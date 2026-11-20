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
fn only_review_changes_post_one_plain_language_card() {
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
    assert_eq!(deliver_finished_runs(&state, "loupist-a").unwrap(), 1);
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
        ["review"]
    );
    assert_eq!(posts[0].labels, ["Fab"]);
    assert_eq!(posts[0].labels_ko, ["팹"]);
    assert!(posts
        .iter()
        .all(|post| post.labels.iter().all(|label| label != "ProcessNode")));
}

#[test]
fn a_vocabulary_card_hidden_by_an_edit_is_already_posted() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    seed_profile(
        state.store.as_ref(),
        &profile("loupist-a", &temporary.path().join("workspace"), 1),
    );
    add(
        &state,
        Producer::Review,
        "ProcessNode",
        "Process node",
        "공정 노드",
        10,
    );
    assert_eq!(deliver_finished_runs(&state, "loupist-a").unwrap(), 1);
    let before = state
        .store
        .get_chat(&home_thread_id("loupist-a"))
        .unwrap()
        .unwrap();
    let mut edited = before.clone();
    let mut messages = vec![crate::test_fixtures::chat_message(
        "hidden-user",
        ChatRole::User,
        "Earlier question",
    )];
    messages.append(&mut edited.messages);
    edited.versions.push(crate::domain::ChatVersion {
        id: "hidden-vocabulary".into(),
        after_message_id: None,
        messages,
        archived_at_ms: before.updated_at_ms,
    });
    state.store.replace_chat(&before, &edited).unwrap();
    assert_eq!(deliver_finished_runs(&state, "loupist-a").unwrap(), 0);
}

#[tokio::test]
async fn a_template_creation_keeps_its_preset_in_settings_without_a_bot_post() {
    let temporary = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temporary.path().join("app"));
    let loupist = crate::commands::loupist::create_managed_loupist(
        &state,
        LoupistCreateRequest {
            name: "Semiconductors".into(),
            focus: None,
            avatar: None,
            template: Some("semiconductors".into()),
            tone: None,
        },
    )
    .await
    .unwrap();
    fs::create_dir_all(loupist_config_directory(&state, &loupist.id).unwrap()).unwrap();
    assert_eq!(deliver_finished_runs(&state, &loupist.id).unwrap(), 0);
    let bot = state
        .store
        .get_chat(&home_thread_id(&loupist.id))
        .unwrap()
        .unwrap();
    assert!(bot
        .messages
        .iter()
        .all(|message| message.vocabulary.is_none()));
    let changes = crate::commands::assets::with_assets(&state, &loupist.id, false, |tx| {
        crate::knowledge::recent_changes(tx, &loupist.id, 20)
    })
    .unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].producer, Producer::Preset);
    assert!(changes[0].origin.is_none());
    assert!(!crate::knowledge::added_labels(&changes[0]).is_empty());
}
