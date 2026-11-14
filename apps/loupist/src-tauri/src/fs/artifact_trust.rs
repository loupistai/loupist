use std::{
    fs::{File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};

use sha2::{Digest, Sha256};

#[cfg(all(target_os = "macos", any(not(debug_assertions), test)))]
use std::process::{Command, Stdio};
use thiserror::Error;

#[cfg(windows)]
use crate::fs::windows::{
    add_open_reparse_point_flag, add_open_reparse_point_flag_with_read_write_share,
    ensure_no_reparse_points, filesystem_identity, metadata_is_reparse, open_directory_no_reparse,
    open_parent_directories_no_reparse, reopen_regular_no_reparse_for_identity,
};
#[cfg(all(not(debug_assertions), windows))]
use crate::fs::windows::{authenticode_signer_certificate, open_regular_no_reparse};

static APP_EXECUTABLE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// The running app's executable with every symlink resolved, fixed on first use
/// (`AppState::initialize`, before any helper is found). The bundled resources
/// and the helper signature check both derive from this one path.
///
/// macOS reports the path the app was opened by, which may pass through a
/// symlink such as `/tmp` → `/private/tmp`. Tauri refuses such a path, which
/// left the app without its resources; resolving it here does not widen trust,
/// because every helper still runs only after codesign verifies the real files
/// inside this bundle and their signer matches this executable's.
pub fn app_executable() -> io::Result<PathBuf> {
    if let Some(path) = APP_EXECUTABLE.get() {
        return Ok(path.clone());
    }
    let path = std::env::current_exe()?.canonicalize()?;
    Ok(APP_EXECUTABLE.get_or_init(|| path).clone())
}

/// The `Contents/Resources` directory of the macOS app bundle that holds
/// `executable`, after resolving symlinks; `None` outside an app bundle.
pub fn bundle_resources(executable: &Path) -> io::Result<Option<PathBuf>> {
    let executable = executable.canonicalize()?;
    let macos = executable.parent();
    let contents = macos.and_then(Path::parent);
    let bundle = contents.and_then(Path::parent);
    let in_bundle = macos.and_then(Path::file_name) == Some("MacOS".as_ref())
        && contents.and_then(Path::file_name) == Some("Contents".as_ref())
        && bundle.and_then(Path::extension) == Some("app".as_ref());
    Ok(contents
        .filter(|_| in_bundle)
        .map(|contents| contents.join("Resources")))
}

#[derive(Debug, Error)]
pub enum ArtifactTrustError {
    #[error("artifact I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("artifact is not part of the trusted Loupist bundle")]
    Untrusted,
    #[error("artifact exceeds its bounded read contract")]
    Oversized,
}

/// Keeps the exact verified Windows executable non-replaceable until the
/// caller has crossed `Command::spawn`. Other platforms need no extra handle.
#[must_use]
pub struct VerifiedExecutable {
    #[cfg(windows)]
    _opened: File,
    #[cfg(windows)]
    _parent_directories: Vec<File>,
}

/// Creates temporary storage with no group or other access, even under a permissive umask.
#[cfg(target_os = "macos")]
pub(crate) fn private_temporary_directory(prefix: &str) -> io::Result<tempfile::TempDir> {
    tempfile::Builder::new()
        .prefix(prefix)
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
}

/// Creates or hardens one app-owned directory. On Unix the final component is
/// opened without following symlinks and held at mode 0700. The app data root
/// and every direct child pass through this boundary before any state is used.
pub(crate) fn ensure_private_directory(path: &Path) -> Result<(), ArtifactTrustError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata_is_untrusted(&metadata) || !metadata.is_dir() {
                return Err(ArtifactTrustError::Untrusted);
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            #[cfg(windows)]
            ensure_existing_ancestry_trusted(path)?;
            #[cfg(unix)]
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(path)?;
            #[cfg(not(unix))]
            std::fs::create_dir_all(path)?;
        }
        Err(error) => return Err(error.into()),
    }
    harden_private_directory(path)
}

/// Creates a new app-owned directory and refuses pre-existing destinations.
pub(crate) fn create_private_directory(path: &Path) -> Result<(), ArtifactTrustError> {
    #[cfg(windows)]
    ensure_existing_ancestry_trusted(path)?;
    #[cfg(unix)]
    std::fs::DirBuilder::new().mode(0o700).create(path)?;
    #[cfg(not(unix))]
    std::fs::create_dir(path)?;
    harden_private_directory(path)
}

/// Creates or hardens one app-owned regular file. Existing symlinks and
/// special files are rejected before opening; Unix files are held at 0600.
pub(crate) fn ensure_private_regular_file(path: &Path) -> Result<(), ArtifactTrustError> {
    #[cfg(windows)]
    ensure_existing_ancestry_trusted(path)?;
    let exists = match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata_is_untrusted(&metadata) || !metadata.is_file() {
                return Err(ArtifactTrustError::Untrusted);
            }
            true
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };

    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if !exists {
        #[cfg(windows)]
        ensure_existing_ancestry_trusted(path)?;
        options.create_new(true);
    }
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    #[cfg(windows)]
    add_open_reparse_point_flag_with_read_write_share(&mut options);
    let file = options.open(path)?;
    if !file.metadata()?.is_file() || metadata_is_untrusted(&file.metadata()?) {
        return Err(ArtifactTrustError::Untrusted);
    }
    #[cfg(unix)]
    {
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        if file.metadata()?.permissions().mode() & 0o777 != 0o600 {
            return Err(ArtifactTrustError::Untrusted);
        }
    }
    reject_changed_path(path, &file, false)
}

/// Reads one bounded app-owned regular file without following a replaced path.
pub(crate) fn read_private_regular_file_bounded(
    path: &Path,
    max_bytes: u64,
) -> Result<Vec<u8>, ArtifactTrustError> {
    #[cfg(windows)]
    ensure_no_reparse_points(path).map_err(|_| ArtifactTrustError::Untrusted)?;
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata_is_untrusted(&metadata) || !metadata.is_file() {
        return Err(ArtifactTrustError::Untrusted);
    }
    if metadata.len() > max_bytes {
        return Err(ArtifactTrustError::Oversized);
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
    #[cfg(windows)]
    add_open_reparse_point_flag(&mut options);
    let mut file = options.open(path)?;
    reject_changed_path(path, &file, false)?;
    if file.metadata()?.len() > max_bytes {
        return Err(ArtifactTrustError::Oversized);
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(ArtifactTrustError::Oversized);
    }
    Ok(bytes)
}

/// Hardens an existing SQLite side file without creating one that SQLite has
/// not requested. This is called both before and after connection setup.
pub(crate) fn harden_private_regular_file_if_exists(path: &Path) -> Result<(), ArtifactTrustError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => ensure_private_regular_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Hashes the exact bytes of a bounded regular file through a no-follow file
/// descriptor. The returned value contains neither its path nor any metadata.
pub(crate) fn digest_bounded_regular_file(
    path: &Path,
    max_bytes: u64,
) -> Result<String, ArtifactTrustError> {
    #[cfg(windows)]
    ensure_no_reparse_points(path).map_err(|_| ArtifactTrustError::Untrusted)?;
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata_is_untrusted(&metadata) || !metadata.is_file() || metadata.len() > max_bytes {
        return Err(if metadata.len() > max_bytes {
            ArtifactTrustError::Oversized
        } else {
            ArtifactTrustError::Untrusted
        });
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    #[cfg(windows)]
    add_open_reparse_point_flag(&mut options);
    let mut file = options.open(path)?;
    reject_changed_path(path, &file, false)?;
    if file.metadata()?.len() > max_bytes {
        return Err(ArtifactTrustError::Oversized);
    }

    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > max_bytes {
            return Err(ArtifactTrustError::Oversized);
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn harden_private_directory(path: &Path) -> Result<(), ArtifactTrustError> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata_is_untrusted(&metadata) || !metadata.is_dir() {
        return Err(ArtifactTrustError::Untrusted);
    }

    #[cfg(unix)]
    let file = {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW);
        let file = options.open(path)?;
        if !file.metadata()?.is_dir() {
            return Err(ArtifactTrustError::Untrusted);
        }
        file.set_permissions(std::fs::Permissions::from_mode(0o700))?;
        if file.metadata()?.permissions().mode() & 0o777 != 0o700 {
            return Err(ArtifactTrustError::Untrusted);
        }
        file
    };
    #[cfg(unix)]
    {
        reject_changed_path(path, &file, true)
    }
    #[cfg(windows)]
    {
        let file = open_directory_no_reparse(path)?;
        reject_changed_path(path, &file, true)
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata_is_untrusted(&metadata) || !metadata.is_dir() {
            return Err(ArtifactTrustError::Untrusted);
        }
        Ok(())
    }
}

fn reject_changed_path(
    path: &Path,
    opened: &File,
    expect_directory: bool,
) -> Result<(), ArtifactTrustError> {
    let path_metadata = std::fs::symlink_metadata(path)?;
    let opened_metadata = opened.metadata()?;
    let expected_type = if expect_directory {
        path_metadata.is_dir() && opened_metadata.is_dir()
    } else {
        path_metadata.is_file() && opened_metadata.is_file()
    };
    if metadata_is_untrusted(&path_metadata)
        || metadata_is_untrusted(&opened_metadata)
        || !expected_type
    {
        return Err(ArtifactTrustError::Untrusted);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        if path_metadata.dev() != opened_metadata.dev()
            || path_metadata.ino() != opened_metadata.ino()
        {
            return Err(ArtifactTrustError::Untrusted);
        }
    }
    #[cfg(windows)]
    {
        let reopened = if expect_directory {
            open_directory_no_reparse(path)?
        } else {
            reopen_regular_no_reparse_for_identity(path)?
        };
        if filesystem_identity(opened)? != filesystem_identity(&reopened)? {
            return Err(ArtifactTrustError::Untrusted);
        }
    }
    Ok(())
}

/// Validates an executable immediately before use. Development builds require
/// a regular, non-link/reparse file. A macOS release additionally requires the
/// helper to remain inside the current signed app bundle, the whole bundle and
/// helper signatures to validate, and both executables to share the app's Team ID
/// or (for a teamless app) its actual leaf certificate. Ad-hoc apps fail closed. A
/// Windows release requires the native Authenticode trust policy to accept it.
pub fn verify_executable(path: &Path) -> Result<VerifiedExecutable, ArtifactTrustError> {
    #[cfg(windows)]
    ensure_no_reparse_points(path).map_err(|_| ArtifactTrustError::Untrusted)?;
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata_is_untrusted(&metadata) || !metadata.is_file() {
        return Err(ArtifactTrustError::Untrusted);
    }

    #[cfg(windows)]
    let opened = {
        let mut options = OpenOptions::new();
        options.read(true);
        add_open_reparse_point_flag(&mut options);
        let opened = options.open(path)?;
        reject_changed_path(path, &opened, false)?;
        opened
    };
    #[cfg(windows)]
    let parent_directories =
        open_parent_directories_no_reparse(path).map_err(|_| ArtifactTrustError::Untrusted)?;
    #[cfg(windows)]
    // Recheck after all ancestor handles are retained. This closes the small
    // gap between the first identity check and pinning the path used by
    // `Command::spawn`.
    reject_changed_path(path, &opened, false)?;

    #[cfg(all(not(debug_assertions), target_os = "macos"))]
    verify_macos_distribution_identity(path)?;

    #[cfg(all(not(debug_assertions), windows))]
    {
        let helper_signer =
            authenticode_signer_certificate(path, &opened).ok_or(ArtifactTrustError::Untrusted)?;
        let current_path = std::env::current_exe()?;
        ensure_no_reparse_points(&current_path).map_err(|_| ArtifactTrustError::Untrusted)?;
        let current =
            open_regular_no_reparse(&current_path).map_err(|_| ArtifactTrustError::Untrusted)?;
        let app_signer = authenticode_signer_certificate(&current_path, &current)
            .ok_or(ArtifactTrustError::Untrusted)?;
        if helper_signer != app_signer {
            return Err(ArtifactTrustError::Untrusted);
        }
    }

    Ok(VerifiedExecutable {
        #[cfg(windows)]
        _opened: opened,
        #[cfg(windows)]
        _parent_directories: parent_directories,
    })
}

fn metadata_is_untrusted(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        if metadata_is_reparse(metadata) {
            return true;
        }
    }
    false
}

#[cfg(windows)]
fn ensure_existing_ancestry_trusted(path: &Path) -> Result<(), ArtifactTrustError> {
    let mut ancestor = path.parent().ok_or(ArtifactTrustError::Untrusted)?;
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or(ArtifactTrustError::Untrusted)?;
    }
    ensure_no_reparse_points(ancestor).map_err(|_| ArtifactTrustError::Untrusted)
}

#[cfg(all(not(debug_assertions), target_os = "macos"))]
fn verify_macos_distribution_identity(helper: &Path) -> Result<(), ArtifactTrustError> {
    verify_macos_identity_for_app(&app_executable()?, helper)
}

#[cfg(all(target_os = "macos", any(not(debug_assertions), test)))]
fn verify_macos_identity_for_app(current: &Path, helper: &Path) -> Result<(), ArtifactTrustError> {
    let metadata = std::fs::symlink_metadata(helper)?;
    if metadata_is_untrusted(&metadata) || !metadata.is_file() {
        return Err(ArtifactTrustError::Untrusted);
    }
    let current_executable = current.canonicalize()?;
    let app_bundle = current_executable
        .ancestors()
        .find(|path| path.extension().and_then(|value| value.to_str()) == Some("app"))
        .ok_or(ArtifactTrustError::Untrusted)?
        .canonicalize()?;
    let helper = helper.canonicalize()?;
    if !helper.starts_with(&app_bundle) {
        return Err(ArtifactTrustError::Untrusted);
    }
    verify_codesign(&app_bundle, true)?;
    verify_codesign(&helper, false)?;
    let app_identity = macos_signing_identity(&current_executable)?;
    let helper_identity = macos_signing_identity(&helper)?;
    require_macos_matching_signer(&app_identity, &helper_identity)
}

#[cfg(any(test, all(not(debug_assertions), target_os = "macos")))]
fn require_macos_matching_signer(
    app: &MacosSigningIdentity,
    helper: &MacosSigningIdentity,
) -> Result<(), ArtifactTrustError> {
    if app != helper {
        return Err(ArtifactTrustError::Untrusted);
    }
    Ok(())
}

#[cfg(any(test, all(not(debug_assertions), target_os = "macos")))]
#[derive(Debug, PartialEq, Eq)]
enum MacosSigningIdentity {
    Team(String),
    Certificate(String),
}

#[cfg(any(test, all(not(debug_assertions), target_os = "macos")))]
fn parse_macos_team(details: &str) -> Result<Option<String>, ArtifactTrustError> {
    let teams: Vec<_> = details
        .lines()
        .filter_map(|line| line.strip_prefix("TeamIdentifier="))
        .collect();
    match teams.as_slice() {
        ["not set"] => Ok(None),
        [team]
            if team.len() == 10
                && team
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit()) =>
        {
            Ok(Some((*team).to_owned()))
        }
        _ => Err(ArtifactTrustError::Untrusted),
    }
}

#[cfg(all(target_os = "macos", any(not(debug_assertions), test)))]
fn macos_signing_identity(path: &Path) -> Result<MacosSigningIdentity, ArtifactTrustError> {
    let output = Command::new("/usr/bin/codesign")
        .env_clear()
        .args(["--display", "--verbose=4"])
        .arg(path)
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(ArtifactTrustError::Untrusted);
    }
    if let Some(team) = parse_macos_team(&String::from_utf8_lossy(&output.stderr))? {
        return Ok(MacosSigningIdentity::Team(team));
    }
    // Read the actual embedded leaf, never the designated requirement text or
    // a compiled-in certificate pin. The private directory prevents path swaps;
    // no keychain, trust setting or certificate from the user's profile is used.
    let temporary = private_temporary_directory("loupist-signature-")?;
    let prefix = temporary.path().join("certificate");
    let mut extract = std::ffi::OsString::from("--extract-certificates=");
    extract.push(&prefix);
    let status = Command::new("/usr/bin/codesign")
        .env_clear()
        .arg("--display")
        .arg(extract)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(ArtifactTrustError::Untrusted);
    }
    let leaf = temporary.path().join("certificate0");
    let digest =
        digest_bounded_regular_file(&leaf, 64 * 1024).map_err(|_| ArtifactTrustError::Untrusted)?;
    if std::fs::metadata(&leaf)?.len() == 0 {
        return Err(ArtifactTrustError::Untrusted);
    }
    Ok(MacosSigningIdentity::Certificate(digest))
}

#[cfg(all(target_os = "macos", any(not(debug_assertions), test)))]
fn verify_codesign(path: &Path, deep: bool) -> Result<(), ArtifactTrustError> {
    let mut command = Command::new("/usr/bin/codesign");
    command.env_clear().args(["--verify", "--strict"]);
    if deep {
        command.arg("--deep");
    }
    let status = command
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(ArtifactTrustError::Untrusted)
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "artifact_trust_macos_tests.rs"]
mod macos_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn bundle_resources_resolve_through_a_symlinked_folder_to_the_real_bundle() {
        let root = tempfile::tempdir().unwrap();
        let real = root.path().join("private").join("Loupist.app");
        std::fs::create_dir_all(real.join("Contents/MacOS")).unwrap();
        std::fs::create_dir_all(real.join("Contents/Resources/pi-runtime")).unwrap();
        std::fs::write(real.join("Contents/MacOS/loupist"), b"app").unwrap();
        // Like /tmp -> /private/tmp: the app is opened through a symlinked ancestor.
        let linked = root.path().join("tmp");
        std::os::unix::fs::symlink(root.path().join("private"), &linked).unwrap();
        let opened = linked.join("Loupist.app/Contents/MacOS/loupist");
        assert!(opened.ancestors().any(|ancestor| ancestor
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())));

        let resources = bundle_resources(&opened).unwrap().unwrap();
        assert_eq!(
            resources,
            real.join("Contents/Resources").canonicalize().unwrap()
        );
        assert!(resources.join("pi-runtime").is_dir());
        assert!(!resources.ancestors().any(|ancestor| ancestor
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink())));

        // Outside an app bundle (a development build) there is no bundle answer.
        std::fs::create_dir_all(root.path().join("target/debug")).unwrap();
        std::fs::write(root.path().join("target/debug/loupist"), b"dev").unwrap();
        assert_eq!(
            bundle_resources(&root.path().join("target/debug/loupist")).unwrap(),
            None
        );
        assert!(bundle_resources(&root.path().join("missing/loupist")).is_err());
    }

    #[test]
    fn the_app_executable_is_resolved_once_and_has_no_symlink() {
        let first = app_executable().unwrap();
        assert_eq!(
            first,
            std::env::current_exe().unwrap().canonicalize().unwrap()
        );
        assert_eq!(app_executable().unwrap(), first);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn temporary_directories_are_created_owner_only() {
        let temporary = private_temporary_directory("loupist-private-test-").unwrap();
        assert_eq!(
            temporary.path().metadata().unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn macos_signer_identity_negative_corpus() {
        let team = MacosSigningIdentity::Team("ABCDE12345".into());
        let certificate = MacosSigningIdentity::Certificate("a".repeat(64));
        require_macos_matching_signer(&team, &MacosSigningIdentity::Team("ABCDE12345".into()))
            .unwrap();
        require_macos_matching_signer(
            &certificate,
            &MacosSigningIdentity::Certificate("a".repeat(64)),
        )
        .unwrap();
        for (app, helper) in [
            (team, MacosSigningIdentity::Team("OTHER12345".into())),
            (
                certificate,
                MacosSigningIdentity::Certificate("b".repeat(64)),
            ),
            (
                MacosSigningIdentity::Team("ABCDE12345".into()),
                MacosSigningIdentity::Certificate("a".repeat(64)),
            ),
            (
                MacosSigningIdentity::Certificate("a".repeat(64)),
                MacosSigningIdentity::Team("ABCDE12345".into()),
            ),
        ] {
            assert!(require_macos_matching_signer(&app, &helper).is_err());
        }
        assert_eq!(parse_macos_team("TeamIdentifier=not set").unwrap(), None);
        assert_eq!(
            parse_macos_team("TeamIdentifier=ABCDE12345")
                .unwrap()
                .as_deref(),
            Some("ABCDE12345")
        );
        for details in [
            "",
            "TeamIdentifier=",
            "TeamIdentifier=abcde12345",
            "TeamIdentifier=bad",
            "TeamIdentifier=not set\nTeamIdentifier=ABCDE12345",
            "TeamIdentifier=ABCDE12345\nTeamIdentifier=ABCDE12345",
        ] {
            assert!(parse_macos_team(details).is_err(), "accepted {details:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn development_validation_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let executable = temporary.path().join("worker");
        let link = temporary.path().join("worker-link");
        std::fs::write(&executable, b"worker").unwrap();
        symlink(&executable, &link).unwrap();

        let _verified = verify_executable(&executable).unwrap();
        assert!(matches!(
            verify_executable(&link),
            Err(ArtifactTrustError::Untrusted)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn private_paths_are_exact_and_symlinks_fail_closed() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join("private");
        ensure_private_directory(&directory).unwrap();
        assert_eq!(
            std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );

        let file = directory.join("state");
        ensure_private_regular_file(&file).unwrap();
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );

        let link = temporary.path().join("private-link");
        symlink(&directory, &link).unwrap();
        assert!(matches!(
            ensure_private_directory(&link),
            Err(ArtifactTrustError::Untrusted)
        ));
    }

    #[test]
    fn bounded_digest_tracks_exact_regular_file_bytes() {
        let temporary = tempfile::tempdir().unwrap();
        let artifact = temporary.path().join("runtime");
        std::fs::write(&artifact, b"runtime-a").unwrap();
        let first = digest_bounded_regular_file(&artifact, 32).unwrap();
        std::fs::write(&artifact, b"runtime-b").unwrap();
        let second = digest_bounded_regular_file(&artifact, 32).unwrap();
        assert_ne!(first, second);
        assert!(matches!(
            digest_bounded_regular_file(&artifact, 4),
            Err(ArtifactTrustError::Oversized)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn private_regular_file_can_be_hardened_repeatedly() {
        let temporary = tempfile::tempdir().unwrap();
        let file = temporary.path().join("state");

        ensure_private_regular_file(&file).unwrap();
        ensure_private_regular_file(&file).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn private_regular_file_can_be_hardened_while_sqlite_is_open() {
        let temporary = tempfile::tempdir().unwrap();
        let file = temporary.path().join("state.sqlite3");

        ensure_private_regular_file(&file).unwrap();
        let connection = rusqlite::Connection::open(&file).unwrap();
        ensure_private_regular_file(&file).unwrap();
        drop(connection);
    }

    #[cfg(windows)]
    #[test]
    fn verified_executable_blocks_writes_and_deletion_while_held() {
        let temporary = tempfile::tempdir().unwrap();
        let executable = temporary.path().join("worker");
        std::fs::write(&executable, b"worker").unwrap();

        let verified = verify_executable(&executable).unwrap();
        assert!(OpenOptions::new().write(true).open(&executable).is_err());
        assert!(std::fs::remove_file(&executable).is_err());
        drop(verified);

        OpenOptions::new().write(true).open(&executable).unwrap();
        std::fs::remove_file(&executable).unwrap();
    }
}
