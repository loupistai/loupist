use super::*;

#[derive(Clone, Debug)]
pub(in crate::commands) struct StagedPrinciple {
    pub record_id: String,
    pub expected_revision_id: Option<String>,
    pub input: crate::knowledge::PrincipleInput,
}

impl AppToolExecutor {
    pub(super) async fn stage_principle_save(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if !policy.propose_memory_updates || !policy.use_memory {
            return Err(BrokerError::MemoryDisabled);
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Input {
            statement: String,
            why: Option<String>,
            example: Option<String>,
            replaces_record_id: Option<String>,
        }
        let request: Input =
            serde_json::from_value(params).map_err(|_| BrokerError::InvalidArguments {
                field: "principle_save".into(),
                expected: "statement, optional why, example, and replaces_record_id".into(),
            })?;
        let mut input = crate::knowledge::PrincipleInput {
            statement: request.statement,
            why: request.why,
            example: request.example,
            origin: "chat".into(),
            origin_chat_id: self.capture.bound_chat_id.clone(),
            origin_message_id: self.capture.principle_user_message_id.clone(),
        };
        crate::knowledge::principles::validate_principle(&mut input)
            .map_err(|e| BrokerError::Execution(e.to_string()))?;
        let current = crate::commands::knowledge::principles::list(
            &self.state,
            crate::commands::knowledge::principles::PrincipleListRequest {
                loupist_id: policy.loupist_id.clone(),
                include_archived: Some(true),
            },
        )
        .map_err(|e| BrokerError::Execution(e.to_string()))?;
        let folded = crate::knowledge::fold_lens_title(&input.statement);
        if current.iter().any(|p| {
            !p.archived
                && Some(&p.record_id) != request.replaces_record_id.as_ref()
                && crate::knowledge::fold_lens_title(&p.statement) == folded
        }) || self.capture.staged_principles.lock().await.iter().any(|p| {
            Some(&p.record_id) != request.replaces_record_id.as_ref()
                && crate::knowledge::fold_lens_title(&p.input.statement) == folded
        }) {
            return Err(BrokerError::Execution(
                "You already have this principle.".into(),
            ));
        }
        let (record_id, expected_revision_id) = if let Some(id) = request.replaces_record_id {
            let context = self.capture.principle_context.lock().await;
            let mut revision = context.get(&id).cloned();
            drop(context);
            if revision.is_none() {
                revision = self
                    .capture
                    .staged_reads
                    .lock()
                    .await
                    .iter()
                    .find(|r| r.scope == "lens" && r.record_id == id)
                    .map(|r| r.revision_id.clone());
            }
            let revision = revision.ok_or_else(|| {
                BrokerError::Execution(
                    "Read the current principle revision before replacing it".into(),
                )
            })?;
            if !current
                .iter()
                .any(|p| p.record_id == id && p.revision_id == revision)
            {
                return Err(BrokerError::Execution(
                    "The principle revision changed; read it again".into(),
                ));
            }
            (id, Some(revision))
        } else {
            (format!("rec_{}", uuid::Uuid::new_v4().simple()), None)
        };
        let staged = StagedPrinciple {
            record_id: record_id.clone(),
            expected_revision_id,
            input,
        };
        self.capture
            .stage(delivery_id, |pending| pending.principles.push(staged))
            .await;
        Ok(json!({"status":"staged","recordId":record_id,"kind":"principle","durable":false}))
    }
}
