//! The executor's own authority checks: the Loupist's enabled connections,
//! the turn's Loupist and Chat scope, and a provider result's identity.
use super::*;
use rusqlite::OptionalExtension;

impl AppToolExecutor {
    pub(super) fn capability_enabled(&self, entry_id: &str) -> bool {
        self.capability_ids.contains(entry_id)
    }

    pub(super) fn require_capability(&self, entry_id: &str) -> Result<(), BrokerError> {
        if self.capability_enabled(entry_id) {
            Ok(())
        } else {
            Err(BrokerError::MethodDenied)
        }
    }

    /// A native vendor read's result names its tool and the vendor that serves it.
    pub(super) fn validate_provider_tool_output(
        &self,
        output: Value,
        method: ToolMethod,
    ) -> Result<Value, BrokerError> {
        let vendor =
            crate::finance_data::serving_vendor(method).ok_or(BrokerError::MethodDenied)?;
        validate_provider_result(&output, &method.tool_name(), vendor.source_id)?;
        Ok(output)
    }

    pub(super) fn ensure_scope(&self, policy: &ToolPolicy) -> Result<(), BrokerError> {
        if policy.loupist_id != self.loupist_id {
            return Err(BrokerError::Execution(
                "Loupist/tool root scope does not match".into(),
            ));
        }
        // Read only the authoritative binding, not the entire transcript. Check
        // it each time so deletion or a changed binding is still refused.
        let chat_loupist: String = self
            .state
            .store
            .lock_loupist(&policy.loupist_id)
            .map_err(|_| BrokerError::Execution("chat scope is unavailable".into()))?
            .query_row(
                "SELECT loupist_id FROM chat_sessions WHERE id = ?1",
                [&policy.session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| BrokerError::Execution("chat scope is unavailable".into()))?
            .ok_or_else(|| BrokerError::Execution("chat session is missing".into()))?;
        if chat_loupist != policy.loupist_id {
            return Err(BrokerError::Execution(
                "Loupist/session scope does not match".into(),
            ));
        }
        Ok(())
    }
}
