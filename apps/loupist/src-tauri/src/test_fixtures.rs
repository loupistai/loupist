//! Shared builders for unit tests. Tests override fields with struct update
//! syntax instead of keeping private copies of these literals.

use crate::domain::{
    memory_refs_digest, ChatMessage, ChatMessageStatus, ChatRole, ChatSession, LoupistProfile,
    MemoryPolicy, RootFilesystemIdentity,
};

/// A Loupist whose workspace path is not opened. Tests that need a real folder
/// bind one through the app's folder commands instead.
pub(crate) fn loupist_profile(id: &str) -> LoupistProfile {
    LoupistProfile {
        id: id.into(),
        name: id.into(),
        focus: String::new(),
        instructions: String::new(),
        avatar: "initial".into(),
        tone: Default::default(),
        template: None,
        workspace_root: format!("/tmp/{id}"),
        runtime_filesystem_identity: None,
        root_filesystem_identity: Some(RootFilesystemIdentity {
            device: 1,
            inode: id.bytes().fold(7u64, |hash, byte| {
                hash.wrapping_mul(31).wrapping_add(u64::from(byte))
            }),
        }),
        last_model_profile_id: None,
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

pub(crate) fn chat_session(id: &str, loupist_id: &str) -> ChatSession {
    ChatSession {
        id: id.into(),
        loupist_id: loupist_id.into(),
        pi_session_id: "123e4567-e89b-42d3-a456-426614174000".into(),
        pi_session_cache: None,
        title: "Test chat".into(),
        memory_policy: MemoryPolicy::default(),
        set_aside_memory: Vec::new(),
        versions: Vec::new(),
        messages: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
    }
}

pub(crate) fn chat_message(id: &str, role: ChatRole, content: &str) -> ChatMessage {
    ChatMessage {
        tool_refusals: Vec::new(),
        feedback: None,
        id: id.into(),
        role,
        status: ChatMessageStatus::Complete,
        content: content.into(),
        created_at_ms: 1,
        lens_receipts: Vec::new(),
        memory_refs: Vec::new(),
        observed_exact_count: 0,
        refs_truncated: false,
        refs_digest: memory_refs_digest(&[]).unwrap(),
        memory_update: None,
        memory_revision: None,
        execution_model: None,
        agent_harness: None,
        attachments: Vec::new(),
        artifact_refs: Vec::new(),
        evidence_refs: Vec::new(),
        schedule_receipts: Vec::new(),
        progress: None,
        performance: None,
        tool_trace: None,
        failure_kind: None,
        host_composed: false,
        steer: false,
        retry_after_seconds: None,
        delivery: None,
        replaced_model: None,
        as_of: None,
        number_checks: Vec::new(),
        sensitivity_cards: Vec::new(),
        as_of_report: None,
        alert: None,
        watch: None,
        earnings: None,
    }
}
