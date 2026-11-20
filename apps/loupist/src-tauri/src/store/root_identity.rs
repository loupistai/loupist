//! Remount recovery requires directory generations and the very database file
//! admitted for this Loupist. Device and inode alone are not durable evidence.
use super::SqliteStore;
use crate::domain::LoupistProfile;

#[cfg(unix)]
use super::{StoreError, StoreResult};
#[cfg(unix)]
use crate::{domain::RootFilesystemIdentity, fs::loupist_root::BoundLoupistRoot};

#[cfg(unix)]
fn birth_matches(
    actual: &RootFilesystemIdentity,
    saved: &RootFilesystemIdentity,
    written_ms: i64,
) -> bool {
    let Some(birth) = actual.birth_time else {
        return false;
    };
    if birth.nanoseconds >= 1_000_000_000 {
        return false;
    }
    match saved.birth_time {
        Some(recorded) => birth == recorded,
        // Compare at full filesystem precision, failing closed within a legacy
        // row's ambiguous final millisecond rather than accepting newer data.
        None => {
            i128::from(birth.seconds) * 1_000_000_000 + i128::from(birth.nanoseconds)
                <= i128::from(written_ms) * 1_000_000
        }
    }
}

impl SqliteStore {
    pub(super) fn recover_remounted_profile(&self, profile: LoupistProfile) -> LoupistProfile {
        #[cfg(unix)]
        if self.folder_catalog {
            if let Some(recorded) = &profile.root_filesystem_identity {
                if let Ok(root) =
                    BoundLoupistRoot::open_unbound(profile.workspace_root.clone().into())
                {
                    let actual = root.identity().expect("Unix root identity");
                    if actual.device != recorded.device
                        && actual.inode == recorded.inode
                        && birth_matches(&actual, recorded, profile.updated_at_ms)
                    {
                        match self.persist_remounted_profile(&profile, &root) {
                            Ok(updated) => return updated,
                            Err(_) => {
                                // A concurrent reader may have completed this recovery.
                                if let Ok(Some(current)) = self.get_loupist_recorded(&profile.id) {
                                    if current.root_filesystem_identity.as_ref() == Some(&actual) {
                                        return current;
                                    }
                                }
                                eprintln!(
                                    "Loupist could not verify a remounted folder for {}",
                                    profile.id
                                );
                            }
                        }
                    }
                }
            }
        }
        profile
    }

    #[cfg(unix)]
    fn persist_remounted_profile(
        &self,
        expected: &LoupistProfile,
        root: &BoundLoupistRoot,
    ) -> StoreResult<LoupistProfile> {
        use super::{from_json, root_identity_columns, to_json};
        use crate::fs::{loupist_root::profile_workspace, secure_delete::SecureDeletionRoot};
        use rusqlite::{params, TransactionBehavior};
        use std::path::Path;

        expected.validate().map_err(invalid)?;
        let folders = self.folders.lock().map_err(|_| StoreError::LockPoisoned)?;
        if folders.contains_key(&expected.id) {
            return Err(StoreError::Conflict("Loupist storage is already bound"));
        }
        let dir = root.path().join(".loupist");
        let data_root = BoundLoupistRoot::open_unbound(dir.clone()).map_err(invalid)?;
        let mut database = AdmittedDatabase::open(dir.join("loupist.sqlite3"))?;
        // This connection owns the SQLite descriptor throughout admission and
        // commit. The folder stays main; only the catalog is attached to it.
        crate::knowledge::validate_folder_connection(&database.connection, &dir)
            .map_err(invalid)?;
        let local = super::loupist_profile_in(&database.connection, &expected.id)?;
        let actual_root = root.identity().expect("Unix root identity");
        if let Some(saved) = &expected.root_filesystem_identity {
            // The folder's original profile timestamp predates database
            // creation. Ordinary edits and reconnect preserve it; a folder
            // cannot advance it independently of the catalog's original value.
            if saved.birth_time.is_none()
                && local.created_at_ms > 0
                && ((expected.created_at_ms > 0 && local.created_at_ms != expected.created_at_ms)
                    || !birth_matches(&actual_root, saved, local.created_at_ms))
            {
                return Err(StoreError::Conflict(
                    "Loupist workspace is newer than its creation record",
                ));
            }
        }
        if local
            .root_filesystem_identity
            .as_ref()
            .is_none_or(|saved| !birth_matches(&actual_root, saved, expected.updated_at_ms))
        {
            return Err(StoreError::Conflict(
                "Loupist folder root creation time changed",
            ));
        }
        #[cfg(test)]
        run_after_admission_hook();
        database.ensure_same_file()?;
        let _folder_lock = crate::app::AppInstanceLock::acquire(&dir).map_err(invalid)?;
        let catalog_path = self
            .database_path()
            .ok_or(StoreError::Conflict("Loupist catalog has no path"))?;
        let app_dir = catalog_path
            .parent()
            .ok_or(StoreError::Conflict("Loupist catalog has no directory"))?;
        let private_root = SecureDeletionRoot::open(app_dir).map_err(invalid)?;
        let runtime_relative = Path::new("loupists").join(&expected.id);
        let runtime_guard = private_root
            .pin_existing_directory(&runtime_relative)
            .map_err(invalid)?;
        let runtime = runtime_guard
            .as_ref()
            .map(|guard| crate::fs::filesystem_identity::of_file(guard.file()))
            .transpose()
            .map_err(invalid)?;
        for previous in [expected, &local] {
            if let Some(saved) = &previous.runtime_filesystem_identity {
                if runtime.as_ref().is_none_or(|actual| {
                    actual.inode != saved.inode
                        // Legacy runtime evidence is the catalog cutoff only:
                        // the folder row can predate runtime allocation.
                        || !birth_matches(actual, saved, expected.updated_at_ms)
                }) {
                    return Err(StoreError::Conflict(
                        "Loupist private runtime folder changed",
                    ));
                }
            }
        }
        let mut updated = expected.clone();
        updated.root_filesystem_identity = Some(actual_root);
        if expected.runtime_filesystem_identity.is_some() {
            updated.runtime_filesystem_identity = runtime.clone();
        }
        updated.updated_at_ms = expected.updated_at_ms.saturating_add(1);
        let mut updated_local = local.clone();
        updated_local.root_filesystem_identity = updated.root_filesystem_identity.clone();
        updated_local.runtime_filesystem_identity = updated.runtime_filesystem_identity.clone();
        updated_local.updated_at_ms = local.updated_at_ms.saturating_add(1);

        let catalog = self.lock()?;
        // Both on-disk databases need rollback journals for SQLite's atomic
        // super-journal commit. The catalog mutex excludes its other callers.
        catalog.pragma_update(None, "journal_mode", "DELETE")?;
        let result: StoreResult<LoupistProfile> = (|| {
            let mut uri = tauri::Url::from_file_path(catalog_path)
                .map_err(|_| StoreError::Conflict("Loupist catalog path is invalid"))?;
            uri.set_query(Some("mode=rw"));
            database
                .connection
                .execute("ATTACH DATABASE ?1 AS catalog", [uri.as_str()])?;
            let result: StoreResult<LoupistProfile> = (|| {
                for schema in ["main", "catalog"] {
                    let mode: String = database.connection.pragma_update_and_check(
                        Some(schema),
                        "journal_mode",
                        "DELETE",
                        |row| row.get(0),
                    )?;
                    if mode != "delete" {
                        return Err(StoreError::Conflict(
                            "Loupist recovery requires rollback journals",
                        ));
                    }
                }
                database.connection.execute_batch(
                    "PRAGMA main.synchronous=FULL; PRAGMA catalog.synchronous=FULL;",
                )?;
                // A read snapshot first permits validation of an upgraded copy
                // of an older schema. Promotion to writing fails if another
                // connection changed either file while this snapshot was held.
                let transaction = database
                    .connection
                    .transaction_with_behavior(TransactionBehavior::Deferred)?;
                crate::knowledge::validate_folder_connection(&transaction, &dir)
                    .map_err(invalid)?;
                let saved: String = transaction.query_row(
                    "SELECT data_json FROM main.loupist_profiles",
                    [],
                    |row| row.get(0),
                )?;
                if from_json::<LoupistProfile>(&saved)? != local {
                    return Err(StoreError::Conflict(
                        "Loupist folder changed during admission",
                    ));
                }
                for (schema, previous, next) in [
                    ("catalog", expected, &updated),
                    ("main", &local, &updated_local),
                ] {
                    let (device, inode) = root_identity_columns(next);
                    let changed = super::map_loupist_profile_write(transaction.execute(
                        &format!("UPDATE {schema}.loupist_profiles SET root_device=?1, root_inode=?2, data_json=?3, updated_at_ms=?4 WHERE id=?5 AND data_json=?6"),
                        params![device, inode, to_json(next)?, next.updated_at_ms, next.id, to_json(previous)?]))?;
                    if changed != 1 {
                        return Err(StoreError::Conflict(
                            "Loupist changed during remount recovery",
                        ));
                    }
                }
                crate::knowledge::validate_folder_rows(&transaction, &dir).map_err(invalid)?;
                profile_workspace(&updated).map_err(invalid)?;
                let reopened_data = BoundLoupistRoot::open_unbound(dir.clone()).map_err(invalid)?;
                if reopened_data.identity() != data_root.identity() {
                    return Err(StoreError::Conflict(
                        "Loupist data folder changed during admission",
                    ));
                }
                if private_root
                    .directory_identity(&runtime_relative)
                    .map_err(invalid)?
                    != runtime
                {
                    return Err(StoreError::Conflict(
                        "Loupist private runtime changed during admission",
                    ));
                }
                // The transaction borrows the connection; use the other fields
                // and its live SQLite handle without reopening the database.
                #[cfg(test)]
                run_before_commit_hook();
                verify_database(&transaction, c"main", &database.file, &database.path)?;
                transaction.commit()?;
                Ok(updated)
            })();
            let detached = database.connection.execute_batch("DETACH DATABASE catalog");
            let updated = result?;
            detached?;
            Ok(updated)
        })();
        let restored = catalog.pragma_update(None, "journal_mode", "WAL");
        let updated = result?;
        restored?;
        Ok(updated)
    }
}

#[cfg(unix)]
fn invalid(error: impl std::fmt::Display) -> StoreError {
    StoreError::Invalid(error.to_string())
}

#[cfg(unix)]
struct AdmittedDatabase {
    file: std::fs::File,
    connection: rusqlite::Connection,
    path: std::path::PathBuf,
}

#[cfg(unix)]
impl AdmittedDatabase {
    fn open(path: std::path::PathBuf) -> StoreResult<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(invalid)?;
        if !file.metadata().map_err(invalid)?.is_file() {
            return Err(StoreError::Conflict(
                "Loupist database is not a regular file",
            ));
        }
        let connection = rusqlite::Connection::open_with_flags(
            &path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW
                | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let admitted = Self {
            file,
            connection,
            path,
        };
        admitted.ensure_same_file()?;
        Ok(admitted)
    }
    fn ensure_same_file(&self) -> StoreResult<()> {
        verify_database(&self.connection, c"main", &self.file, &self.path)
    }
}

#[cfg(unix)]
pub(super) fn verify_database(
    connection: &rusqlite::Connection,
    schema: &std::ffi::CStr,
    file: &std::fs::File,
    path: &std::path::Path,
) -> StoreResult<()> {
    use std::os::unix::fs::MetadataExt;
    let held = file.metadata().map_err(invalid)?;
    let current = std::fs::symlink_metadata(path).map_err(invalid)?;
    if !current.is_file() || current.dev() != held.dev() || current.ino() != held.ino() {
        return Err(StoreError::Conflict(
            "Loupist database changed during admission",
        ));
    }
    let mut moved: std::ffi::c_int = 0;
    // SAFETY: this connection is exclusively used on this thread, the schema is its
    // retained folder database, and SQLite writes one integer to this pointer.
    let result = unsafe {
        rusqlite::ffi::sqlite3_file_control(
            connection.handle(),
            schema.as_ptr(),
            rusqlite::ffi::SQLITE_FCNTL_HAS_MOVED,
            (&mut moved as *mut std::ffi::c_int).cast(),
        )
    };
    if result != rusqlite::ffi::SQLITE_OK || moved != 0 {
        return Err(StoreError::Conflict(
            "SQLite folder database no longer matches its held file",
        ));
    }
    Ok(())
}

#[cfg(all(test, unix))]
thread_local! {
    static AFTER_ADMISSION: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    static BEFORE_COMMIT: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}
#[cfg(all(test, unix))]
pub(super) fn run_before_commit_hook() {
    BEFORE_COMMIT.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
}
#[cfg(all(test, unix))]
pub(crate) fn with_before_commit<T>(hook: impl FnOnce() + 'static, body: impl FnOnce() -> T) -> T {
    struct Reset(Option<Box<dyn FnOnce()>>);
    impl Drop for Reset {
        fn drop(&mut self) {
            BEFORE_COMMIT.with(|hook| {
                hook.replace(self.0.take());
            });
        }
    }
    let _reset = Reset(BEFORE_COMMIT.with(|slot| slot.replace(Some(Box::new(hook)))));
    body()
}
#[cfg(all(test, unix))]
fn run_after_admission_hook() {
    AFTER_ADMISSION.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
}
#[cfg(all(test, unix))]
pub(crate) fn with_after_admission<T>(
    hook: impl FnOnce() + 'static,
    body: impl FnOnce() -> T,
) -> T {
    struct Reset(Option<Box<dyn FnOnce()>>);
    impl Drop for Reset {
        fn drop(&mut self) {
            AFTER_ADMISSION.with(|hook| {
                hook.replace(self.0.take());
            });
        }
    }
    let _reset = Reset(AFTER_ADMISSION.with(|slot| slot.replace(Some(Box::new(hook)))));
    body()
}
