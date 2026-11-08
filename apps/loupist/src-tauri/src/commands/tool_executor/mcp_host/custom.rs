use super::*;
use crate::commands::tool_executor::approval::AskOutcome;
use crate::custom_connections::policy::ApprovalChoice;
use crate::custom_connections::{service, ConnectionError};

/// How a card for an unapproved read-only tool ended. Only a card the user
/// saw and turned down (or let expire) is a decline; a call no card could be
/// shown for still needs an approval in Settings.
enum CardAnswer {
    Allowed(ApprovalChoice),
    Declined,
    Unasked,
}

fn failure(error: ConnectionError, connection_id: &str, tool_name: &str) -> BrokerError {
    match error {
        ConnectionError::ApprovalRequired => BrokerError::CustomApprovalRequired {
            connection_id: connection_id.into(),
            tool_name: tool_name.into(),
        },
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
        if self.capture.unattended_run_id.is_some() || self.capture.approval_sink.is_none() {
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
        use crate::chat::approvals::{self, ApprovalDecision, ApprovalState, ChatApproval};
        let approval = ChatApproval {
            request_id: uuid::Uuid::new_v4().to_string(),
            kind: None,
            item_id: None,
            connection_id: request.server_id.clone(),
            connection_name: connection.name.clone(),
            tool_name: card.name.clone(),
            target: approvals::target_summary(&request.arguments),
            state: ApprovalState::Waiting,
            requested_at_ms: chrono::Utc::now().timestamp_millis(),
        };
        match self.ask_user(approval).await {
            AskOutcome::Answered(ApprovalDecision::Once) => {
                CardAnswer::Allowed(ApprovalChoice::AllowOnce)
            }
            AskOutcome::Answered(ApprovalDecision::Always) => {
                CardAnswer::Allowed(ApprovalChoice::Always)
            }
            AskOutcome::Answered(ApprovalDecision::Deny) | AskOutcome::NoAnswer => {
                CardAnswer::Declined
            }
            AskOutcome::Unasked => CardAnswer::Unasked,
        }
    }

    pub(super) async fn custom_mcp_connect(&self, id: &str) -> Result<Value, BrokerError> {
        if !self.capability_ids.contains(id) {
            return Err(BrokerError::MethodDenied);
        }
        self.capture
            .turn_taint
            .enforce(ToolMethod::McpConnect, &json!({}))?;
        let manager = service::for_state(&self.state).map_err(|e| failure(e, id, ""))?;
        let mut service = manager.lock().await;
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
        if !self.capability_ids.contains(&request.server_id) {
            return Err(BrokerError::MethodDenied);
        }
        self.capture
            .turn_taint
            .enforce(ToolMethod::McpCall, &json!({}))?;
        let manager = service::for_state(&self.state)
            .map_err(|e| failure(e, &request.server_id, &request.tool_name))?;
        let mut service = manager.lock().await;
        // Recheck after acquiring the connection lock: concurrent custom calls
        // must not dispatch after an earlier result has tainted this turn.
        self.capture
            .turn_taint
            .enforce(ToolMethod::McpCall, &json!({}))?;
        let connection = service
            .get(&request.server_id, Some(&self.loupist_id))
            .map_err(|e| failure(e, &request.server_id, &request.tool_name))?;
        let mut result = service
            .call(
                &self.loupist_id,
                &request.server_id,
                &request.tool_name,
                request.arguments.clone(),
                self.capture.unattended_run_id.is_some(),
            )
            .await;
        // An interactive turn asks the user instead of refusing a read-only tool outright.
        let mut declined = false;
        if matches!(result, Err(ConnectionError::ApprovalRequired)) {
            if let Some(card) = self.approvable_tool(&connection, &request.tool_name) {
                drop(service);
                let answer = self.wait_for_approval(&connection, &request, &card).await;
                service = manager.lock().await;
                self.capture
                    .turn_taint
                    .enforce(ToolMethod::McpCall, &json!({}))?;
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
                        declined = true;
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
                            } else if declined {
                                crate::domain::ChatToolRefusalReason::Declined
                            } else {
                                crate::domain::ChatToolRefusalReason::NeedsApproval
                            },
                        });
                }
                if let Some(run_id) = self.capture.scheduled_run_id() {
                    self.state.store.scheduled_run_skip_tool(&self.loupist_id, run_id, &request.tool_name, "Custom MCP tool requires an existing Always approval and an unchanged read-only definition")
                        .map_err(|_| BrokerError::Execution("Could not record refused unattended tool".into()))?;
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
        attach_result_ref(&mut result, &result_ref);
        Ok(json!({"result":result}))
    }
}
