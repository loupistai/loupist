use super::*;
use crate::custom_connections::{service, ConnectionError};

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
    /// Returns the approval to store, or None when the run continues without the tool.
    async fn wait_for_approval(
        &self,
        connection: &service::ConnectionView,
        request: &McpToolCallRequest,
        card: &service::ToolCard,
    ) -> Option<crate::custom_connections::policy::ApprovalChoice> {
        use crate::chat::approvals::{self, ApprovalDecision, ApprovalState, ChatApproval};
        use crate::custom_connections::policy::ApprovalChoice;
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
        match self.ask_user(approval).await? {
            ApprovalDecision::Once => Some(ApprovalChoice::AllowOnce),
            ApprovalDecision::Always => Some(ApprovalChoice::Always),
            ApprovalDecision::Deny => None,
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
        let mut asked = None;
        if matches!(result, Err(ConnectionError::ApprovalRequired)) {
            if let Some(card) = self.approvable_tool(&connection, &request.tool_name) {
                drop(service);
                let decision = self.wait_for_approval(&connection, &request, &card).await;
                service = manager.lock().await;
                self.capture
                    .turn_taint
                    .enforce(ToolMethod::McpCall, &json!({}))?;
                result = match decision {
                    Some(choice) => {
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
                    None => Err(ConnectionError::ApprovalRequired),
                };
                asked = Some(decision.is_some());
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
                            } else if asked == Some(false) {
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
