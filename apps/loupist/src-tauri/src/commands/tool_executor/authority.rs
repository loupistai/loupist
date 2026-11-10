//! The executor's own authority checks: the Loupist's enabled connections,
//! the turn's Loupist and Chat scope, and a provider result's identity.
use super::*;

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
        let chat = self
            .state
            .store
            .get_chat(&policy.session_id)
            .map_err(|error| BrokerError::Execution(error.to_string()))?
            .ok_or_else(|| BrokerError::Execution("chat session is missing".into()))?;
        if chat.loupist_id != policy.loupist_id {
            return Err(BrokerError::Execution(
                "Loupist/session scope does not match".into(),
            ));
        }
        Ok(())
    }
}
