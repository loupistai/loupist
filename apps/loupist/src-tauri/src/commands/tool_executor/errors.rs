//! Storage diagnostics can name private files or quote stored/provider bytes.
//! Keep validation repairs, but never return internal diagnostics to a model.
use super::*;
use crate::{app::CommandErrorCode, broker::BrokerRefusalReason};

pub(super) fn command_error(error: crate::app::CommandError) -> BrokerError {
    if error.code == "credential_locked" {
        return BrokerError::KeychainUnavailable;
    }
    if error.code == "credential_unavailable" {
        match error
            .detail
            .as_ref()
            .and_then(|detail| detail.get("kind"))
            .and_then(Value::as_str)
        {
            Some("keychain_busy") => return BrokerError::KeychainBusy,
            Some("keychain_unavailable") => return BrokerError::KeychainUnavailable,
            _ => {}
        }
    }
    let reason = if error.code == CommandErrorCode::InvalidConfiguration
        && error
            .detail
            .as_ref()
            .and_then(|detail| detail.get("reason"))
            .and_then(Value::as_str)
            == Some(crate::commands::configuration::BELOW_SOURCE_CALL_LIMIT)
    {
        BrokerRefusalReason::BelowSourceCallLimit
    } else {
        BrokerRefusalReason::Command(error.code.clone())
    };
    BrokerError::Execution(command_message(error)).with_reason(reason)
}

pub(super) fn credential_read_error(
    error: crate::finance_credentials::FinanceCredentialError,
) -> BrokerError {
    use crate::finance_credentials::FinanceCredentialError;
    match error {
        FinanceCredentialError::PermissionRequired => BrokerError::KeychainPermissionRequired,
        FinanceCredentialError::Busy => BrokerError::KeychainBusy,
        FinanceCredentialError::Locked | FinanceCredentialError::Unavailable => {
            BrokerError::KeychainUnavailable
        }
        _ => BrokerError::Execution("credential lookup failed".into()),
    }
}

pub(super) fn command_message(error: crate::app::CommandError) -> String {
    if matches!(
        error.code.as_str(),
        "internal" | "sqlite" | "json" | "io" | "private_storage" | "lock_poisoned"
    ) {
        "Local data could not be read. Do not repeat this call; tell the user it failed and that restarting Loupist may help.".into()
    } else {
        error.message
    }
}

pub(super) fn store_error(error: crate::store::StoreError) -> BrokerError {
    match error {
        crate::store::StoreError::Sqlite(_)
        | crate::store::StoreError::Json(_)
        | crate::store::StoreError::PrivateStorage(_)
        | crate::store::StoreError::Upgrade(_)
        | crate::store::StoreError::Invalid(_)
        | crate::store::StoreError::LockPoisoned => BrokerError::Execution(
            "Local data could not be read. Do not repeat this call; tell the user it failed and that restarting Loupist may help.".into(),
        ),
        error => BrokerError::Execution(error.to_string()),
    }
}

pub(super) fn knowledge_error(error: crate::knowledge::KnowledgeError) -> BrokerError {
    use crate::knowledge::KnowledgeError as E;
    let reason = BrokerRefusalReason::Command(CommandErrorCode::parse(error.code()));
    match error {
        E::Sqlite(_) | E::Json(_) | E::PrivateStorage(_) | E::Io(_) | E::LockPoisoned => {
            command_error(crate::app::CommandError::internal(
                "knowledge storage failed",
            ))
        }
        error => BrokerError::Execution(error.to_string()),
    }
    .with_reason(reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_refusals_keep_typed_reasons_and_their_model_messages() {
        let message = "A daily allowance must be zero or at least 7.";
        let command = |code, reason| {
            command_error(crate::app::CommandError::with_detail(
                code,
                message,
                json!({"reason": reason, "minimum": 7}),
            ))
        };
        for (code, detail, expected) in [
            (
                CommandErrorCode::InvalidConfiguration,
                "below_source_call_limit",
                "below_source_call_limit",
            ),
            (
                CommandErrorCode::InvalidConfiguration,
                "private_value",
                "invalid_configuration",
            ),
            (
                CommandErrorCode::NotFound,
                "below_source_call_limit",
                "not_found",
            ),
            (
                CommandErrorCode::Unlisted("private_value".into()),
                "private_value",
                "execution_failed",
            ),
        ] {
            let error = command(code, detail);
            assert_eq!(error.trace_reason(), expected);
            let body = crate::broker::error_body(&error);
            assert_eq!(body["code"], "execution_failed");
            assert_eq!(body["message"], format!("tool execution failed: {message}"));
            assert!(body.get("reason").is_none());
        }
    }

    #[test]
    fn bundled_keychain_failures_keep_the_model_recovery_contract() {
        use crate::{
            broker::error_body, finance_credentials::FinanceCredentialError,
            keychain_operation::KeychainError,
        };
        let busy = json!({
            "code": "keychain_busy",
            "message": "The saved key for this connection is being changed right now, so it was not read.",
            "retryable": true,
            "next": "Call this once more. If it fails again, tell the user the key change has not finished and continue without this connection.",
        });
        let unavailable = json!({
            "code": "keychain_unavailable",
            "message": "macOS did not release the saved key for this connection. It may be showing the user a permission prompt, or the keychain is locked.",
            "retryable": false,
            "next": "Tell the user to answer the macOS prompt with Always Allow, or unlock the keychain, and then ask again. Continue this reply without this connection.",
        });
        assert_eq!(
            error_body(&credential_read_error(FinanceCredentialError::Busy)),
            busy
        );
        assert_eq!(
            error_body(&command_error(KeychainError::Busy.command_error())),
            busy
        );
        for error in [
            FinanceCredentialError::Locked,
            FinanceCredentialError::Unavailable,
        ] {
            assert_eq!(error_body(&credential_read_error(error)), unavailable);
        }
        for error in [KeychainError::TimedOut, KeychainError::Unavailable] {
            assert_eq!(
                error_body(&command_error(error.command_error())),
                unavailable
            );
        }
    }

    #[test]
    fn local_diagnostics_hide_private_details_and_keep_validation_repairs() {
        let private = "/private/app-data/database.sqlite: api_key=fixture-secret";
        for error in [
            command_error(crate::app::CommandError::internal(private)),
            knowledge_error(crate::knowledge::KnowledgeError::Io(std::io::Error::other(
                private,
            ))),
            store_error(crate::store::StoreError::Upgrade(private.into())),
            store_error(crate::store::StoreError::Invalid(private.into())),
            store_error(crate::store::StoreError::Sqlite(
                rusqlite::Error::InvalidParameterName(private.into()),
            )),
        ] {
            assert!(!error.to_string().contains(private));
            assert!(error.to_string().contains("restarting Loupist"));
            assert!(error.to_string().contains("Do not repeat this call"));
            assert!(!error.to_string().contains("Try again"));
        }
        for code in [
            "internal",
            "sqlite",
            "json",
            "io",
            "private_storage",
            "lock_poisoned",
        ] {
            assert!(!command_error(crate::app::CommandError::new(code, private))
                .to_string()
                .contains(private));
        }
        assert_eq!(
            command_error(crate::app::CommandError::invalid(
                "Read the current revision"
            ))
            .to_string(),
            "tool execution failed: Read the current revision"
        );
    }
}
