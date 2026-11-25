use super::*;
use crate::broker::CustomApprovalRefusal;
use crate::commands::tool_executor::approval::AskOutcome;
use crate::custom_connections::policy::ApprovalChoice;
use crate::custom_connections::{service, ConnectionError};

/// How a card for an unapproved read-only tool ended. Only a card the user
/// saw and turned down (or let expire) is a decline; a call no card could be
/// shown for still needs an approval in Settings.
enum CardAnswer {
    Allowed(ApprovalChoice),
    Declined,
    TimedOut,
    Unasked,
}

fn card_answer(outcome: AskOutcome) -> CardAnswer {
    use crate::chat::approvals::ApprovalDecision;
    match outcome {
        AskOutcome::Answered(ApprovalDecision::Once) => {
            CardAnswer::Allowed(ApprovalChoice::AllowOnce)
        }
        AskOutcome::Answered(ApprovalDecision::Always) => {
            CardAnswer::Allowed(ApprovalChoice::Always)
        }
        AskOutcome::Answered(ApprovalDecision::Deny) => CardAnswer::Declined,
        AskOutcome::NoAnswer => CardAnswer::TimedOut,
        AskOutcome::Unasked => CardAnswer::Unasked,
    }
}

const TOO_LARGE_NEXT: &str = "The connection is still running: call it again asking for less (a shorter range, fewer rows or one item) if its arguments allow; otherwise continue without it and say the reply was too large.";
const IN_USE_NEXT: &str = "Call it once more later in this reply, after other work; if it is still in use, continue without this connection and say so.";

fn failure(error: ConnectionError, connection_id: &str, tool_name: &str) -> BrokerError {
    match error {
        ConnectionError::ApprovalRequired => BrokerError::CustomApprovalRequired {
            connection_id: connection_id.into(),
            tool_name: tool_name.into(),
        },
        ConnectionError::Unavailable => BrokerError::MethodDenied,
        ConnectionError::InvalidArguments => super::errors::invalid_mcp_arguments(),
        ConnectionError::KeychainBusy | ConnectionError::KeychainMutationPending => {
            BrokerError::KeychainBusy
        }
        ConnectionError::KeychainUnavailable => BrokerError::KeychainUnavailable,
        // Another request holds the connection; its key is not involved.
        ConnectionError::InUse => BrokerError::Execution(error.to_string()).with_next(IN_USE_NEXT),
        ConnectionError::ResultTooLarge { limit_bytes } => BrokerError::ResultTooLarge {
            bytes: None,
            limit_bytes: Some(limit_bytes),
        }
        .with_next(TOO_LARGE_NEXT),
        _ => BrokerError::Execution(error.to_string()),
    }
}

impl AppToolExecutor {
    /// The read-only tool card the user can approve from Chat; None in unattended runs,
    /// without an approval sink, or for tools that can never be approved.
    fn approvable_tool(
        &self,
        connection: &service::ConnectionView,
        tool_name: &str,
    ) -> Option<service::ToolCard> {
        if self.capture.unattended_run_id.is_some()
            || self.capture.approval_sink.is_none()
            || super::super::python_reads::nested()
        {
            return None;
        }
        connection
            .tools
            .iter()
            .find(|tool| {
                tool.name == tool_name
                    && tool.eligibility
                        == crate::custom_connections::policy::ToolEligibility::ReadOnly
            })
            .cloned()
    }

    /// Shows an approval card and waits for the user, the wait limit or the run stopping.
    async fn wait_for_approval(
        &self,
        connection: &service::ConnectionView,
        request: &McpToolCallRequest,
        card: &service::ToolCard,
    ) -> CardAnswer {
        use crate::chat::approvals::{self, ApprovalState, ChatApproval};
        let approval = ChatApproval {
            request_id: uuid::Uuid::new_v4().to_string(),
            item_id: None,
            connection_id: request.server_id.clone(),
            connection_name: connection.name.clone(),
            tool_name: card.name.clone(),
            target: approvals::target_summary(&request.arguments),
            state: ApprovalState::Waiting,
            requested_at_ms: chrono::Utc::now().timestamp_millis(),
        };
        card_answer(self.ask_user(approval).await)
    }

    pub(super) async fn custom_mcp_connect(&self, id: &str) -> Result<Value, BrokerError> {
        if !crate::extension_availability::custom_connections() || !self.capability_ids.contains(id)
        {
            return Err(BrokerError::MethodDenied);
        }
        self.capture
            .turn_taint
            .enforce(ToolMethod::McpConnect, &json!({"server_id": id}))?;
        let manager = service::for_state(&self.state).map_err(|e| failure(e, id, ""))?;
        let _outbound = self.capture.turn_taint.custom_outbound.lock().await;
        self.capture
            .turn_taint
            .enforce(ToolMethod::McpConnect, &json!({"server_id": id}))?;
        let mut service = manager.for_connection(id).map_err(|e| failure(e, id, ""))?;
        let tools = service
            .connect(
                &self.loupist_id,
                id,
                self.capture.unattended_run_id.is_none(),
            )
            .await
            .map_err(|e| failure(e, id, ""))?;
        Ok(json!({"server_id":id,"tools":tools}))
    }

    pub(super) async fn custom_mcp_call(
        &self,
        request: McpToolCallRequest,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if !crate::extension_availability::custom_connections()
            || !self.capability_ids.contains(&request.server_id)
        {
            return Err(BrokerError::MethodDenied);
        }
        // The custom tier pauses a call only when the request names its
        // server, so every recheck below passes the same target.
        let target = json!({"server_id": request.server_id, "tool_name": request.tool_name});
        self.capture
            .turn_taint
            .enforce(ToolMethod::McpCall, &target)?;
        let manager = service::for_state(&self.state)
            .map_err(|e| failure(e, &request.server_id, &request.tool_name))?;
        let mut outbound = self.capture.turn_taint.custom_outbound.lock().await;
        let mut service = manager
            .for_connection(&request.server_id)
            .map_err(|e| failure(e, &request.server_id, &request.tool_name))?;
        // Recheck after acquiring the turn gate: concurrent custom calls
        // must not dispatch after an earlier result has tainted this turn.
        self.capture
            .turn_taint
            .enforce(ToolMethod::McpCall, &target)?;
        let connection = service
            .get(&request.server_id, Some(&self.loupist_id))
            .map_err(|e| failure(e, &request.server_id, &request.tool_name))?;
        let mut result = service
            .call(
                &self.loupist_id,
                &request.server_id,
                &request.tool_name,
                request.arguments.clone(),
                self.capture.unattended_run_id.is_some() || super::super::python_reads::nested(),
            )
            .await;
        // An interactive turn asks the user instead of refusing a read-only tool outright.
        let mut refusal = None;
        if matches!(result, Err(ConnectionError::ApprovalRequired)) {
            if let Some(card) = self.approvable_tool(&connection, &request.tool_name) {
                drop(service);
                drop(outbound);
                let answer = self.wait_for_approval(&connection, &request, &card).await;
                outbound = self.capture.turn_taint.custom_outbound.lock().await;
                service = manager
                    .for_connection(&request.server_id)
                    .map_err(|e| failure(e, &request.server_id, &request.tool_name))?;
                self.capture
                    .turn_taint
                    .enforce(ToolMethod::McpCall, &target)?;
                result = match answer {
                    CardAnswer::Allowed(choice) => {
                        let approved = service
                            .approve(
                                &self.loupist_id,
                                &request.server_id,
                                &request.tool_name,
                                &card.definition_hash,
                                choice,
                            )
                            .await;
                        match approved {
                            Ok(_) => {
                                service
                                    .call(
                                        &self.loupist_id,
                                        &request.server_id,
                                        &request.tool_name,
                                        request.arguments.clone(),
                                        false,
                                    )
                                    .await
                            }
                            Err(error) => Err(error),
                        }
                    }
                    CardAnswer::Declined => {
                        refusal = Some(CustomApprovalRefusal::Declined);
                        Err(ConnectionError::ApprovalRequired)
                    }
                    CardAnswer::TimedOut => {
                        refusal = Some(CustomApprovalRefusal::TimedOut);
                        Err(ConnectionError::ApprovalRequired)
                    }
                    CardAnswer::Unasked => Err(ConnectionError::ApprovalRequired),
                };
            }
        }
        let mut result = match result {
            Ok(result) => result,
            Err(error) => {
                if matches!(error, ConnectionError::ApprovalRequired) {
                    self.capture
                        .record_tool_refusal(crate::domain::ChatToolRefusal {
                            connection_id: request.server_id.clone(),
                            connection_name: connection.name.clone(),
                            tool_name: request.tool_name.clone(),
                            reason: if self.capture.unattended_run_id.is_some() {
                                crate::domain::ChatToolRefusalReason::BlockedUnattended
                            } else if refusal.is_some() {
                                crate::domain::ChatToolRefusalReason::Declined
                            } else {
                                crate::domain::ChatToolRefusalReason::NeedsApproval
                            },
                        });
                    if let Some(run_id) = self.capture.scheduled_run_id() {
                        self.state
                            .store
                            .scheduled_run_skip_tool(
                                &self.loupist_id,
                                run_id,
                                &request.tool_name,
                                CustomApprovalRefusal::Unattended.message(),
                            )
                            .map_err(|_| {
                                BrokerError::Execution(
                                    "Could not record refused unattended tool".into(),
                                )
                            })?;
                    }
                    let cause = if self.capture.unattended_run_id.is_some() {
                        Some(CustomApprovalRefusal::Unattended)
                    } else {
                        refusal
                    };
                    if let Some(cause) = cause {
                        return Err(BrokerError::CustomApprovalRefused {
                            connection_id: request.server_id.clone(),
                            tool_name: request.tool_name.clone(),
                            cause,
                        });
                    }
                }
                return Err(failure(error, &request.server_id, &request.tool_name));
            }
        };
        // Rust is the source of truth. Mark before exposing any response bytes;
        // this also covers isError content and later delivery cancellation.
        self.capture.turn_taint.mark(
            &request.server_id,
            &connection.name,
            connection.dont_save_to_memory,
        );
        drop(service);
        drop(outbound);
        if let Some(error) = super::errors::rejected_custom_arguments(&result) {
            return Err(error);
        }
        if !result.is_object() {
            return Err(BrokerError::Malformed);
        }
        result["origin"] = json!(self.capture.turn_taint.origin());
        let capture_request = json!({"server_id":request.server_id,"tool_name":request.tool_name,"arguments":request.arguments});
        let result_ref = self
            .capture
            .stage_run_result_with_delivery(
                delivery_id,
                RunResultProducer {
                    origin: if result.get("isError").and_then(Value::as_bool) == Some(true) {
                        RunResultOrigin::Discovery
                    } else {
                        RunResultOrigin::AcquiredOriginal
                    },
                    runtime_id: request.server_id,
                    tool_name: request.tool_name,
                    provider: None,
                },
                &capture_request,
                result.clone(),
                Vec::new(),
                None,
            )
            .await?;
        if result.get("isError").and_then(Value::as_bool) != Some(true) && returned_no_rows(&result)
        {
            self.capture
                .stage(delivery_id, |pending| {
                    pending.unlisted_results.insert(result_ref.clone());
                })
                .await;
            describe_empty_read(&mut result);
            result["result_ref"] = json!(result_ref);
        } else {
            attach_result_ref(&mut result, &result_ref);
        }
        Ok(json!({"result":result}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::approvals::ApprovalDecision;

    #[test]
    fn approval_timeout_and_decline_are_distinct_without_changing_allowed_or_unasked_cards() {
        assert!(matches!(
            card_answer(AskOutcome::NoAnswer),
            CardAnswer::TimedOut
        ));
        assert!(matches!(
            card_answer(AskOutcome::Answered(ApprovalDecision::Deny)),
            CardAnswer::Declined
        ));
        assert!(matches!(
            card_answer(AskOutcome::Answered(ApprovalDecision::Once)),
            CardAnswer::Allowed(ApprovalChoice::AllowOnce)
        ));
        assert!(matches!(
            card_answer(AskOutcome::Answered(ApprovalDecision::Always)),
            CardAnswer::Allowed(ApprovalChoice::Always)
        ));
        assert!(matches!(
            card_answer(AskOutcome::Unasked),
            CardAnswer::Unasked
        ));
    }
    #[test]
    fn custom_keychain_failures_keep_the_model_recovery_contract() {
        for error in [
            ConnectionError::KeychainBusy,
            ConnectionError::KeychainMutationPending,
        ] {
            assert!(matches!(
                failure(error, "fixture", ""),
                BrokerError::KeychainBusy
            ));
        }
        assert!(matches!(
            failure(ConnectionError::KeychainUnavailable, "fixture", ""),
            BrokerError::KeychainUnavailable
        ));
        // A connection another request holds is not a key being changed.
        assert!(matches!(
            failure(ConnectionError::InUse, "fixture", ""),
            BrokerError::WithNext { source, next }
                if matches!(source.as_ref(), BrokerError::Execution(message) if message.contains("in use"))
                    && next == IN_USE_NEXT
        ));
        // An oversized reply is its own refusal; the session keeps running.
        for limit_bytes in [
            crate::custom_connections::http::MAX_RESPONSE_BYTES as u64,
            crate::mcp::MAX_FRAME_BYTES as u64,
        ] {
            let body = crate::broker::error_body(&failure(
                ConnectionError::ResultTooLarge { limit_bytes },
                "fixture",
                "",
            ));
            assert_eq!(body["code"], "result_too_large");
            assert_eq!(body["limit_bytes"], limit_bytes);
            assert!(body["next"]
                .as_str()
                .unwrap()
                .starts_with("The connection is still running"));
        }
    }
}
