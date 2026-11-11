use super::*;

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LensArchiveRequest {
    pub loupist_id: String,
    pub record_id: String,
    pub archived: bool,
}

#[tauri::command]
pub fn knowledge_lens_archive(
    state: State<'_, AppState>,
    request: LensArchiveRequest,
) -> Result<(), CommandError> {
    archive(&state, request)
}

fn archive(state: &AppState, request: LensArchiveRequest) -> Result<(), CommandError> {
    let _activity = admit_mutation(state, &request.loupist_id)?;
    with_loupist_knowledge(state, &request.loupist_id, |tx, blobs| {
        let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
        let (editable, _) = write.record_permissions(&request.loupist_id, &request.record_id)?;
        if !editable {
            return Err(KnowledgeError::Invalid("this Lens is read-only"));
        }
        write.set_lens_archived(
            &request.loupist_id,
            &request.record_id,
            request.archived,
            if request.archived {
                "Archived by the user"
            } else {
                "Restored by the user"
            },
            crate::commands::now_ms(),
            "user",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tests::{profile, seed_profile};
    use crate::knowledge::test_support::user_origin;
    use crate::knowledge::KnowledgeStore;

    #[test]
    fn method_archive_changes_listing_and_growth_and_stays_with_its_loupist() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path().join("app"));
        let workspace = temp.path().join("loupist");
        std::fs::create_dir_all(&workspace).unwrap();
        seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
        let store = KnowledgeStore::for_loupist(state.store.clone(), "loupist-a").unwrap();
        let method = store
            .file_method(
                "loupist-a",
                "Compare cash conversion",
                "Compare cash conversion across dated periods.",
                &user_origin(),
                2,
            )
            .unwrap();
        archive(
            &state,
            LensArchiveRequest {
                loupist_id: "loupist-a".into(),
                record_id: method.record_id.clone(),
                archived: true,
            },
        )
        .unwrap();
        with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
            let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
            assert!(write.lens_listing("loupist-a", &method.record_id)?.1);
            assert!(write.read_page("loupist-a", &method.record_id)?.archived);
            let count: i64 = tx.query_row("SELECT count(*) FROM knowledge_growth_events WHERE loupist_id = 'loupist-a' AND record_id = ?1 AND kind = 'lens_archived'", [&method.record_id], |row| row.get(0))?;
            assert_eq!(count, 1);
            Ok(())
        }).unwrap();
        archive(
            &state,
            LensArchiveRequest {
                loupist_id: "loupist-a".into(),
                record_id: method.record_id.clone(),
                archived: false,
            },
        )
        .unwrap();
        assert!(
            !store
                .read_page("loupist-a", &method.record_id)
                .unwrap()
                .archived
        );
        assert!(archive(
            &state,
            LensArchiveRequest {
                loupist_id: "other-loupist".into(),
                record_id: method.record_id,
                archived: true
            }
        )
        .is_err());
    }
}
