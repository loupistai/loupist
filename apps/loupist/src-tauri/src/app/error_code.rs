//! The codes a [`super::CommandError`] reaches the renderer with.
//!
//! The renderer branches on these, so the set is a contract: its TypeScript
//! union is generated with the bridge types (`src/contracts/generated/app/error_code.ts`).
//! Add a code here before returning it. A string code still converts, for
//! modules not yet migrated to the enum, but crosses as an unlisted code the
//! renderer only shows; `listed_command_error_codes` checks the literals.

macro_rules! command_error_codes {
    ($($variant:ident => $code:literal,)*) => {
        #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
        #[cfg_attr(test, derive(ts_rs::TS))]
        pub enum CommandErrorCode {
            $(#[serde(rename = $code)] $variant,)*
            /// A code missing from this list. Serialized unchanged.
            #[serde(untagged)]
            #[cfg_attr(test, ts(skip))]
            Unlisted(String),
        }

        impl CommandErrorCode {
            pub fn as_str(&self) -> &str {
                match self {
                    $(Self::$variant => $code,)*
                    Self::Unlisted(code) => code,
                }
            }

            pub fn parse(code: &str) -> Self {
                match code {
                    $($code => Self::$variant,)*
                    other => Self::Unlisted(other.to_owned()),
                }
            }
        }
    };
}

command_error_codes! {
    AddFailed => "add_failed",
    AppInstanceConflict => "app_instance_conflict",
    AssetIdentity => "asset_identity",
    AttachmentTooLarge => "attachment_too_large",
    AuthorizationRequired => "authorization_required",
    BrowserUrlCredentials => "browser_url_credentials",
    BrowserUrlInvalid => "browser_url_invalid",
    BrowserUrlTooLong => "browser_url_too_long",
    BrowserUrlUnsupportedProtocol => "browser_url_unsupported_protocol",
    Cancelled => "cancelled",
    CatalogNewerVersion => "catalog_newer_version",
    CatalogUpgradeFailed => "catalog_upgrade_failed",
    ChatCancelled => "chat_cancelled",
    ClientIdRequired => "client_id_required",
    ConfigApplyPending => "config_apply_pending",
    ConfigurationUnavailable => "configuration_unavailable",
    Conflict => "conflict",
    ConflictingDependencyRevision => "conflicting_dependency_revision",
    ConnectionUnavailable => "connection_unavailable",
    CredentialRejected => "credential_rejected",
    CredentialStoreUnavailable => "credential_store_unavailable",
    CursorStale => "cursor_stale",
    CustomConnectionFailed => "custom_connection_failed",
    DigestMismatch => "digest_mismatch",
    DuplicatePrinciple => "duplicate_principle",
    EarningsCaptureFailed => "earnings_capture_failed",
    FindingsUnresolved => "findings_unresolved",
    FlaggedOneAtATime => "flagged_one_at_a_time",
    GalleryTermsRequired => "gallery_terms_required",
    GithubFailed => "github_failed",
    HoldingsImportFailed => "holdings_import_failed",
    InspectionReceiptNotCitable => "inspection_receipt_not_citable",
    Internal => "internal",
    Invalid => "invalid",
    InvalidConfiguration => "invalid_configuration",
    InvalidRequest => "invalid_request",
    Io => "io",
    ItemTooLarge => "item_too_large",
    JournalCheck => "journal_check",
    JournalCheckConfidence => "journal_check_confidence",
    JournalCheckDate => "journal_check_date",
    JournalCheckDirection => "journal_check_direction",
    JournalCheckThreshold => "journal_check_threshold",
    JournalDate => "journal_date",
    JournalEntities => "journal_entities",
    JournalEntity => "journal_entity",
    JournalOutcome => "journal_outcome",
    JournalOutcomeCheck => "journal_outcome_check",
    JournalOutcomeExactRead => "journal_outcome_exact_read",
    JournalOutcomeNotDue => "journal_outcome_not_due",
    JournalOutcomeObserved => "journal_outcome_observed",
    JournalOutcomeObservedAt => "journal_outcome_observed_at",
    JournalOutcomeReview => "journal_outcome_review",
    JournalReviewEvidence => "journal_review_evidence",
    JournalReviewedCheck => "journal_reviewed_check",
    JournalThesis => "journal_thesis",
    JournalThesisLine => "journal_thesis_line",
    Json => "json",
    KeyRejected => "key_rejected",
    KindAlreadyKnown => "kind_already_known",
    LensBodyForbidden => "lens_body_forbidden",
    LicenseConflict => "license_conflict",
    LicenseNotShareable => "license_not_shareable",
    LinkInvalid => "link_invalid",
    LinkNotFound => "link_not_found",
    ListFailed => "list_failed",
    LockPoisoned => "lock_poisoned",
    LoupistEarlierVersion => "loupist_earlier_version",
    LoupistNewerVersion => "loupist_newer_version",
    LoupistStorageUnavailable => "loupist_storage_unavailable",
    MaintenanceActive => "maintenance_active",
    MetadataOnly => "metadata_only",
    MissingBytes => "missing_bytes",
    NameTaken => "name_taken",
    NeedsAccountProfile => "needs_account_profile",
    NeedsApproval => "needs_approval",
    NotConfigured => "not_configured",
    NotFound => "not_found",
    NothingToShare => "nothing_to_share",
    Offline => "offline",
    PasteInvalid => "paste_invalid",
    PiUnavailable => "pi_unavailable",
    PrivateStorage => "private_storage",
    ProviderConnectFailed => "provider_connect_failed",
    ProviderUnavailable => "provider_unavailable",
    PublishDenied => "publish_denied",
    RateLimited => "rate_limited",
    RepoGone => "repo_gone",
    RepoMismatch => "repo_mismatch",
    RevisionConflict => "revision_conflict",
    SafetyBlocked => "safety_blocked",
    ScheduledStreamLimit => "scheduled_stream_limit",
    SchemaMismatch => "schema_mismatch",
    SecretRefused => "secret_refused",
    SignedOut => "signed_out",
    SkillUnavailable => "skill_unavailable",
    Sqlite => "sqlite",
    StagingExpired => "staging_expired",
    ThesisAfterUndo => "thesis_after_undo",
    ThesisEvidence => "thesis_evidence",
    ThesisLine => "thesis_line",
    ThesisLineObserved => "thesis_line_observed",
    ThesisLineUnchecked => "thesis_line_unchecked",
    ThesisNoPage => "thesis_no_page",
    ThesisNone => "thesis_none",
    ThesisReason => "thesis_reason",
    ThesisReasonId => "thesis_reason_id",
    ThesisSameStep => "thesis_same_step",
    ThesisTwoSteps => "thesis_two_steps",
    ThesisUndo => "thesis_undo",
    ThesisUpdateShape => "thesis_update_shape",
    TooLarge => "too_large",
    Unauthorized => "unauthorized",
    UnsupportedAddress => "unsupported_address",
    UpdateBusy => "update_busy",
    UpdateChanged => "update_changed",
    UpdateCheckFailed => "update_check_failed",
    UpdateDownloadFailed => "update_download_failed",
    UpdateInstallFailed => "update_install_failed",
    UpdaterUnavailable => "updater_unavailable",
    WalletLimit => "wallet_limit",
    WorldBusy => "world_busy",
}

impl From<&str> for CommandErrorCode {
    fn from(code: &str) -> Self {
        Self::parse(code)
    }
}

impl From<String> for CommandErrorCode {
    fn from(code: String) -> Self {
        Self::parse(&code)
    }
}

impl std::fmt::Display for CommandErrorCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PartialEq<str> for CommandErrorCode {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<String> for CommandErrorCode {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for CommandErrorCode {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[cfg(test)]
mod tests {
    use super::CommandErrorCode;

    #[test]
    fn storage_version_codes_are_listed() {
        // These cross through named consts, which the literal scan below cannot see.
        for code in [
            crate::app::CATALOG_NEWER_VERSION,
            crate::app::CATALOG_UPGRADE_FAILED,
            crate::commands::LOUPIST_EARLIER_VERSION,
            crate::commands::LOUPIST_NEWER_VERSION,
        ] {
            assert!(
                !matches!(CommandErrorCode::parse(code), CommandErrorCode::Unlisted(_)),
                "{code} is listed"
            );
        }
    }

    /// Every literal code passed to an error constructor is listed.
    #[test]
    fn listed_command_error_codes() {
        let patterns = [
            regex_lite("CommandError::new("),
            regex_lite("CommandError::unavailable("),
            regex_lite("asset_error("),
            regex_lite("journal::field("),
        ];
        let mut unlisted = Vec::new();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir)
                .expect("read the source tree")
                .flatten()
            {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|ext| ext != "rs")
                    || path.ends_with("app/error_code.rs")
                {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                for (call, suffix) in &patterns {
                    for (at, _) in text.match_indices(call) {
                        let rest = text[at + call.len()..].trim_start();
                        let Some(rest) = rest.strip_prefix('"') else {
                            continue;
                        };
                        let Some(end) = rest.find('"') else { continue };
                        let code = format!("{}{suffix}", &rest[..end]);
                        if matches!(
                            CommandErrorCode::parse(&code),
                            CommandErrorCode::Unlisted(_)
                        ) {
                            unlisted.push(format!("{}: {code}", path.display()));
                        }
                    }
                }
            }
        }
        assert!(
            unlisted.is_empty(),
            "add these codes to CommandErrorCode (src/app/error_code.rs):\n{}",
            unlisted.join("\n")
        );
    }

    /// The constructor call and the suffix it adds to the literal.
    fn regex_lite(call: &'static str) -> (&'static str, &'static str) {
        (
            call,
            if call.ends_with("unavailable(") {
                "_unavailable"
            } else {
                ""
            },
        )
    }

    #[test]
    fn codes_cross_as_their_names() {
        assert_eq!(
            serde_json::to_value(CommandErrorCode::PiUnavailable).unwrap(),
            "pi_unavailable"
        );
        assert_eq!(
            serde_json::to_value(CommandErrorCode::parse("not_listed")).unwrap(),
            "not_listed"
        );
        assert_eq!(
            CommandErrorCode::from("conflict"),
            CommandErrorCode::Conflict
        );
    }
}
