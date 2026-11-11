//! Provider-wait watchdogs for one Chat turn: first provider body progress,
//! tool silence and compaction budgets.

use super::*;

/// The prompt response and message lifecycle only confirm that Pi accepted
/// the turn. The first-progress watchdog must instead observe a body event
/// emitted by the provider stream.
pub(super) fn pi_event_indicates_first_provider_body_progress(
    event: &Result<PiEvent, tokio::sync::broadcast::error::RecvError>,
) -> bool {
    match event {
        Ok(PiEvent::Rpc { payload }) => match payload.get("type").and_then(Value::as_str) {
            Some("message_update") => matches!(
                payload
                    .get("assistantMessageEvent")
                    .and_then(|event| event.get("type"))
                    .and_then(Value::as_str),
                Some(
                    "thinking_start"
                        | "thinking_delta"
                        | "thinking_end"
                        | "text_start"
                        | "text_delta"
                        | "text_end"
                        | "toolcall_start"
                        | "toolcall_delta"
                        | "toolcall_end"
                )
            ),
            Some("message_end") => {
                payload
                    .get("message")
                    .and_then(|message| message.get("role"))
                    .and_then(Value::as_str)
                    == Some("assistant")
            }
            _ => false,
        },
        Ok(PiEvent::ProtocolError { .. } | PiEvent::Exited)
        | Err(
            tokio::sync::broadcast::error::RecvError::Lagged(_)
            | tokio::sync::broadcast::error::RecvError::Closed,
        ) => false,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum CompactionOutcome {
    Succeeded,
    Cancelled,
    Failed,
}

/// Pi has no `success` field. Cancellation without an error or result also
/// represents auxiliary compaction deferred by the host extension.
pub(super) fn compaction_outcome(payload: &Value) -> CompactionOutcome {
    let has_result = payload.get("result").is_some_and(Value::is_object);
    if payload
        .get("errorMessage")
        .and_then(Value::as_str)
        .is_some_and(|message| !message.is_empty())
    {
        CompactionOutcome::Failed
    } else if payload.get("aborted").and_then(Value::as_bool) == Some(true) {
        if has_result {
            CompactionOutcome::Failed
        } else {
            CompactionOutcome::Cancelled
        }
    } else if has_result {
        CompactionOutcome::Succeeded
    } else {
        CompactionOutcome::Failed
    }
}

// Any post-prompt Pi event arrives before provider streaming can make visible
// assistant progress. Its absence therefore indicates a broken Pi/RPC path,
// not normal model thinking. Never replay the prompt here: a hidden tool call
// could otherwise be duplicated.
pub(super) const CHAT_FIRST_PROVIDER_BODY_PROGRESS_TIMEOUT: Duration = Duration::from_secs(60);
// Match Pi's `httpIdleTimeoutMs: 300000` transport limit. This caps the whole
// pre-body phase: compaction is Pi system work, not provider body progress,
// but neither an unended compaction nor serial compactions may hold a run
// forever.
pub(super) const CHAT_PRE_BODY_TOTAL_TIMEOUT: Duration = crate::pi::PI_HTTP_IDLE_TIMEOUT;
// This is a SILENCE budget for the tool protocol, not a job duration limit.
// Individual host tools enforce their own deadlines.
// An hour exceeds World graph_jobs::LOCAL_OPERATION_BUDGET_MS (600 s) and
// tool_executor/mcp_host.rs MCP_CALL_TIMEOUT (45 s). Every Pi event rearms it.
pub(super) const CHAT_TOOL_SILENCE_TIMEOUT: Duration = Duration::from_secs(60 * 60);
pub(super) const CHAT_COMPACTION_TOTAL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
pub(super) const CHAT_TOOL_SILENCE_FAILURE: &str =
    "Pi tool produced nothing for an hour; retry or run it outside Chat";
pub(super) const CHAT_COMPACTION_TIMEOUT_FAILURE: &str =
    "Pi did not finish compaction within 15 minutes";
// At an expired watchdog boundary, Pi can have a short protocol prelude queued
// before `compaction_start`. Give that queued work a fixed, small chance to be
// observed without letting a stream of unrelated events starve the timeout.
pub(super) const CHAT_WATCHDOG_EXPIRY_EVENT_DEFERRAL_LIMIT: usize = 8;

pub(super) fn watchdog_expiry_event_deferral_allowed(expiry_event_deferrals: usize) -> bool {
    expiry_event_deferrals < CHAT_WATCHDOG_EXPIRY_EVENT_DEFERRAL_LIMIT
}

pub(super) type PiEventReceive = Result<PiEvent, tokio::sync::broadcast::error::RecvError>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FirstProviderBodyWatchdogSelectMode {
    Fair,
    PrioritizeQueuedPiEvent,
    Timeout,
}

pub(super) enum ChatLoopInput {
    WatchdogElapsed,
    CommentaryFlush,
    CancelChanged(Result<(), tokio::sync::watch::error::RecvError>),
    Control(Option<ChatControlRequest>),
    /// An approval or question card changed: (tool call id, card), or None once the sender is gone.
    Card(Option<(Option<String>, super::launch::CardUpdate)>),
    PiEvent(PiEventReceive),
}

pub(super) async fn receive_expired_watchdog_input(
    mut first_provider_body_progress_timer: Pin<&mut tokio::time::Sleep>,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
    chat_controls: &mut ChatControlReceiver,
    pi_events: &mut tokio::sync::broadcast::Receiver<PiEvent>,
    watchdog_expiry_event_deferrals: &mut usize,
) -> ChatLoopInput {
    // Only at an expired watchdog boundary, Pi may already have queued
    // `compaction_start`. Prefer that bounded protocol work before failing;
    // normal streaming stays on the fair select in the chat loop.
    tokio::select! {
        biased;
        changed = cancel.changed() => ChatLoopInput::CancelChanged(changed),
        event = pi_events.recv() => {
            *watchdog_expiry_event_deferrals += 1;
            ChatLoopInput::PiEvent(event)
        },
        _ = first_provider_body_progress_timer.as_mut() => ChatLoopInput::WatchdogElapsed,
        control = chat_controls.recv() => ChatLoopInput::Control(control),
    }
}

pub(super) fn take_queued_pi_event_after_watchdog_expiry(
    pi_events: &mut tokio::sync::broadcast::Receiver<PiEvent>,
    expiry_event_deferrals: &mut usize,
) -> Option<PiEventReceive> {
    if !watchdog_expiry_event_deferral_allowed(*expiry_event_deferrals) {
        return None;
    }
    let event = match pi_events.try_recv() {
        Ok(event) => Some(Ok(event)),
        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => Some(Err(
            tokio::sync::broadcast::error::RecvError::Lagged(skipped),
        )),
        Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
            Some(Err(tokio::sync::broadcast::error::RecvError::Closed))
        }
        Err(tokio::sync::broadcast::error::TryRecvError::Empty) => None,
    };
    if event.is_some() {
        *expiry_event_deferrals += 1;
    }
    event
}

#[derive(Clone, Copy, Debug)]
pub(super) enum FirstProviderBodyWatchdog {
    AwaitingProviderBody {
        body_deadline: Instant,
        pre_body_deadline: Instant,
    },
    Compacting {
        pre_body_deadline: Instant,
    },
    Satisfied,
}

impl FirstProviderBodyWatchdog {
    pub(super) fn new(now: Instant) -> Self {
        let pre_body_deadline = now + CHAT_PRE_BODY_TOTAL_TIMEOUT;
        Self::AwaitingProviderBody {
            body_deadline: now + CHAT_FIRST_PROVIDER_BODY_PROGRESS_TIMEOUT,
            pre_body_deadline,
        }
    }

    pub(super) fn deadline(self) -> Option<Instant> {
        match self {
            Self::AwaitingProviderBody {
                body_deadline,
                pre_body_deadline,
            } => Some(body_deadline.min(pre_body_deadline)),
            Self::Compacting { pre_body_deadline } => Some(pre_body_deadline),
            Self::Satisfied => None,
        }
    }

    pub(super) fn is_active(self) -> bool {
        self.deadline().is_some()
    }

    pub(super) fn is_expired(self, now: Instant) -> bool {
        self.deadline().is_some_and(|deadline| now >= deadline)
    }

    pub(super) fn select_mode(
        self,
        now: Instant,
        expiry_event_deferrals: usize,
    ) -> FirstProviderBodyWatchdogSelectMode {
        if !self.is_expired(now) {
            FirstProviderBodyWatchdogSelectMode::Fair
        } else if watchdog_expiry_event_deferral_allowed(expiry_event_deferrals) {
            FirstProviderBodyWatchdogSelectMode::PrioritizeQueuedPiEvent
        } else {
            FirstProviderBodyWatchdogSelectMode::Timeout
        }
    }

    pub(super) fn timeout_failure(self) -> &'static str {
        match self {
            Self::Compacting { .. } => "Pi did not finish pre-body compaction",
            Self::AwaitingProviderBody { .. } | Self::Satisfied => {
                "Pi did not emit provider body progress"
            }
        }
    }

    pub(super) fn observe_provider_body_progress(&mut self) {
        *self = Self::Satisfied;
    }

    pub(super) fn exclude_tool_time(&mut self, elapsed: Duration) {
        match self {
            Self::AwaitingProviderBody {
                body_deadline,
                pre_body_deadline,
            } => {
                *body_deadline += elapsed;
                *pre_body_deadline += elapsed;
            }
            Self::Compacting { pre_body_deadline } => *pre_body_deadline += elapsed,
            Self::Satisfied => {}
        }
    }

    /// Returns a replacement deadline only for a compaction that begins
    /// before any provider body progress. Duplicate starts deliberately keep
    /// the original compaction deadline bounded.
    pub(super) fn observe_compaction_start(&mut self) -> Option<Instant> {
        match self {
            Self::AwaitingProviderBody {
                pre_body_deadline, ..
            } => {
                let deadline = *pre_body_deadline;
                *self = Self::Compacting {
                    pre_body_deadline: deadline,
                };
                Some(deadline)
            }
            Self::Compacting { .. } | Self::Satisfied => None,
        }
    }

    /// Returns a fresh provider-body deadline only when ending the active
    /// pre-body compaction. An unmatched end event must not indefinitely reset
    /// the watchdog.
    pub(super) fn observe_compaction_end(&mut self, now: Instant) -> Option<Instant> {
        match self {
            Self::Compacting { pre_body_deadline } => {
                let pre_body_deadline = *pre_body_deadline;
                let deadline =
                    (now + CHAT_FIRST_PROVIDER_BODY_PROGRESS_TIMEOUT).min(pre_body_deadline);
                *self = Self::AwaitingProviderBody {
                    body_deadline: now + CHAT_FIRST_PROVIDER_BODY_PROGRESS_TIMEOUT,
                    pre_body_deadline,
                };
                Some(deadline)
            }
            Self::AwaitingProviderBody { .. } | Self::Satisfied => None,
        }
    }
}

// One timer covers first-body/provider waits, tool silence and compaction.
// No start event can disarm it, even if the matching end or call id is missing.
pub(super) struct ProviderWaitWatchdog {
    first_body: FirstProviderBodyWatchdog,
    idle_timeout: Duration,
    idle_deadline: Instant,
    executing_tools: BTreeSet<String>,
    anonymous_tools: usize,
    tool_idle_deadline: Option<Instant>,
    tool_observed_at: Option<Instant>,
    compaction_depth: usize,
    compaction_deadline: Option<Instant>,
}

impl ProviderWaitWatchdog {
    pub(super) fn new(now: Instant, idle_timeout: Duration) -> Self {
        Self {
            first_body: FirstProviderBodyWatchdog::new(now),
            idle_timeout,
            idle_deadline: now + idle_timeout,
            executing_tools: BTreeSet::new(),
            anonymous_tools: 0,
            tool_idle_deadline: None,
            tool_observed_at: None,
            compaction_depth: 0,
            compaction_deadline: None,
        }
    }

    pub(super) fn deadline(&self) -> Instant {
        let deadline = if let Some(deadline) = self.tool_idle_deadline {
            deadline
        } else if self.first_body.is_active() {
            self.first_body.deadline().expect("active first body")
        } else if let Some(deadline) = self.compaction_deadline {
            deadline
        } else {
            self.idle_deadline
        };
        self.compaction_deadline
            .map_or(deadline, |cap| deadline.min(cap))
    }

    pub(super) fn select_mode(
        &self,
        now: Instant,
        deferrals: usize,
    ) -> FirstProviderBodyWatchdogSelectMode {
        if Some(self.deadline()) == self.first_body.deadline() {
            self.first_body.select_mode(now, deferrals)
        } else if now < self.deadline() {
            FirstProviderBodyWatchdogSelectMode::Fair
        } else if watchdog_expiry_event_deferral_allowed(deferrals) {
            FirstProviderBodyWatchdogSelectMode::PrioritizeQueuedPiEvent
        } else {
            FirstProviderBodyWatchdogSelectMode::Timeout
        }
    }

    pub(super) fn timeout_failure(&self) -> &'static str {
        if Some(self.deadline()) == self.compaction_deadline {
            CHAT_COMPACTION_TIMEOUT_FAILURE
        } else if self.tool_idle_deadline.is_some() {
            CHAT_TOOL_SILENCE_FAILURE
        } else if self.first_body.is_active() {
            self.first_body.timeout_failure()
        } else {
            "Pi stopped responding while waiting for the model"
        }
    }

    pub(super) fn observe(&mut self, event: &PiEventReceive, now: Instant) {
        if let Some(last_observed) = self.tool_observed_at {
            // Preserve the remaining first-body budget even when a tool starts
            // before Pi emits any body event. The active tool has its own cap.
            self.first_body
                .exclude_tool_time(now.saturating_duration_since(last_observed));
            self.tool_observed_at = Some(now);
        }
        if event.is_ok() {
            self.idle_deadline = now + self.idle_timeout;
            if self.tool_idle_deadline.is_some() {
                self.tool_idle_deadline = Some(now + CHAT_TOOL_SILENCE_TIMEOUT);
            }
        }
        if pi_event_indicates_first_provider_body_progress(event) {
            self.first_body.observe_provider_body_progress();
        }
        if let Ok(PiEvent::Rpc { payload }) = event {
            match payload.get("type").and_then(Value::as_str) {
                Some("tool_execution_start") => {
                    if let Some(id) = payload.get("toolCallId").and_then(Value::as_str) {
                        self.executing_tools.insert(id.to_owned());
                    } else {
                        self.anonymous_tools = self.anonymous_tools.saturating_add(1);
                    }
                    self.tool_idle_deadline = Some(now + CHAT_TOOL_SILENCE_TIMEOUT);
                    self.tool_observed_at = Some(now);
                }
                Some("tool_execution_end") => {
                    if let Some(id) = payload.get("toolCallId").and_then(Value::as_str) {
                        self.executing_tools.remove(id);
                    } else {
                        self.anonymous_tools = self.anonymous_tools.saturating_sub(1);
                    }
                    if self.executing_tools.is_empty() && self.anonymous_tools == 0 {
                        self.tool_idle_deadline = None;
                        self.tool_observed_at = None;
                    }
                }
                Some("compaction_start") => {
                    if self.compaction_depth == 0 {
                        self.compaction_deadline = Some(now + CHAT_COMPACTION_TOTAL_TIMEOUT);
                        self.first_body.observe_compaction_start();
                    }
                    self.compaction_depth = self.compaction_depth.saturating_add(1);
                }
                Some("compaction_end") if self.compaction_depth > 0 => {
                    self.compaction_depth -= 1;
                    if self.compaction_depth == 0 {
                        self.compaction_deadline = None;
                        self.first_body.observe_compaction_end(now);
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use FirstProviderBodyWatchdogSelectMode::{Fair, PrioritizeQueuedPiEvent, Timeout};

    #[tokio::test]
    async fn first_body_watchdog_bounds_pre_body_compaction_and_expiry_deferrals() {
        let started = Instant::now();
        let body = started + CHAT_FIRST_PROVIDER_BODY_PROGRESS_TIMEOUT;
        let total = started + CHAT_PRE_BODY_TOTAL_TIMEOUT;
        let mut watchdog = FirstProviderBodyWatchdog::new(started);
        assert_eq!(watchdog.deadline(), Some(body));
        assert_eq!(
            watchdog.observe_compaction_end(started + Duration::from_secs(1)),
            None,
            "an unmatched compaction end must not reset the body deadline"
        );
        assert_eq!(
            watchdog.select_mode(body - Duration::from_millis(1), 0),
            Fair
        );
        for deferrals in 0..CHAT_WATCHDOG_EXPIRY_EVENT_DEFERRAL_LIMIT {
            assert_eq!(
                watchdog.select_mode(body, deferrals),
                PrioritizeQueuedPiEvent
            );
        }
        assert_eq!(
            watchdog.select_mode(body, CHAT_WATCHDOG_EXPIRY_EVENT_DEFERRAL_LIMIT),
            Timeout,
            "an event flood must not postpone an expired watchdog forever"
        );

        // Pre-body compaction swaps in the fixed total deadline; repeats keep it.
        assert_eq!(watchdog.observe_compaction_start(), Some(total));
        assert_eq!(watchdog.observe_compaction_start(), None);
        assert_eq!(watchdog.deadline(), Some(total));
        assert_eq!(watchdog.select_mode(body, 0), Fair);
        let ended = started + Duration::from_secs(1);
        assert_eq!(
            watchdog.observe_compaction_end(ended),
            Some(ended + CHAT_FIRST_PROVIDER_BODY_PROGRESS_TIMEOUT)
        );
        assert_eq!(watchdog.observe_compaction_start(), Some(total));
        assert_eq!(
            watchdog.observe_compaction_end(total - Duration::from_secs(1)),
            Some(total),
            "a fresh body window may not extend the total pre-body deadline"
        );
        watchdog.observe_provider_body_progress();
        assert_eq!(watchdog.deadline(), None);
        assert_eq!(watchdog.observe_compaction_start(), None);

        // At expiry, Stop wins first, then a queued Pi event beats the timer.
        for cancelled in [false, true] {
            let (cancel_sender, mut cancel) = tokio::sync::watch::channel(false);
            let (_control, mut chat_controls) = chat_control_channel();
            let (pi_sender, mut pi_events) = tokio::sync::broadcast::channel(1);
            pi_sender
                .send(PiEvent::Rpc {
                    payload: serde_json::json!({"type": "compaction_start"}),
                })
                .unwrap();
            if cancelled {
                cancel_sender.send(true).unwrap();
            }
            let timer = tokio::time::sleep(Duration::ZERO);
            tokio::pin!(timer);
            let mut deferrals = 0;
            let input = receive_expired_watchdog_input(
                timer.as_mut(),
                &mut cancel,
                &mut chat_controls,
                &mut pi_events,
                &mut deferrals,
            )
            .await;
            if cancelled {
                assert!(matches!(input, ChatLoopInput::CancelChanged(Ok(()))));
                assert_eq!(deferrals, 0);
                assert!(
                    pi_events.try_recv().is_ok(),
                    "Stop must not consume the event"
                );
            } else {
                assert!(matches!(
                    input,
                    ChatLoopInput::PiEvent(Ok(PiEvent::Rpc { .. }))
                ));
                assert_eq!(deferrals, 1);
            }
        }
    }
}
