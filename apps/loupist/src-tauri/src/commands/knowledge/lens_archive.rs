use super::*;

#[derive(Debug, Deserialize)]
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
    use crate::knowledge::{KnowledgeStore, LensChangeInput, LensDocument, ReviseLens};

    #[test]
    fn method_archive_changes_listing_and_growth_but_refuses_default_perspective() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path().join("app"));
        let workspace = temp.path().join("loupist");
        std::fs::create_dir_all(&workspace).unwrap();
        seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
        let store = KnowledgeStore::for_loupist(state.store.clone(), "loupist-a").unwrap();
        let method = store
            .transact(|write| {
                let record = write.create_record(CreateRecord {
                    loupist_id: "loupist-a".into(),
                    kind: RecordKind::Lens,
                    title: "Compare cash conversion".into(),
                    at_ms: 1,
                })?;
                write.revise_lens(ReviseLens {
                    loupist_id: "loupist-a".into(),
                    record_id: record.record_id,
                    expected_revision_id: None,
                    at_ms: 2,
                    dependencies: vec![],
                    document: LensDocument {
                        markdown: "Compare cash conversion across dated periods.".into(),
                        applicability: "Quarterly research".into(),
                        assumptions: String::new(),
                        procedure: String::new(),
                        principles: String::new(),
                        limitations: String::new(),
                    },
                    change: LensChangeInput {
                        rationale: "Reusable comparison".into(),
                        references: vec![],
                        feedback: None,
                    },
                })
            })
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
            let count: i64 = tx.query_row("SELECT count(*) FROM knowledge_growth_events WHERE loupist_id = 'loupist-a' AND record_id = ?1 AND kind = 'lens_archived'", [&method.record_id], |row| row.get(0))?;
            assert_eq!(count, 1);
            tx.execute("INSERT INTO knowledge_standing_selection(loupist_id,record_id,revision_id,role,selected_at_ms) VALUES('loupist-a',?1,?2,'standing_philosophy',3) ON CONFLICT(loupist_id) DO UPDATE SET record_id=excluded.record_id,revision_id=excluded.revision_id", rusqlite::params![method.record_id, method.revision_id])?;
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
        assert!(archive(
            &state,
            LensArchiveRequest {
                loupist_id: "loupist-a".into(),
                record_id: method.record_id.clone(),
                archived: true
            }
        )
        .is_err());
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
