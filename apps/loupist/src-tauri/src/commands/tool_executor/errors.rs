//! Storage diagnostics can name private files or quote stored/provider bytes.
//! Keep validation repairs, but never return internal diagnostics to a model.
use super::*;

pub(super) fn command_error(error: crate::app::CommandError) -> BrokerError {
    if matches!(
        error.code.as_str(),
        "internal" | "sqlite" | "json" | "io" | "private_storage" | "lock_poisoned"
    ) {
        BrokerError::Execution("Local data is unavailable. Try again or restart Loupist.".into())
    } else {
        BrokerError::Execution(error.message)
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
            "Local data is unavailable. Try again or restart Loupist.".into(),
        ),
        error => BrokerError::Execution(error.to_string()),
    }
}

pub(super) fn knowledge_error(error: crate::knowledge::KnowledgeError) -> BrokerError {
    use crate::knowledge::KnowledgeError as E;
    match error {
        E::Sqlite(_) | E::Json(_) | E::PrivateStorage(_) | E::Io(_) | E::LockPoisoned => {
            command_error(crate::app::CommandError::internal(
                "knowledge storage failed",
            ))
        }
        error => BrokerError::Execution(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert!(error.to_string().contains("restart Loupist"));
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
