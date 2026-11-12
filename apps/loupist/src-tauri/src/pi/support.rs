//! One lifecycle for private Pi support commands. Admission and response schemas
//! belong to callers; every started process is drained even on cancellation.
use crate::pi::{PiError, PiEvent, PiProcess, PiSupportLaunchConfig};
use futures_util::FutureExt;
use serde_json::Value;
use std::{future::Future, path::Path, time::Duration};
use tokio::{
    sync::broadcast,
    time::{interval, timeout, MissedTickBehavior},
};

#[derive(Debug)]
pub(crate) enum SupportFailure<E> {
    Launch(PiError),
    Dispatch(PiError),
    CancelledBeforeDispatch,
    Cancelled,
    Timeout,
    Stopped,
    Rejected,
    ExtensionFailed,
    Operation(E),
}

pub(crate) struct SupportOutcome<E> {
    pub result: Result<(), SupportFailure<E>>,
    pub shutdown: Result<(), PiError>,
}

/// One answer to an extension UI request: its id and the validated value.
pub(crate) type UiAnswer = (String, String);

const API_KEY_TITLE: &str = "loupist-provider/1:api_key";

/// A support mutation may answer exactly one, specifically titled input.
/// No secret is copied into the launch environment or forwarded as an event.
struct ApiKeyAnswer(Option<String>);

impl ApiKeyAnswer {
    fn for_prompt(&mut self, payload: &Value) -> Option<UiAnswer> {
        if payload.get("type").and_then(Value::as_str) != Some("extension_ui_request")
            || payload.get("method").and_then(Value::as_str) != Some("input")
            || payload.get("title").and_then(Value::as_str) != Some(API_KEY_TITLE)
        {
            return None;
        }
        let id = payload.get("id")?.as_str()?;
        if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
            return None;
        }
        self.0.take().map(|value| (id.to_owned(), value))
    }
}

pub(crate) async fn run<E>(
    launch: PiSupportLaunchConfig,
    command: &str,
    budget: Duration,
    cancel: impl Future<Output = ()>,
    on_event: impl FnMut(&Value) -> Result<(), E>,
    ready: impl FnMut() -> Result<bool, E>,
    answers: Option<tokio::sync::mpsc::UnboundedReceiver<UiAnswer>>,
) -> SupportOutcome<E> {
    tokio::pin!(cancel);
    if cancel.as_mut().now_or_never().is_some() {
        return SupportOutcome {
            result: Err(SupportFailure::CancelledBeforeDispatch),
            shutdown: Ok(()),
        };
    }
    // Retain the mutation only in this lifecycle; spawn_support validates it,
    // and the launch environment deliberately never includes it.
    let api_key = launch.mutation_api_key.clone();
    let process = match PiProcess::spawn_support(launch).await {
        Ok(process) => process,
        Err(error) => {
            return SupportOutcome {
                result: Err(SupportFailure::Launch(error)),
                shutdown: Ok(()),
            }
        }
    };
    let mut receiver = process.subscribe();
    let result = if cancel.as_mut().now_or_never().is_some() {
        Err(SupportFailure::CancelledBeforeDispatch)
    } else {
        // A failed/partially written prompt is uncertain. Do not cancel this
        // bounded write and then misreport it as an unsent request.
        match process.prompt(command).await {
            Err(error) => Err(SupportFailure::Dispatch(error)),
            Ok(id) => {
                let process = &process;
                timeout(
                    budget,
                    wait_with_answers(
                        &mut receiver,
                        id,
                        cancel.as_mut(),
                        on_event,
                        ready,
                        api_key,
                        answers,
                        |(request_id, value)| async move {
                            process.respond_extension_ui(&request_id, &value).await
                        },
                    ),
                )
                .await
                .unwrap_or(Err(SupportFailure::Timeout))
            }
        }
    };
    let shutdown = process.shutdown(Duration::from_secs(1)).await;
    SupportOutcome { result, shutdown }
}

#[allow(clippy::too_many_arguments)]
async fn wait_with_answers<E, F: Future<Output = Result<(), PiError>>>(
    receiver: &mut broadcast::Receiver<PiEvent>,
    request_id: u64,
    cancel: impl Future<Output = ()>,
    mut on_event: impl FnMut(&Value) -> Result<(), E>,
    mut ready: impl FnMut() -> Result<bool, E>,
    api_key: Option<String>,
    mut answers: Option<tokio::sync::mpsc::UnboundedReceiver<UiAnswer>>,
    mut respond: impl FnMut(UiAnswer) -> F,
) -> Result<(), SupportFailure<E>> {
    let mut api_key = ApiKeyAnswer(api_key);
    let (private_tx, mut private_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut event = |payload: &Value| {
        if let Some(answer) = api_key.for_prompt(payload) {
            let _ = private_tx.send(answer);
            Ok(())
        } else {
            on_event(payload)
        }
    };
    let forward = async {
        loop {
            let answer = tokio::select! {
                Some(answer) = private_rx.recv() => answer,
                Some(answer) = async {
                    match answers.as_mut() {
                        Some(receiver) => receiver.recv().await,
                        None => std::future::pending().await,
                    }
                } => answer,
            };
            respond(answer).await.map_err(SupportFailure::Dispatch)?;
        }
    };
    // Completion/cancellation wins over a queued answer. Dropping this scope
    // also drops the unused key and any answers on timeout or cancellation.
    tokio::select! {
        biased;
        result = wait_for_result(receiver, request_id, cancel, &mut event, &mut ready) => result,
        result = forward => result,
    }
}

async fn wait_for_result<E>(
    receiver: &mut broadcast::Receiver<PiEvent>,
    request_id: u64,
    cancel: impl Future<Output = ()>,
    on_event: &mut impl FnMut(&Value) -> Result<(), E>,
    ready: &mut impl FnMut() -> Result<bool, E>,
) -> Result<(), SupportFailure<E>> {
    tokio::pin!(cancel);
    let mut acknowledged = false;
    let mut exited = false;
    let mut poll = interval(Duration::from_millis(100));
    poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = &mut cancel => return Err(SupportFailure::Cancelled),
            event = receiver.recv(), if !exited => match event {
                Ok(PiEvent::Rpc { payload }) => {
                    // Never forward extension errors or provider payloads.
                    if payload.get("type").and_then(Value::as_str) == Some("extension_error") {
                        return Err(SupportFailure::ExtensionFailed);
                    }
                    on_event(&payload).map_err(SupportFailure::Operation)?;
                    if payload.get("type").and_then(Value::as_str) == Some("response")
                        && payload.get("id").and_then(Value::as_u64) == Some(request_id) {
                        if payload.get("success").and_then(Value::as_bool) != Some(true) {
                            return Err(SupportFailure::Rejected);
                        }
                        // Slash-command acknowledgement can precede the async
                        // handler. Wait for its completed result before shutdown.
                        acknowledged = true;
                        if ready().map_err(SupportFailure::Operation)? {
                            return Ok(());
                        }
                    }
                }
                Ok(PiEvent::Exited) if acknowledged => exited = true,
                _ => return Err(SupportFailure::Stopped),
            },
            _ = poll.tick() => {
                if acknowledged && ready().map_err(SupportFailure::Operation)? { return Ok(()); }
                if exited { return Err(SupportFailure::Stopped); }
            }
        }
    }
}

/// Observe a bounded, private completed envelope; callers validate its exact
/// schema after shutdown. An inference dispatch marker is not a final result.
pub(crate) fn result_ready(path: &Path, kind: Option<&str>) -> Result<bool, ()> {
    let bytes = crate::fs::artifact_trust::read_private_regular_file_bounded(path, 512 * 1024)
        .map_err(|_| ())?;
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(false);
    };
    Ok(
        value.get("protocol").and_then(Value::as_str) == Some("loupist-provider/1")
            && value
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|found| kind.is_none_or(|expected| found == expected))
            && value.pointer("/body/status").and_then(Value::as_str) != Some("in_flight"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };

    fn ack(id: u64, success: bool) -> PiEvent {
        PiEvent::Rpc {
            payload: json!({"type":"response","id":id,"success":success}),
        }
    }

    fn key_prompt(title: &str) -> Value {
        json!({"type":"extension_ui_request", "method":"input", "title":title, "id":"key-prompt"})
    }

    #[test]
    fn an_api_key_answers_only_the_expected_valid_prompt_once() {
        let expected = key_prompt(API_KEY_TITLE);
        let mut key = ApiKeyAnswer(Some("private-key-fixture".into()));
        for payload in [
            key_prompt("loupist-provider/1:manual_code"),
            key_prompt("API key"),
            json!({"type":"extension_ui_request", "method":"confirm", "title":API_KEY_TITLE, "id":"prompt"}),
            json!({"type":"response", "method":"input", "title":API_KEY_TITLE, "id":"prompt"}),
            json!({"type":"extension_ui_request", "method":"input", "title":API_KEY_TITLE, "id":""}),
            json!({"type":"extension_ui_request", "method":"input", "title":API_KEY_TITLE, "id":"bad\n"}),
            json!({"type":"extension_ui_request", "method":"input", "title":API_KEY_TITLE, "id":"x".repeat(129)}),
        ] {
            assert!(key.for_prompt(&payload).is_none());
        }
        assert_eq!(
            key.for_prompt(&expected),
            Some(("key-prompt".into(), "private-key-fixture".into()))
        );
        assert!(key.for_prompt(&expected).is_none());
        assert!(key.0.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn support_keys_and_manual_answers_use_the_same_private_answer_path() {
        let (tx, mut rx) = broadcast::channel(8);
        let (answers, inbox) = tokio::sync::mpsc::unbounded_channel();
        let sent = std::sync::Mutex::new(Vec::new());
        let result = wait_with_answers(
            &mut rx,
            7,
            std::future::pending(),
            |_| Ok::<_, ()>(()),
            || Ok(false),
            Some("private-key-fixture".into()),
            Some(inbox),
            |answer| {
                sent.lock().unwrap().push(answer);
                async { Ok(()) }
            },
        );
        tokio::pin!(result);
        assert!(futures_util::poll!(&mut result).is_pending());
        tx.send(PiEvent::Rpc {
            payload: key_prompt("unexpected"),
        })
        .unwrap();
        assert!(futures_util::poll!(&mut result).is_pending());
        assert!(sent.lock().unwrap().is_empty());
        tx.send(PiEvent::Rpc {
            payload: key_prompt(API_KEY_TITLE),
        })
        .unwrap();
        assert!(futures_util::poll!(&mut result).is_pending());
        assert_eq!(
            *sent.lock().unwrap(),
            [("key-prompt".into(), "private-key-fixture".into())]
        );
        tx.send(PiEvent::Rpc {
            payload: key_prompt(API_KEY_TITLE),
        })
        .unwrap();
        answers
            .send(("manual-prompt".into(), "validated-code".into()))
            .unwrap();
        assert!(futures_util::poll!(&mut result).is_pending());
        assert_eq!(sent.lock().unwrap().len(), 2);
        assert_eq!(
            sent.lock().unwrap()[1],
            ("manual-prompt".into(), "validated-code".into())
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_and_timeout_discard_pending_keys_and_queued_answers() {
        for cancel_now in [true, false] {
            let (tx, mut rx) = broadcast::channel(8);
            let (cancel, cancelled) = tokio::sync::oneshot::channel();
            let sent = std::sync::Mutex::new(Vec::new());
            let result = timeout(
                Duration::from_secs(1),
                wait_with_answers(
                    &mut rx,
                    7,
                    async {
                        let _ = cancelled.await;
                    },
                    |_| Ok::<_, ()>(()),
                    || Ok(false),
                    Some("private-key-fixture".into()),
                    None,
                    |answer| {
                        sent.lock().unwrap().push(answer);
                        async { Ok(()) }
                    },
                ),
            );
            tokio::pin!(result);
            assert!(futures_util::poll!(&mut result).is_pending());
            if cancel_now {
                tx.send(PiEvent::Rpc {
                    payload: key_prompt(API_KEY_TITLE),
                })
                .unwrap();
                cancel.send(()).unwrap();
                assert!(matches!(result.await, Ok(Err(SupportFailure::Cancelled))));
            } else {
                tokio::time::advance(Duration::from_secs(1)).await;
                assert!(result.await.is_err());
                drop(cancel);
            }
            tx.send(PiEvent::Rpc {
                payload: key_prompt(API_KEY_TITLE),
            })
            .unwrap();
            assert!(sent.lock().unwrap().is_empty());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn pending_support_results_are_polled_at_most_every_hundred_milliseconds() {
        let (tx, mut rx) = broadcast::channel(4);
        tx.send(ack(7, true)).unwrap();
        let checks = AtomicUsize::new(0);
        let ready = AtomicBool::new(false);
        let mut on_event = |_: &Value| Ok::<_, ()>(());
        let mut check = || {
            checks.fetch_add(1, Ordering::SeqCst);
            Ok(ready.load(Ordering::SeqCst))
        };
        let result = wait_for_result(
            &mut rx,
            7,
            std::future::pending(),
            &mut on_event,
            &mut check,
        );
        tokio::pin!(result);
        assert!(futures_util::poll!(&mut result).is_pending());
        let initial = checks.load(Ordering::SeqCst);
        for event in [
            PiEvent::Rpc {
                payload: json!({"type":"extension_ui_request"}),
            },
            PiEvent::Rpc {
                payload: json!({"type":"message_update"}),
            },
            ack(99, true),
        ] {
            tx.send(event).unwrap();
            assert!(futures_util::poll!(&mut result).is_pending());
        }
        assert_eq!(checks.load(Ordering::SeqCst), initial);
        tokio::time::advance(Duration::from_millis(90)).await;
        assert!(futures_util::poll!(&mut result).is_pending());
        assert_eq!(checks.load(Ordering::SeqCst), initial);
        ready.store(true, Ordering::SeqCst);
        tx.send(PiEvent::Exited).unwrap();
        assert!(futures_util::poll!(&mut result).is_pending());
        assert_eq!(checks.load(Ordering::SeqCst), initial);
        tokio::time::advance(Duration::from_millis(10)).await;
        result.await.unwrap();
        assert_eq!(checks.load(Ordering::SeqCst), initial + 1);
    }

    #[tokio::test]
    async fn acknowledgement_waits_for_completed_result_and_handles_oauth_events() {
        let (tx, mut rx) = broadcast::channel(8);
        let ready = Arc::new(AtomicBool::new(false));
        let writer = ready.clone();
        let task = tokio::spawn(async move {
            tx.send(ack(99, true)).unwrap();
            tx.send(PiEvent::Rpc {
                payload: json!({"type":"extension_ui_request"}),
            })
            .unwrap();
            tx.send(ack(7, true)).unwrap();
            tokio::time::sleep(Duration::from_millis(25)).await;
            writer.store(true, Ordering::SeqCst);
            tx.send(PiEvent::Exited).unwrap();
        });
        let mut seen = Vec::new();
        timeout(
            Duration::from_secs(1),
            wait_for_result(
                &mut rx,
                7,
                std::future::pending(),
                &mut |value: &Value| {
                    seen.push(value.clone());
                    Ok::<_, ()>(())
                },
                &mut || Ok(ready.load(Ordering::SeqCst)),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(seen
            .iter()
            .any(|value| value["type"] == "extension_ui_request"));
        assert!(ready.load(Ordering::SeqCst));
        task.await.unwrap();
    }

    #[tokio::test]
    async fn cancellation_rejection_and_extension_errors_end_wait_without_exposing_payloads() {
        for (event, expected) in [
            (ack(7, false), "rejected"),
            (
                PiEvent::Rpc {
                    payload: json!({"type":"extension_error","error":"private-token"}),
                },
                "extension",
            ),
        ] {
            let (tx, mut rx) = broadcast::channel(4);
            tx.send(event).unwrap();
            let error = wait_for_result(
                &mut rx,
                7,
                std::future::pending(),
                &mut |_| Ok::<_, ()>(()),
                &mut || Ok(false),
            )
            .await
            .unwrap_err();
            assert!(!format!("{error:?}").contains("private-token"));
            assert!(matches!(
                (expected, error),
                ("rejected", SupportFailure::Rejected)
                    | ("extension", SupportFailure::ExtensionFailed)
            ));
        }
        let (_tx, mut rx) = broadcast::channel(4);
        assert!(matches!(
            wait_for_result(&mut rx, 7, async {}, &mut |_| Ok::<_, ()>(()), &mut || Ok(
                false
            ))
            .await,
            Err(SupportFailure::Cancelled)
        ));
        assert!(timeout(
            Duration::from_millis(20),
            wait_for_result(
                &mut rx,
                7,
                std::future::pending(),
                &mut |_| Ok::<_, ()>(()),
                &mut || Ok(false)
            )
        )
        .await
        .is_err());
    }

    #[test]
    fn inference_dispatch_marker_is_not_a_completed_result() {
        let root = tempfile::tempdir().unwrap();
        let result = root.path().join("result.json");
        crate::fs::artifact_trust::ensure_private_regular_file(&result).unwrap();
        assert!(!result_ready(&result, Some("inference")).unwrap());
        for (status, expected) in [("in_flight", false), ("completed", true)] {
            std::fs::write(&result, json!({"protocol":"loupist-provider/1","type":"inference","body":{"status":status}}).to_string()).unwrap();
            assert_eq!(result_ready(&result, Some("inference")).unwrap(), expected);
        }
    }
}
