//! Each Loupist owns a database and immutable bytes inside its selected folder.
//! The host database is only a registration catalog and shared preferences.
use super::{
    lock_store, map_loupist_profile_write, root_identity_columns, to_json, SqliteStore, StoreError,
    StoreResult,
};
use crate::{
    domain::LoupistProfile,
    fs::artifact_trust::{
        create_private_directory, ensure_private_directory, read_private_regular_file_bounded,
    },
    fs::loupist_root::{profile_workspace, BoundLoupistRoot},
};
use rusqlite::{params, Connection};
use std::io::Write;
use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::MutexGuard,
};

pub(crate) struct FolderConnection {
    connection: Connection,
    root: BoundLoupistRoot,
    database_identity: Option<crate::domain::RootFilesystemIdentity>,
    _lock: crate::app::AppInstanceLock,
}

pub(crate) enum StoreConnection<'a> {
    Global(MutexGuard<'a, Connection>),
    Folder(MutexGuard<'a, HashMap<String, FolderConnection>>, String),
}
impl Deref for StoreConnection<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        match self {
            Self::Global(c) => c,
            Self::Folder(c, id) => &c[id].connection,
        }
    }
}
impl DerefMut for StoreConnection<'_> {
    fn deref_mut(&mut self) -> &mut Connection {
        match self {
            Self::Global(c) => c,
            Self::Folder(c, id) => &mut c.get_mut(id).expect("locked Loupist").connection,
        }
    }
}
fn invalid(error: impl std::fmt::Display) -> StoreError {
    StoreError::Invalid(error.to_string())
}

/// Folder admission keeps "made by an earlier or newer build" typed for the
/// renderer.
fn validate_folder(directory: &Path) -> StoreResult<()> {
    crate::knowledge::validate_folder(directory).map_err(|error| match error {
        crate::knowledge::KnowledgeError::EarlierVersion => StoreError::EarlierVersion,
        crate::knowledge::KnowledgeError::NewerVersion => StoreError::NewerVersion,
        other => invalid(other),
    })
}

impl SqliteStore {
    pub(crate) fn open_catalog(path: impl AsRef<Path>) -> StoreResult<Self> {
        let mut store = Self::open(path)?;
        store.folder_catalog = true;
        Ok(store)
    }
    pub(crate) fn is_folder_catalog(&self) -> bool {
        self.folder_catalog
    }
    pub(crate) fn loupist_data_dir(&self, loupist_id: &str) -> StoreResult<PathBuf> {
        if !self.folder_catalog {
            return self
                .database_path()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .ok_or(StoreError::Conflict(
                    "an in-memory store has no data folder",
                ));
        }
        let profile = self
            .get_loupist(loupist_id)?
            .ok_or(StoreError::Conflict("Loupist is missing"))?;
        let root = profile_workspace(&profile).map_err(invalid)?;
        Ok(root.path().join(".loupist"))
    }
    pub(crate) fn lock_loupist(&self, loupist_id: &str) -> StoreResult<StoreConnection<'_>> {
        if !self.folder_catalog {
            return self.lock();
        }
        let dir = self.loupist_data_dir(loupist_id)?;
        let mut folders = lock_store(&self.folders)?;
        let root = BoundLoupistRoot::open_unbound(dir.clone()).map_err(invalid)?;
        if let Some(opened) = folders.get(loupist_id) {
            if opened.root.identity() != root.identity()
                || opened.root.path() != root.path()
                || opened.database_identity != database_identity(&dir.join("loupist.sqlite3"))?
            {
                return Err(StoreError::Conflict(
                    "Loupist data folder changed; reopen the folder",
                ));
            }
        } else {
            let lock = crate::app::AppInstanceLock::acquire(&dir).map_err(invalid)?;
            let database = dir.join("loupist.sqlite3");
            if !database.is_file() {
                return Err(invalid("Loupist database is missing"));
            }
            let identity = database_identity(&database)?;
            // An earlier or newer build's folder is never opened for turns,
            // briefings or Memory writes: it would half-work against its tables.
            let local = Self::open_folder(&database)?;
            if identity != database_identity(&database)? {
                return Err(StoreError::Conflict(
                    "Loupist database changed while opening",
                ));
            }
            let connection = local
                .connection
                .into_inner()
                .map_err(|_| StoreError::LockPoisoned)?;
            folders.insert(
                loupist_id.to_owned(),
                FolderConnection {
                    connection,
                    root,
                    database_identity: identity,
                    _lock: lock,
                },
            );
        }
        Ok(StoreConnection::Folder(folders, loupist_id.to_owned()))
    }
    fn attached_identity<T, E>(
        &self,
        root: &BoundLoupistRoot,
        identity: Option<&crate::domain::RootFilesystemIdentity>,
        map: impl Fn(StoreError) -> E + Copy,
        body: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        let mut catalog = self.lock().map_err(map)?;
        let data_dir = root.path();
        let database = data_dir.join("loupist.sqlite3");
        let mut uri = tauri::Url::from_file_path(&database).map_err(|_| {
            map(StoreError::Conflict(
                "Loupist folder database path is invalid",
            ))
        })?;
        uri.set_query(Some("mode=rw"));
        #[cfg(unix)]
        let held = {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&database)
                .map_err(|error| map(invalid(error)))?
        };
        let verify = |conn: &Connection| {
            let reopened = BoundLoupistRoot::open_unbound(data_dir.to_owned())
                .map_err(|error| map(invalid(error)))?;
            if reopened.identity() != root.identity()
                || database_identity(&database).map_err(map)?.as_ref() != identity
            {
                return Err(map(StoreError::Conflict(
                    "Loupist data folder changed during identity write",
                )));
            }
            #[cfg(unix)]
            super::root_identity::verify_database(conn, c"asset_folder", &held, &database)
                .map_err(map)?;
            #[cfg(not(unix))]
            let _ = conn;
            Ok(())
        };
        let catalog_mode: String = catalog
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .map_err(|error| map(StoreError::from(error)))?;
        let result = (|| {
            catalog
                .execute("ATTACH DATABASE ?1 AS asset_folder", [uri.as_str()])
                .map_err(|error| map(StoreError::from(error)))?;
            let result = (|| {
                verify(&catalog)?;
                for schema in ["main", "asset_folder"] {
                    let mode: String = catalog
                        .pragma_update_and_check(Some(schema), "journal_mode", "DELETE", |r| {
                            r.get(0)
                        })
                        .map_err(|error| map(StoreError::from(error)))?;
                    if mode != "delete" {
                        return Err(map(StoreError::Conflict(
                            "Asset identity writes require rollback journals",
                        )));
                    }
                }
                catalog
                    .execute_batch(
                        "PRAGMA main.synchronous=FULL; PRAGMA asset_folder.synchronous=FULL;",
                    )
                    .map_err(|error| map(StoreError::from(error)))?;
                let tx = catalog
                    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
                    .map_err(|error| map(StoreError::from(error)))?;
                let value = body(&tx)?;
                #[cfg(all(test, unix))]
                super::root_identity::run_before_commit_hook();
                verify(&tx)?;
                tx.commit().map_err(|error| map(StoreError::from(error)))?;
                Ok(value)
            })();
            if let Err(error) = catalog.execute_batch("DETACH DATABASE asset_folder") {
                eprintln!("could not detach folder from asset correction connection: {error}");
            }
            result
        })();
        if let Err(error) = catalog.pragma_update(None, "journal_mode", catalog_mode) {
            eprintln!("could not restore SQLite journal mode after asset correction: {error}");
        }
        result
    }

    pub(crate) fn lock_record(&self, table: &str, id: &str) -> StoreResult<StoreConnection<'_>> {
        if !self.folder_catalog {
            return self.lock();
        }
        // An already bound database can identify its records without opening
        // unrelated folders. Re-enter lock_loupist before returning it so root
        // and database identity checks still run on this fast path.
        let mut opened = std::collections::BTreeSet::new();
        let owner = {
            let folders = lock_store(&self.folders)?;
            let mut owner = None;
            for (loupist, folder) in folders.iter() {
                opened.insert(loupist.clone());
                let found: bool = match folder.connection.query_row(
                    &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=?1)"),
                    [id],
                    |row| row.get(0),
                ) {
                    Ok(found) => found,
                    Err(_) => continue,
                };
                if found {
                    owner = Some(loupist.clone());
                    break;
                }
            }
            owner
        };
        if let Some(owner) = owner {
            return self.lock_loupist(&owner);
        }
        // Table names are static host code, never renderer input. A first read
        // after restart still discovers ownership from the registered folders.
        for profile in self.list_loupists()? {
            if opened.contains(&profile.id) {
                continue;
            }
            let connection = match self.lock_loupist(&profile.id) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let found: bool = match connection.query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=?1)"),
                [id],
                |r| r.get(0),
            ) {
                Ok(found) => found,
                Err(_) => continue,
            };
            if found {
                return Ok(connection);
            }
        }
        self.lock()
    }
    /// Runs `body` on every bound Loupist database and stops at the first error
    /// it returns. Writes use this, so a caller can retry the whole pass.
    pub(crate) fn for_each_database<T>(
        &self,
        body: impl FnMut(&str, &mut Connection) -> StoreResult<T>,
    ) -> StoreResult<Vec<T>> {
        self.each_database(body, false)
    }

    /// Runs a read or scan on every bound Loupist database. One database that
    /// fails is logged and skipped, so the others still return their results
    /// (due scheduled tasks, queued jobs and other cross-Loupist listings).
    pub(crate) fn for_each_readable_database<T>(
        &self,
        body: impl FnMut(&str, &mut Connection) -> StoreResult<T>,
    ) -> StoreResult<Vec<T>> {
        self.each_database(body, true)
    }

    fn each_database<T>(
        &self,
        mut body: impl FnMut(&str, &mut Connection) -> StoreResult<T>,
        skip_failures: bool,
    ) -> StoreResult<Vec<T>> {
        if !self.folder_catalog {
            let mut connection = self.lock()?;
            return Ok(vec![body("", &mut connection)?]);
        }
        let mut result = Vec::new();
        for profile in self.list_loupists()? {
            // Missing folders stay in the catalog, but cannot run background work.
            if profile_workspace(&profile).is_err() {
                continue;
            }
            let mut connection = match self.lock_loupist(&profile.id) {
                Ok(connection) => connection,
                Err(error) => {
                    eprintln!(
                        "Loupist skipped unavailable storage for {}: {error}",
                        profile.id
                    );
                    continue;
                }
            };
            match body(&profile.id, &mut connection) {
                Ok(value) => result.push(value),
                Err(error) if skip_failures => {
                    eprintln!(
                        "Loupist skipped storage for {} in a scan: {error}",
                        profile.id
                    );
                }
                Err(error) => return Err(error),
            }
        }
        Ok(result)
    }
    pub(super) fn create_folder_loupist(&self, profile: &LoupistProfile) -> StoreResult<()> {
        let registered = self.list_loupists()?;
        if registered.iter().any(|p| {
            p.id == profile.id
                || p.workspace_root == profile.workspace_root
                || (p.root_filesystem_identity.is_some()
                    && p.root_filesystem_identity == profile.root_filesystem_identity)
        }) {
            return Err(StoreError::Conflict(
                "this Loupist or folder is already registered",
            ));
        }
        let root = profile_workspace(profile).map_err(invalid)?;
        let dir = root.path().join(".loupist");
        if dir.exists() {
            if !is_configuration_only(&dir)? {
                return Err(StoreError::Conflict(
                    "Folder already contains Loupist data; open it instead",
                ));
            }
            ensure_private_directory(&dir)?;
        } else {
            create_private_directory(&dir)?;
        }
        let _lock = crate::app::AppInstanceLock::acquire(&dir).map_err(invalid)?;
        write_project_gitignore(&dir)?;
        let database = dir.join("loupist.sqlite3");
        // Never initialize over a database that appeared after admission.
        let mut create = std::fs::OpenOptions::new();
        create.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            create.mode(0o600);
        }
        drop(create.open(&database).map_err(invalid)?);
        let local = Self::open_folder(database)?;
        local.create_loupist(profile)?;
        self.refresh_folder_registration(profile)
    }
    pub(crate) fn refresh_folder_registration(&self, profile: &LoupistProfile) -> StoreResult<()> {
        if !self.folder_catalog {
            return Ok(());
        }
        let connection = self.lock()?;
        let (device, inode) = root_identity_columns(profile);
        map_loupist_profile_write(connection.execute("INSERT INTO loupist_profiles(id,updated_at_ms,workspace_root,root_device,root_inode,data_json) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET updated_at_ms=excluded.updated_at_ms,workspace_root=excluded.workspace_root,root_device=excluded.root_device,root_inode=excluded.root_inode,data_json=excluded.data_json", params![profile.id,profile.updated_at_ms,profile.workspace_root,device,inode,to_json(profile)?]))?;
        Ok(())
    }
    pub(super) fn unregister_folder(&self, expected: &LoupistProfile) -> StoreResult<()> {
        self.folders
            .lock()
            .map_err(|_| StoreError::LockPoisoned)?
            .remove(&expected.id);
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "DELETE FROM loupist_profiles WHERE id=?1 AND data_json=?2",
            params![expected.id, to_json(expected)?],
        )?;
        if changed != 1 {
            return Err(StoreError::Conflict("Loupist changed before removal"));
        }
        transaction.execute(
            "DELETE FROM app_settings WHERE id = ?1",
            [format!("default_model:{}", expected.id)],
        )?;
        transaction.commit()?;
        Ok(())
    }
    /// Open a copied folder under its saved Loupist identity, rebinding only host
    /// paths and runtime identity. Another registered copy cannot share its ID.
    pub(crate) fn folder_profile(
        &self,
        root: &BoundLoupistRoot,
    ) -> StoreResult<Option<LoupistProfile>> {
        if !self.folder_catalog || !root.path().join(".loupist").exists() {
            return Ok(None);
        }
        let dir = root.path().join(".loupist");
        BoundLoupistRoot::open_unbound(dir.clone()).map_err(invalid)?;
        if is_configuration_only(&dir)? {
            return Ok(None);
        }
        validate_folder(&dir)?;
        let profiles = read_folder_profiles(&dir)?;
        if profiles.len() != 1 {
            return Err(invalid("A Loupist folder must contain exactly one Loupist"));
        }
        let mut profile = profiles.into_iter().next().unwrap();
        profile.validate().map_err(invalid)?;
        if self.list_loupists()?.iter().any(|p| p.id == profile.id) {
            return Err(StoreError::Conflict("This Loupist is already registered. Remove its previous registration before opening the copied folder."));
        }
        profile.workspace_root = root
            .path()
            .to_str()
            .ok_or_else(|| invalid("Loupist path must be Unicode"))?
            .to_owned();
        profile.root_filesystem_identity = root.identity();
        profile.runtime_filesystem_identity = None;
        Ok(Some(profile))
    }
    pub(super) fn rebind_folder(
        &self,
        expected: &LoupistProfile,
        updated: &LoupistProfile,
    ) -> StoreResult<()> {
        updated.validate().map_err(invalid)?;
        if expected.id != updated.id || updated.updated_at_ms <= expected.updated_at_ms {
            return Err(invalid("workspace reconnection is invalid"));
        }
        if self.get_loupist_recorded(&expected.id)?.as_ref() != Some(expected) {
            return Err(StoreError::Conflict("Loupist changed before reconnection"));
        }
        if self.list_loupists()?.iter().any(|p| {
            p.id != updated.id
                && (p.workspace_root == updated.workspace_root
                    || (p.root_filesystem_identity.is_some()
                        && p.root_filesystem_identity == updated.root_filesystem_identity))
        }) {
            return Err(StoreError::Conflict("Folder is already registered"));
        }
        let dir = Path::new(&updated.workspace_root).join(".loupist");
        validate_folder(&dir)?;
        let profiles = read_folder_profiles(&dir)?;
        if profiles.len() != 1 || profiles[0].id != updated.id {
            return Err(StoreError::Conflict(
                "Choose this Loupist's original or copied folder",
            ));
        }
        self.register_folder(updated, Some(expected))
    }

    pub(crate) fn register_existing_folder(&self, profile: &LoupistProfile) -> StoreResult<()> {
        self.register_folder(profile, None)
    }

    fn register_folder(
        &self,
        profile: &LoupistProfile,
        expected: Option<&LoupistProfile>,
    ) -> StoreResult<()> {
        let root = profile_workspace(profile).map_err(invalid)?;
        let dir = root.path().join(".loupist");
        let data_root = BoundLoupistRoot::open_unbound(dir.clone()).map_err(invalid)?;
        let mut folders = self.folders.lock().map_err(|_| StoreError::LockPoisoned)?;
        let already_locked = folders
            .get(&profile.id)
            .is_some_and(|opened| opened.root.identity() == data_root.identity());
        let _lock = if already_locked {
            None
        } else {
            Some(crate::app::AppInstanceLock::acquire(&dir).map_err(invalid)?)
        };
        let database = dir.join("loupist.sqlite3");
        let identity = database_identity(&database)?;
        validate_folder(&dir)?;
        let local = Self::open_folder(&database)?;
        // The retained connection and attached write must identify the same
        // file admitted above. The transaction rereads all cards in that file.
        if identity != database_identity(&database)? {
            return Err(StoreError::Conflict(
                "Loupist database changed while opening",
            ));
        }
        self.attached_identity(&data_root, identity.as_ref(), |error| error, |tx| {
            let old = super::load_json_by_id::<LoupistProfile>(tx, "main.loupist_profiles", &profile.id)?;
            if old.as_ref() != expected {
                return Err(StoreError::Conflict("Loupist changed before registration"));
            }
            let stored: String = tx.query_row(
                "SELECT id FROM asset_folder.loupist_profiles WHERE (SELECT count(*) FROM asset_folder.loupist_profiles)=1", [], |row| row.get(0)
            )?;
            if stored != profile.id {
                return Err(StoreError::Conflict("Choose this Loupist's original or copied folder"));
            }
            let (device, inode) = root_identity_columns(profile);
            let data = to_json(profile)?;
            map_loupist_profile_write(tx.execute(
                "UPDATE asset_folder.loupist_profiles SET workspace_root=?2,updated_at_ms=?3,
                 root_device=?4,root_inode=?5,data_json=?6 WHERE id=?1",
                params![profile.id,profile.workspace_root,profile.updated_at_ms,device,inode,data]))?;
            map_loupist_profile_write(tx.execute(
                "INSERT INTO main.loupist_profiles(id,workspace_root,updated_at_ms,root_device,root_inode,data_json)
                 VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET workspace_root=excluded.workspace_root,
                 updated_at_ms=excluded.updated_at_ms, root_device=excluded.root_device,
                 root_inode=excluded.root_inode,data_json=excluded.data_json",
                params![profile.id,profile.workspace_root,profile.updated_at_ms,device,inode,data]))?;
            crate::scheduled_tasks::store::pause_copied_tasks(tx, "asset_folder")?;
            drop_pi_session_caches_in(tx, None, "asset_folder")?;
            crate::knowledge::pause_copied_processing(tx, &profile.id, "asset_folder").map_err(invalid)?;
            Ok(())
        })?;
        drop(local);
        folders.remove(&profile.id);
        if let Err(error) = write_project_gitignore(&dir) {
            eprintln!("could not update copied folder's gitignore: {error}");
        }
        Ok(())
    }
}

/// Drops every derived Pi cache of a Loupist's Chats (all Chats when `None`),
/// so the next turn rebuilds from the canonical transcript. The key keeps its
/// place with a `null` value, as `ChatSession` writes it: a Chat's
/// compare-and-swap compares the stored text with the re-encoded Chat, so
/// removing the key would leave every Chat unwritable.
#[cfg(test)]
pub(crate) fn drop_pi_session_caches(
    connection: &Connection,
    loupist_id: Option<&str>,
) -> StoreResult<()> {
    drop_pi_session_caches_in(connection, loupist_id, "main")
}

fn drop_pi_session_caches_in(
    connection: &Connection,
    loupist_id: Option<&str>,
    schema: &str,
) -> StoreResult<()> {
    connection.execute(
        &format!("UPDATE {schema}.chat_sessions SET data_json = json_replace(data_json, '$.piSessionCache', NULL)
         WHERE ?1 IS NULL OR loupist_id = ?1"),
        [loupist_id],
    )?;
    Ok(())
}

/// A checked-in configuration folder may precede the first local Loupist DB.
/// Anything else is treated as existing research, so a missing DB cannot be
/// silently replaced after corruption or a partial folder copy.
fn is_configuration_only(directory: &Path) -> StoreResult<bool> {
    BoundLoupistRoot::open_unbound(directory.to_path_buf()).map_err(invalid)?;
    for entry in std::fs::read_dir(directory).map_err(invalid)? {
        let entry = entry.map_err(invalid)?;
        let metadata = std::fs::symlink_metadata(entry.path()).map_err(invalid)?;
        if metadata.file_type().is_symlink() {
            return Err(invalid("Loupist configuration entries cannot be symlinks"));
        }
        #[cfg(windows)]
        if crate::fs::windows::metadata_is_reparse(&metadata) {
            return Err(invalid(
                "Loupist configuration entries cannot be reparse points",
            ));
        }
        match entry.file_name().to_str() {
            Some("config.toml" | "AGENTS.md" | ".gitignore") if metadata.is_file() => {}
            Some("skills") if metadata.is_dir() => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

fn write_project_gitignore(directory: &Path) -> StoreResult<()> {
    const RULES: &str = "*\n!.gitignore\n!config.toml\n!AGENTS.md\n!skills/\n!skills/**\n";
    let path = directory.join(".gitignore");
    let existing = match std::fs::symlink_metadata(&path) {
        Ok(_) => read_private_regular_file_bounded(&path, 64 * 1024)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(invalid(error)),
    };
    let existing = std::str::from_utf8(&existing).map_err(invalid)?;
    if existing.ends_with(RULES) {
        return Ok(());
    }
    let content = if existing.is_empty() || existing == "*\n" {
        RULES.to_owned()
    } else {
        format!(
            "{}\n# Loupist: share configuration, keep research private.\n{RULES}",
            existing.trim_end()
        )
    };
    let mut temporary = tempfile::NamedTempFile::new_in(directory).map_err(invalid)?;
    temporary.write_all(content.as_bytes()).map_err(invalid)?;
    temporary.as_file().sync_all().map_err(invalid)?;
    temporary.persist(path).map_err(invalid)?;
    Ok(())
}

fn read_folder_profiles(directory: &Path) -> StoreResult<Vec<LoupistProfile>> {
    let connection = Connection::open_with_flags(
        directory.join("loupist.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    let mut statement = connection.prepare("SELECT data_json FROM loupist_profiles")?;
    let rows = statement.query_map([], |r| r.get::<_, String>(0))?;
    rows.map(|row| super::from_json(&row?)).collect()
}

fn database_identity(path: &Path) -> StoreResult<Option<crate::domain::RootFilesystemIdentity>> {
    let metadata = std::fs::symlink_metadata(path).map_err(invalid)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid("Loupist database must be a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(invalid)?;
        Ok(Some(
            crate::fs::filesystem_identity::of_file(&file).map_err(invalid)?,
        ))
    }
    #[cfg(windows)]
    {
        let file = crate::fs::windows::open_regular_no_reparse(path).map_err(invalid)?;
        Ok(Some(
            crate::fs::windows::filesystem_identity(&file).map_err(invalid)?,
        ))
    }
    #[cfg(not(any(unix, windows)))]
    {
        Ok(None)
    }
}
