use std::{fs, path::Path, sync::Arc};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use tauri::ipc::Channel;

use super::*;
use super::{
    attachments::*,
    chat_runtime::chat_send,
    loupist::{create_folder_loupist, delete_loupist_inner},
};
use crate::{
    agent_harness,
    domain::{ChatAttachment, ChatMessage, ChatMessageStatus, ChatRole, ChatSession},
    runs::coordinator::{RunKind, RunTarget},
    util::sha256,
    web::browser::validated_http_url as validated_external_url,
};

mod chat_behavior;
mod contracts;
mod home_thread;
mod home_vocabulary;
mod loupist;
mod research_handoff;
mod support;

pub(super) use support::{chat, initialized_workspace, profile, seed_profile};

#[test]
fn chat_connector_authority_seal_is_order_independent_and_tracks_credential_revisions() {
    use crate::marketplace::connector_config::ConnectorConfigRevision;

    let mut first = ChatConnectorAuthoritySeal {
        version: CHAT_CONNECTOR_AUTHORITY_SEAL_VERSION,
        bindings: vec![
            ChatConnectorBindingSeal {
                entry_id: "zeta.connector".into(),
                enabled: true,
                execute: true,
                updated_at_ms: 20,
            },
            ChatConnectorBindingSeal {
                entry_id: "alpha.connector".into(),
                enabled: false,
                execute: false,
                updated_at_ms: 10,
            },
        ],
        connectors: vec![
            ChatConnectorSeal {
                entry_id: "zeta.connector".into(),
                config_revision: ConnectorConfigRevision::Revision(
                    "11111111-1111-4111-8111-111111111111".into(),
                ),
                active_credential_revision: Some("22222222-2222-4222-8222-222222222222".into()),
            },
            ChatConnectorSeal {
                entry_id: "alpha.connector".into(),
                config_revision: ConnectorConfigRevision::Absent,
                active_credential_revision: None,
            },
        ],
    };
    let mut reordered = ChatConnectorAuthoritySeal {
        version: CHAT_CONNECTOR_AUTHORITY_SEAL_VERSION,
        bindings: first.bindings.iter().cloned().rev().collect(),
        connectors: first.connectors.iter().cloned().rev().collect(),
    };
    canonicalize_chat_connector_authority_seal(&mut first);
    canonicalize_chat_connector_authority_seal(&mut reordered);
    assert_eq!(
        chat_connector_authority_sha256(&first).unwrap(),
        chat_connector_authority_sha256(&reordered).unwrap(),
    );

    reordered.connectors[1].active_credential_revision =
        Some("33333333-3333-4333-8333-333333333333".into());
    assert_ne!(
        chat_connector_authority_sha256(&first).unwrap(),
        chat_connector_authority_sha256(&reordered).unwrap(),
    );
}
