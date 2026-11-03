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

pub(crate) async fn run<E>(
    launch: PiSupportLaunchConfig,
    command: &str,
    budget: Duration,
    cancel: impl Future<Output = ()>,
    mut on_event: impl FnMut(&Value) -> Result<(), E>,
    mut ready: impl FnMut() -> Result<bool, E>,
    answers: Option<tokio::sync::mpsc::UnboundedReceiver<UiAnswer>>,
) -> SupportOutcome<E> {
    tokio::pin!(cancel);
    if cancel.as_mut().now_or_never().is_some() {
        return SupportOutcome {
            result: Err(SupportFailure::CancelledBeforeDispatch),
            shutdown: Ok(()),
        };
    }
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
                // A sign-in's pasted address or code reaches Pi's waiting prompt
                // while this command runs; Rust validated each value first.
                let forward = async {
                    if let Some(mut answers) = answers {
                        while let Some((request_id, value)) = answers.recv().await {
                            let _ = process.respond_extension_ui(&request_id, &value).await;
                        }
                    }
                    std::future::pending::<()>().await
                };
                tokio::select! {
                    result = timeout(
                        budget,
                        wait_for_result(
                            &mut receiver,
                            id,
                            cancel.as_mut(),
                            &mut on_event,
                            &mut ready,
                        ),
                    ) => result.unwrap_or(Err(SupportFailure::Timeout)),
                    _ = forward => Err(SupportFailure::Stopped),
                }
            }
        }
    };
    let shutdown = process.shutdown(Duration::from_secs(1)).await;
    SupportOutcome { result, shutdown }
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
    let mut poll = interval(Duration::from_millis(10));
    poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = &mut cancel => return Err(SupportFailure::Cancelled),
            event = receiver.recv() => match event {
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
                    }
                }
                Ok(PiEvent::Exited) if acknowledged && ready().map_err(SupportFailure::Operation)? => return Ok(()),
                _ => return Err(SupportFailure::Stopped),
            },
            _ = poll.tick() => {
                if acknowledged && ready().map_err(SupportFailure::Operation)? { return Ok(()); }
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
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    fn ack(id: u64, success: bool) -> PiEvent {
        PiEvent::Rpc {
            payload: json!({"type":"response","id":id,"success":success}),
        }
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
