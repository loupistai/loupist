use super::*;
use crate::chat::{
    questions::{self, AnswerError, ChatQuestion, QuestionAnswer, QuestionState},
    surface::ChatSurface,
};

type QuestionCards = tokio::sync::mpsc::UnboundedReceiver<(Option<String>, ChatQuestion)>;

/// An interactive Bot turn whose question cards arrive on the returned channel.
fn asking_executor(
    temporary: &tempfile::TempDir,
    cancel: Option<tokio::sync::watch::Receiver<bool>>,
) -> (Arc<AppToolExecutor>, QuestionCards) {
    let (cards_tx, cards) = tokio::sync::mpsc::unbounded_channel();
    let state = seeded_state(temporary);
    let mut capture = ToolCapture {
        surface: Some(ChatSurface::Bot),
        question_sink: Some(Arc::new(move |call_id, question| {
            let _ = cards_tx.send((call_id, question));
        })),
        ..ToolCapture::default()
    };
    if let Some(cancel) = cancel {
        capture.search_cancel = crate::web::SearchCancel::from_watch(cancel);
    }
    (Arc::new(executor_for(&state, Arc::new(capture))), cards)
}

fn fork() -> Value {
    json!({
        "question": "Compare on which period?",
        "options": [
            {"label": "Last 4 quarters", "description": "Trailing twelve months"},
            {"label": "Last 3 years"},
        ],
        "recommended": 0,
    })
}

async fn ask(executor: &AppToolExecutor, params: Value) -> Result<Value, BrokerError> {
    executor
        .execute(&policy(false, false), ToolMethod::AskUser, params)
        .await
}

fn spawn_ask(
    executor: &Arc<AppToolExecutor>,
    params: Value,
) -> tokio::task::JoinHandle<Result<Value, BrokerError>> {
    let executor = executor.clone();
    tokio::spawn(async move { ask(&executor, params).await })
}

#[tokio::test]
async fn an_option_answer_resumes_the_waiting_call_and_settles_the_card() {
    let temporary = tempfile::tempdir().unwrap();
    let (executor, mut cards) = asking_executor(&temporary, None);
    let pending = spawn_ask(&executor, fork());

    let (_, waiting) = cards.recv().await.unwrap();
    assert_eq!(waiting.state, QuestionState::Waiting);
    assert_eq!(waiting.question, "Compare on which period?");
    assert_eq!(waiting.recommended, Some(0));
    assert!(!pending.is_finished(), "the call waits for the user");
    // Another Loupist cannot answer; neither can an answer outside the options.
    assert_eq!(
        questions::answer("loupist-b", &waiting.request_id, QuestionAnswer::Choice(0)),
        Err(AnswerError::NotWaiting)
    );
    assert!(matches!(
        questions::answer("loupist-a", &waiting.request_id, QuestionAnswer::Choice(5)),
        Err(AnswerError::Invalid(_))
    ));
    questions::answer("loupist-a", &waiting.request_id, QuestionAnswer::Choice(1)).unwrap();

    assert_eq!(
        pending.await.unwrap().unwrap(),
        json!({"answered": true, "choice": "Last 3 years"})
    );
    let (_, settled) = cards.recv().await.unwrap();
    assert_eq!(settled.request_id, waiting.request_id);
    assert_eq!(settled.state, QuestionState::Answered);
    assert_eq!(settled.answer, Some(QuestionAnswer::Choice(1)));
}

#[tokio::test]
async fn a_free_text_answer_reaches_the_model_as_the_users_words() {
    let temporary = tempfile::tempdir().unwrap();
    let (executor, mut cards) = asking_executor(&temporary, None);
    let pending = spawn_ask(&executor, fork());
    let (_, waiting) = cards.recv().await.unwrap();
    questions::answer(
        "loupist-a",
        &waiting.request_id,
        QuestionAnswer::Text("Since the 2023 listing".into()),
    )
    .unwrap();
    assert_eq!(
        pending.await.unwrap().unwrap(),
        json!({"answered": true, "text": "Since the 2023 listing"})
    );
    let (_, settled) = cards.recv().await.unwrap();
    assert_eq!(
        settled.answer,
        Some(QuestionAnswer::Text("Since the 2023 listing".into()))
    );
}

#[tokio::test]
async fn a_typed_answer_never_leaves_in_a_url_after_third_party_text() {
    use crate::custom_connections::taint::{WebFetchTarget, WEB_FETCH_PRIVATE_REFUSAL};
    let temporary = tempfile::tempdir().unwrap();
    let (executor, mut cards) = asking_executor(&temporary, None);
    let pending = spawn_ask(&executor, fork());
    let (_, waiting) = cards.recv().await.unwrap();
    let typed = "my app key is PSk3yQ9vT2mXw8Lr4Zp1Nd6";
    questions::answer(
        "loupist-a",
        &waiting.request_id,
        QuestionAnswer::Text(typed.into()),
    )
    .unwrap();
    pending.await.unwrap().unwrap();
    let taint = &executor.capture.turn_taint;
    taint.mark_untrusted_text();
    assert_eq!(
        taint.web_fetch_target(
            "https://collector.example/?k=my-app-key-is-PSk3yQ9vT2mXw8Lr4Zp1Nd6",
            &[]
        ),
        WebFetchTarget::Refused(WEB_FETCH_PRIVATE_REFUSAL)
    );
}

#[tokio::test(start_paused = true)]
async fn the_wait_running_out_continues_with_the_recommended_option() {
    let temporary = tempfile::tempdir().unwrap();
    let (executor, mut cards) = asking_executor(&temporary, None);
    let result = ask(&executor, fork()).await.unwrap();
    assert_eq!(result["answered"], false);
    assert_eq!(result["assumed"], "Last 4 quarters");
    let next = result["next"].as_str().unwrap();
    assert!(
        next.contains("did not answer")
            && next.contains("Last 4 quarters")
            && next.contains("which one you used"),
        "{next}"
    );
    let (_, waiting) = cards.recv().await.unwrap();
    assert_eq!(waiting.state, QuestionState::Waiting);
    let (_, settled) = cards.recv().await.unwrap();
    assert_eq!(settled.state, QuestionState::Expired);
    assert_eq!(settled.answer, None);
    // The expired question can no longer be answered.
    assert_eq!(
        questions::answer("loupist-a", &settled.request_id, QuestionAnswer::Choice(0)),
        Err(AnswerError::NotWaiting)
    );
}

#[tokio::test]
async fn stopping_the_run_ends_the_wait_without_an_error_and_assumes_nothing_without_a_recommendation(
) {
    let temporary = tempfile::tempdir().unwrap();
    let (stop, cancel) = tokio::sync::watch::channel(false);
    let (executor, mut cards) = asking_executor(&temporary, Some(cancel));
    let mut open_ended = fork();
    open_ended.as_object_mut().unwrap().remove("recommended");
    let pending = spawn_ask(&executor, open_ended);
    let (_, waiting) = cards.recv().await.unwrap();
    assert_eq!(waiting.recommended, None);
    stop.send(true).unwrap();
    let result = pending.await.unwrap().unwrap();
    assert_eq!(result["answered"], false);
    assert_eq!(result["assumed"], Value::Null);
    assert!(result["next"]
        .as_str()
        .unwrap()
        .contains("most reasonable assumption"));
    let (_, settled) = cards.recv().await.unwrap();
    assert_eq!(settled.state, QuestionState::Expired);
}

#[tokio::test]
async fn a_run_without_cards_reports_no_answer_instead_of_waiting() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let capture = ToolCapture {
        surface: Some(ChatSurface::Session),
        ..ToolCapture::default()
    };
    let executor = executor_for(&state, Arc::new(capture));
    let result = ask(&executor, fork()).await.unwrap();
    assert_eq!(result["answered"], false);
    assert_eq!(result["assumed"], "Last 4 quarters");
}

#[tokio::test]
async fn unattended_and_host_turns_refuse_the_tool() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    for surface in [
        Some(ChatSurface::Scheduled),
        Some(ChatSurface::Delegated {
            handoff_id: "handoff-a".into(),
        }),
        None,
    ] {
        let (cards_tx, mut cards) = tokio::sync::mpsc::unbounded_channel::<ChatQuestion>();
        let capture = ToolCapture {
            surface: surface.clone(),
            question_sink: Some(Arc::new(move |_, question| {
                let _ = cards_tx.send(question);
            })),
            ..ToolCapture::default()
        };
        let executor = executor_for(&state, Arc::new(capture));
        let error = ask(&executor, fork()).await.unwrap_err();
        assert!(
            matches!(&error, BrokerError::Execution(message) if message.contains("ask_user is unavailable")),
            "{surface:?}: {error}"
        );
        assert!(cards.try_recv().is_err(), "no card is shown");
    }
    // The scheduled allowlist does not grant the tool either.
    assert!(!crate::briefings::policy::allowed_method(
        ToolMethod::AskUser
    ));
}

#[tokio::test]
async fn a_malformed_question_is_refused_naming_the_field_and_shows_no_card() {
    let temporary = tempfile::tempdir().unwrap();
    let (executor, mut cards) = asking_executor(&temporary, None);
    let with = |edit: fn(&mut Value)| {
        let mut params = fork();
        edit(&mut params);
        params
    };
    let cases: [(Value, &str); 8] = [
        (with(|p| p["question"] = json!("")), "question"),
        (with(|p| p["question"] = json!("q".repeat(301))), "question"),
        (
            with(|p| p["options"] = json!([{"label": "Only one"}])),
            "options",
        ),
        (
            with(
                |p| p["options"] = json!([{"label": "A"}, {"label": "B"}, {"label": "C"}, {"label": "D"}, {"label": "E"}]),
            ),
            "options",
        ),
        (
            with(|p| p["options"] = json!([{"label": "Same"}, {"label": "same"}])),
            "options[1].label",
        ),
        (
            with(|p| p["options"][1]["label"] = json!("l".repeat(61))),
            "options[1].label",
        ),
        (
            with(|p| p["options"][0]["description"] = json!("d".repeat(161))),
            "options[0].description",
        ),
        (with(|p| p["recommended"] = json!(2)), "recommended"),
    ];
    for (params, field) in cases {
        match ask(&executor, params.clone()).await {
            Err(BrokerError::InvalidArguments { field: named, .. }) => {
                assert_eq!(named, field, "{params}");
            }
            other => panic!("{params}: {other:?}"),
        }
    }
    for params in [
        json!({}),
        json!({"question": "Which?"}),
        with(|p| p["extra"] = json!(1)),
        with(|p| p["recommended"] = json!(-1)),
    ] {
        assert!(
            matches!(
                ask(&executor, params.clone()).await,
                Err(BrokerError::InvalidArguments { .. })
            ),
            "{params}"
        );
    }
    assert!(
        cards.try_recv().is_err(),
        "a refused question shows no card"
    );
}
