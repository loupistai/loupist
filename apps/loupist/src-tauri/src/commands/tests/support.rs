use super::*;
use crate::store::SqliteStore;
use std::fs;

#[cfg(unix)]
pub(in crate::commands) fn initialized_workspace(workspace: &Path, marker: &str) {
    fs::create_dir_all(workspace).unwrap();
    fs::write(workspace.join("runtime-marker"), format!("{marker}\n")).unwrap();
}

pub(in crate::commands) fn profile(id: &str, workspace: &Path, timestamp: i64) -> LoupistProfile {
    fs::create_dir_all(workspace.join(".loupist")).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    let root = BoundLoupistRoot::open_unbound(workspace.clone()).unwrap();
    LoupistProfile {
        id: id.into(),
        name: id.into(),
        focus: String::new(),
        instructions: String::new(),
        avatar: "initial".into(),
        tone: Default::default(),
        template: None,
        workspace_root: workspace.to_string_lossy().into_owned(),
        runtime_filesystem_identity: None,
        root_filesystem_identity: root.identity(),
        last_model_profile_id: None,
        created_at_ms: timestamp,
        updated_at_ms: timestamp,
    }
}

pub(in crate::commands) fn seed_profile(store: &SqliteStore, profile: &LoupistProfile) {
    store.create_loupist(profile).unwrap();
}

pub(in crate::commands) fn chat(id: &str, loupist_id: &str, timestamp: i64) -> ChatSession {
    ChatSession {
        created_at_ms: timestamp,
        updated_at_ms: timestamp,
        ..crate::test_fixtures::chat_session(id, loupist_id)
    }
}
