//! Application-wide keep-awake while the World is processing (every Loupist).

use serde::Deserialize;
use tauri::State;

use crate::{
    app::{AppState, CommandError},
    power::{KeepAwake, KeepAwakeStatus},
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldKeepAwakeRequest {
    pub enabled: bool,
}

fn set_keep_awake(
    keep_awake: &KeepAwake,
    request: WorldKeepAwakeRequest,
) -> Result<KeepAwakeStatus, CommandError> {
    keep_awake
        .set_enabled(request.enabled)
        .map_err(super::map_store)
}

#[tauri::command(async)]
pub(crate) fn world_keep_awake_get(state: State<'_, AppState>) -> KeepAwakeStatus {
    state.graph.keep_awake().status()
}

#[tauri::command(async)]
pub(crate) fn world_keep_awake_set(
    request: WorldKeepAwakeRequest,
    state: State<'_, AppState>,
) -> Result<KeepAwakeStatus, CommandError> {
    let status = set_keep_awake(state.graph.keep_awake(), request)?;
    // The loop re-reports its demand, so a hold follows pending work at once.
    state.graph.wake();
    Ok(status)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::power::testing::FakePower;
    use crate::store::SqliteStore;

    #[test]
    fn keep_awake_set_saves_the_choice_and_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(SqliteStore::open(dir.path().join("power.sqlite3")).unwrap());
        let keep_awake = KeepAwake::with_backend(store.clone(), FakePower::new(true));
        let request = |body| serde_json::from_value::<WorldKeepAwakeRequest>(body).unwrap();
        let status =
            set_keep_awake(&keep_awake, request(serde_json::json!({"enabled": true}))).unwrap();
        assert!(status.supported && status.enabled && !status.active);
        assert!(store.keep_awake_enabled().unwrap());
        assert!(keep_awake.status().enabled);
        let status =
            set_keep_awake(&keep_awake, request(serde_json::json!({"enabled": false}))).unwrap();
        assert!(!status.enabled);
        assert!(!store.keep_awake_enabled().unwrap());
        assert!(serde_json::from_value::<WorldKeepAwakeRequest>(
            serde_json::json!({"enabled": true, "loupistId": "a"})
        )
        .is_err());
    }
}
