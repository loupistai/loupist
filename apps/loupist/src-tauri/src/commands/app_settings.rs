use tauri::State;

use crate::{
    app::{AppState, CommandError},
    store::{app_settings::AppSettings, SqliteStore},
};

fn read_notice(store: &SqliteStore) -> Result<AppSettings, CommandError> {
    store.notice_settings().map_err(super::map_store)
}

fn acknowledge_notice(store: &SqliteStore) -> Result<AppSettings, CommandError> {
    store.acknowledge_notice().map_err(super::map_store)
}

#[tauri::command]
pub(crate) fn notice_settings_get(state: State<'_, AppState>) -> Result<AppSettings, CommandError> {
    read_notice(&state.store)
}

// The host selects the version. The renderer can acknowledge the current notice,
// but cannot mark unseen future notices acknowledged or reset earlier consent.
#[tauri::command]
pub(crate) fn notice_acknowledge(state: State<'_, AppState>) -> Result<AppSettings, CommandError> {
    acknowledge_notice(&state.store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_commands_read_and_acknowledge_the_host_version() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(dir.path().join("notice.sqlite3")).unwrap();
        assert_eq!(read_notice(&store).unwrap().notice_acknowledged_version, 0);
        assert_eq!(
            acknowledge_notice(&store)
                .unwrap()
                .notice_acknowledged_version,
            1
        );
        assert_eq!(read_notice(&store).unwrap().notice_acknowledged_version, 1);
    }
}
