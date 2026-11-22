//! Explicit Files-panel action. Open the checked regular file through a
//! document handler. Never pass an unvalidated workspace path to a shell.
use std::{
    fs::File,
    io::{self, Read, Seek},
    path::Path,
};

use super::{loupist_root::BoundLoupistRoot, workspace_files};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct WorkspaceDocumentRequest {
    pub loupist_id: String,
    pub path: String,
}

#[derive(Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum WorkspaceDocumentOpen {
    Requested {
        application_name: Option<String>,
    },
    Refused {
        reason: DocumentRefusal,
    },
    HandlerUnavailable {
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        application_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        reason: Option<String>,
    },
}

#[derive(Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum WorkspaceDocumentHandler {
    Allowed { application_name: Option<String> },
    Refused { reason: DocumentRefusal },
}

#[derive(Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DocumentRefusal {
    OutsideWorkspace,
    UnsupportedType,
    InaccessibleFile,
    UnsafeDocument,
}

fn admit(root: &BoundLoupistRoot, input: &str) -> Result<(String, File), DocumentRefusal> {
    use DocumentRefusal::*;
    let relative = workspace_files::relative_path(input).map_err(|_| OutsideWorkspace)?;
    let path = Path::new(&relative);
    if path
        .ancestors()
        .filter_map(Path::extension)
        .any(|ext| ext.eq_ignore_ascii_case("app"))
    {
        return Err(UnsafeDocument);
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(
        ext.as_str(),
        "xlsx"
            | "xlsm"
            | "xls"
            | "csv"
            | "tsv"
            | "docx"
            | "pptx"
            | "pdf"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "svg"
            | "txt"
            | "md"
            | "json"
    ) {
        return Err(UnsupportedType);
    }
    let mut file =
        workspace_files::open_relative(root, &relative, false).map_err(|_| InaccessibleFile)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if file
            .metadata()
            .map_err(|_| InaccessibleFile)?
            .permissions()
            .mode()
            & 0o111
            != 0
        {
            return Err(UnsafeDocument);
        }
    }
    #[cfg(target_os = "macos")]
    if mac_file_redirects(&file).map_err(|_| InaccessibleFile)? {
        return Err(UnsafeDocument);
    }
    let mut prefix = Vec::new();
    (&mut file)
        .take(8192)
        .read_to_end(&mut prefix)
        .map_err(|_| InaccessibleFile)?;
    let safe = match ext.as_str() {
        "xlsx" | "xlsm" | "docx" | "pptx" => {
            prefix.starts_with(b"PK\x03\x04")
                || prefix.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1")
        }
        "xls" => prefix.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1"),
        "pdf" => prefix.starts_with(b"%PDF-"),
        "png" => workspace_files::raster_media_type(&prefix) == Some("image/png"),
        "jpg" | "jpeg" => workspace_files::raster_media_type(&prefix) == Some("image/jpeg"),
        "gif" => workspace_files::raster_media_type(&prefix) == Some("image/gif"),
        "webp" => workspace_files::raster_media_type(&prefix) == Some("image/webp"),
        _ => safe_text(&prefix),
    };
    if !safe {
        return Err(UnsafeDocument);
    }
    file.rewind().map_err(|_| InaccessibleFile)?;
    Ok((relative, file))
}

fn safe_text(prefix: &[u8]) -> bool {
    let decoded;
    let text = if prefix.starts_with(&[0xff, 0xfe]) || prefix.starts_with(&[0xfe, 0xff]) {
        let little = prefix[0] == 0xff;
        decoded = String::from_utf16_lossy(
            &prefix[2..]
                .chunks_exact(2)
                .map(|b| {
                    if little {
                        u16::from_le_bytes([b[0], b[1]])
                    } else {
                        u16::from_be_bytes([b[0], b[1]])
                    }
                })
                .collect::<Vec<_>>(),
        );
        decoded.as_str()
    } else {
        if prefix.contains(&0) {
            return false;
        }
        decoded = String::from_utf8_lossy(prefix).into_owned();
        decoded.as_str()
    };
    let lower = text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .to_ascii_lowercase();
    !(lower.contains('\0')
        || lower.starts_with("#!")
        || lower.starts_with("bplist00")
        || lower.starts_with("<!doctype html")
        || lower.starts_with("<html")
        || lower.starts_with("<script")
        || lower
            .lines()
            .any(|line| matches!(line.trim(), "[desktop entry]" | "[internetshortcut]"))
        || lower.starts_with("<plist")
        || (lower.starts_with("<?xml") && lower.contains("<plist")))
}

#[cfg(target_os = "macos")]
fn mac_file_redirects(file: &File) -> io::Result<bool> {
    use std::os::fd::AsRawFd;
    let mut info = [0u8; 32];
    // Finder aliases are regular files. Inspect the Finder metadata on the
    // same opened descriptor even if the caller changes the filename.
    let n = unsafe {
        libc::fgetxattr(
            file.as_raw_fd(),
            c"com.apple.FinderInfo".as_ptr(),
            info.as_mut_ptr().cast(),
            info.len(),
            0,
            0,
        )
    };
    if n < 0 {
        let e = io::Error::last_os_error();
        if matches!(e.raw_os_error(), Some(libc::ENOATTR) | Some(libc::ENOTSUP)) {
            return Ok(false);
        }
        return Err(e);
    }
    Ok(n != 32 || info[8] & 0x80 != 0 || matches!(&info[..4], b"APPL" | b"adrp" | b"ilnk"))
}

pub(crate) fn handler(root: &BoundLoupistRoot, input: &str) -> WorkspaceDocumentHandler {
    let (_relative, _file) = match admit(root, input) {
        Ok(document) => document,
        Err(reason) => return WorkspaceDocumentHandler::Refused { reason },
    };
    WorkspaceDocumentHandler::Allowed {
        application_name: application_name(&_file),
    }
}

fn application_name(_file: &File) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let reference = mac_reference(_file).ok()?;
        mac_handler::display_name(Path::new(&reference))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub(crate) fn open(root: &BoundLoupistRoot, input: &str) -> WorkspaceDocumentOpen {
    open_with(root, input, dispatch)
}

fn open_with(
    root: &BoundLoupistRoot,
    input: &str,
    dispatch: impl FnOnce(&File, &Path) -> io::Result<Option<String>>,
) -> WorkspaceDocumentOpen {
    // Unix dispatch uses the descriptor. Windows dispatch uses a pathname,
    // so it also holds every ancestor through the system call.
    #[cfg(windows)]
    let _parents = match workspace_files::relative_path(input) {
        Ok(relative) => {
            match super::windows::open_parent_directories_no_reparse(&root.path().join(relative)) {
                Ok(guards) => guards,
                Err(_) => {
                    return WorkspaceDocumentOpen::Refused {
                        reason: DocumentRefusal::InaccessibleFile,
                    }
                }
            }
        }
        Err(_) => {
            return WorkspaceDocumentOpen::Refused {
                reason: DocumentRefusal::OutsideWorkspace,
            }
        }
    };
    match admit(root, input) {
        Err(reason) => WorkspaceDocumentOpen::Refused { reason },
        Ok((relative, file)) => match dispatch(&file, &root.path().join(relative)) {
            Ok(application_name) => WorkspaceDocumentOpen::Requested { application_name },
            Err(error) => {
                let description = error.to_string();
                let reason = description.split_whitespace().collect::<Vec<_>>().join(" ");
                WorkspaceDocumentOpen::HandlerUnavailable {
                    application_name: application_name(&file),
                    reason: (!reason.is_empty()).then(|| crate::util::shorten(&reason, 384)),
                }
            }
        },
    }
}

#[cfg(target_os = "macos")]
fn mac_reference(file: &File) -> io::Result<String> {
    use std::os::unix::fs::MetadataExt;
    let meta = file.metadata()?;
    Ok(format!("/.vol/{}/{}", meta.dev(), meta.ino()))
}

#[cfg(target_os = "macos")]
fn dispatch(file: &File, _: &Path) -> io::Result<Option<String>> {
    // Give LaunchServices a file-reference URL to the admitted inode. An
    // extensionless /.vol pathname cannot select a document handler.
    let reference = mac_reference(file)?;
    let path = Path::new(&reference);
    let url = mac_handler::file_reference_url(path).ok_or_else(|| {
        io::Error::other("Could not resolve the admitted file's system reference.")
    })?;
    let output = std::process::Command::new("/usr/bin/open")
        .arg("--")
        .arg(&url)
        .output()?;
    if output.status.success() {
        Ok(mac_handler::display_name(path))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr.trim();
        let description = if reason.is_empty() {
            format!("The system open command failed with {}.", output.status)
        } else {
            reason
                .replace(&url, "this file")
                .replace(&reference, "this file")
        };
        Err(io::Error::other(description))
    }
}

#[cfg(target_os = "macos")]
mod mac_handler {
    use std::{
        ffi::{c_char, c_void, CStr},
        os::unix::ffi::OsStrExt,
        path::Path,
        ptr,
    };

    type CFRef = *const c_void;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: CFRef,
            buffer: *const u8,
            length: isize,
            directory: u8,
        ) -> CFRef;
        fn CFURLCreateFileReferenceURL(allocator: CFRef, url: CFRef, error: *mut CFRef) -> CFRef;
        fn CFURLGetString(url: CFRef) -> CFRef;
        fn CFURLCopyResourcePropertyForKey(
            url: CFRef,
            key: CFRef,
            value: *mut CFRef,
            error: *mut CFRef,
        ) -> u8;
        static kCFURLLocalizedNameKey: CFRef;
        fn CFStringGetCString(value: CFRef, buffer: *mut c_char, size: isize, encoding: u32) -> u8;
        fn CFRelease(value: CFRef);
    }

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn LSCopyDefaultApplicationURLForURL(url: CFRef, roles: u32, error: *mut CFRef) -> CFRef;
    }

    struct Owned(CFRef);

    impl Owned {
        fn from_copy(value: CFRef) -> Option<Self> {
            if value.is_null() {
                return None;
            }
            Some(Self(value))
        }
    }

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: Owned stores one non-null Create/Copy reference.
            unsafe { CFRelease(self.0) }
        }
    }

    pub(super) fn file_reference_url(path: &Path) -> Option<String> {
        let bytes = path.as_os_str().as_bytes();
        let mut buffer = [0; 1024];
        // SAFETY: the URL owns its input, and its borrowed string stays
        // valid while the reference is held. The buffer is writable.
        unsafe {
            let location = Owned::from_copy(CFURLCreateFromFileSystemRepresentation(
                ptr::null(),
                bytes.as_ptr(),
                bytes.len().try_into().ok()?,
                0,
            ))?;
            let reference = Owned::from_copy(CFURLCreateFileReferenceURL(
                ptr::null(),
                location.0,
                ptr::null_mut(),
            ))?;
            let string = CFURLGetString(reference.0);
            if string.is_null()
                || CFStringGetCString(
                    string,
                    buffer.as_mut_ptr(),
                    buffer.len() as isize,
                    0x0800_0100,
                ) == 0
            {
                return None;
            }
            CStr::from_ptr(buffer.as_ptr())
                .to_str()
                .ok()
                .map(str::to_owned)
        }
    }

    pub(super) fn display_name(path: &Path) -> Option<String> {
        let bytes = path.as_os_str().as_bytes();
        let mut name = ptr::null();
        let mut buffer = [0; 1024];
        // SAFETY: pass live UTF-8 path bytes and bounded writable buffers.
        // The resource key requests a CFString. Each successful Copy is owned.
        unsafe {
            let url = Owned::from_copy(CFURLCreateFromFileSystemRepresentation(
                ptr::null(),
                bytes.as_ptr(),
                bytes.len().try_into().ok()?,
                0,
            ))?;
            let handler = Owned::from_copy(LSCopyDefaultApplicationURLForURL(
                url.0,
                u32::MAX,
                ptr::null_mut(),
            ))?;
            if CFURLCopyResourcePropertyForKey(
                handler.0,
                kCFURLLocalizedNameKey,
                &mut name,
                ptr::null_mut(),
            ) == 0
            {
                return None;
            }
            let name = Owned::from_copy(name)?;
            if CFStringGetCString(
                name.0,
                buffer.as_mut_ptr(),
                buffer.len() as isize,
                0x0800_0100,
            ) == 0
            {
                return None;
            }
            let name = CStr::from_ptr(buffer.as_ptr()).to_str().ok()?;
            let name = name.strip_suffix(".app").unwrap_or(name);
            (!name.is_empty()).then(|| name.to_owned())
        }
    }
}

#[cfg(target_os = "linux")]
fn dispatch(file: &File, _: &Path) -> io::Result<Option<String>> {
    use webkit2gtk::{
        gio,
        glib::{
            variant::{Handle, ToVariant},
            VariantDict,
        },
    };
    let bus = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)
        .map_err(io::Error::other)?;
    let list = gio::UnixFDList::from_array([file.try_clone()?]);
    let parameters = webkit2gtk::glib::Variant::tuple_from_iter([
        "".to_variant(),
        Handle(0).to_variant(),
        VariantDict::new(None).end(),
    ]);
    let _ = bus
        .call_with_unix_fd_list_sync(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.OpenURI",
            "OpenFile",
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            10000,
            Some(&list),
            None::<&gio::Cancellable>,
        )
        .map_err(io::Error::other)?;
    Ok(None)
}

#[cfg(windows)]
fn dispatch(_: &File, path: &Path) -> io::Result<Option<String>> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            window: *mut c_void,
            verb: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> isize;
    }

    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, mode: u32) -> i32;
        fn CoUninitialize();
    }

    const COINIT_APARTMENTTHREADED: u32 = 0x2;
    const COINIT_DISABLE_OLE1DDE: u32 = 0x4;
    let path: Vec<_> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: establish COM on the current blocking thread for shell handlers.
    let initialized = unsafe {
        CoInitializeEx(
            ptr::null_mut(),
            COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE,
        )
    };
    if initialized < 0 {
        return Err(io::Error::other("document handler unavailable"));
    }
    // SAFETY: the checked file pathname is NUL-terminated and pinned by
    // open_with's handles; a null verb requests the default association.
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            ptr::null(),
            path.as_ptr(),
            ptr::null(),
            ptr::null(),
            1,
        )
    };
    // SAFETY: balance this thread's successful CoInitializeEx.
    unsafe { CoUninitialize() }
    if result > 32 {
        Ok(None)
    } else {
        Err(io::Error::other("document handler unavailable"))
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn dispatch(_: &File, _: &Path) -> io::Result<Option<String>> {
    Err(io::Error::other("document handler unavailable"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn admits_documents_and_refuses_non_documents() {
        let dir = tempfile::tempdir().unwrap();
        let root = BoundLoupistRoot::open_unbound(dir.path().canonicalize().unwrap()).unwrap();
        for (ext, bytes) in [
            ("xlsx", b"PK\x03\x04".as_slice()),
            ("xlsm", b"PK\x03\x04"),
            ("xls", b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1"),
            ("docx", b"PK\x03\x04"),
            ("pptx", b"PK\x03\x04"),
            ("pdf", b"%PDF-"),
            ("png", b"\x89PNG\r\n\x1a\n"),
            ("jpg", &[0xff, 0xd8, 0xff]),
            ("jpeg", &[0xff, 0xd8, 0xff]),
            ("gif", b"GIF89a"),
            ("webp", b"RIFFxxxxWEBP"),
            ("svg", b"<svg/>"),
            ("md", b"# report"),
            ("txt", b"text"),
            ("json", b"{}"),
            ("csv", b"a,b"),
            ("tsv", b"a\tb"),
        ] {
            let name = format!("report.{ext}");
            std::fs::write(root.path().join(&name), bytes).unwrap();
            assert!(admit(&root, &name).is_ok(), "{ext}");
        }
        for ext in ["html", "sh", "js", "exe", "app", "lnk", "webloc", "desktop"] {
            assert!(admit(&root, &format!("report.{ext}")).is_err());
        }
        std::fs::write(root.path().join("script.txt"), b"#!/bin/sh").unwrap();
        assert_eq!(
            admit(&root, "script.txt").unwrap_err(),
            DocumentRefusal::UnsafeDocument
        );
        std::fs::create_dir(root.path().join("folder.xlsx")).unwrap();
        assert!(admit(&root, "folder.xlsx").is_err());
        #[cfg(unix)]
        {
            let other = tempfile::tempdir().unwrap();
            std::fs::write(other.path().join("outside.txt"), "outside").unwrap();
            std::os::unix::fs::symlink(other.path(), root.path().join("sub")).unwrap();
            assert!(admit(&root, "sub/outside.txt").is_err());
        }
    }
    #[test]
    fn dispatches_only_an_admitted_document_and_returns_the_handler_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = BoundLoupistRoot::open_unbound(dir.path().canonicalize().unwrap()).unwrap();
        std::fs::write(root.path().join("report.CSV"), b"year,value").unwrap();
        let opened = open_with(&root, "./report.CSV", |mut file, path| {
            assert_eq!(path, root.path().join("report.CSV"));
            let mut contents = String::new();
            file.read_to_string(&mut contents).unwrap();
            assert_eq!(contents, "year,value");
            Ok(Some("Numbers".into()))
        });
        assert_eq!(
            serde_json::to_value(opened).unwrap(),
            serde_json::json!({"status": "requested", "application_name": "Numbers"}),
        );
        let failed = open_with(&root, "report.CSV", |_, _| {
            Err(io::Error::other("The application could not read the file."))
        });
        assert!(
            matches!(&failed, WorkspaceDocumentOpen::HandlerUnavailable {
            reason: Some(reason), ..
        } if reason == "The application could not read the file.")
        );
        assert_eq!(
            serde_json::to_value(failed).unwrap()["status"],
            "handler_unavailable"
        );
        assert_eq!(
            open_with(&root, "page.html", |_, _| panic!("unexpected handler call")),
            WorkspaceDocumentOpen::Refused {
                reason: DocumentRefusal::UnsupportedType
            },
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_url_is_a_file_reference_after_the_admitted_path_moves() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("report.xlsx");
        std::fs::write(&path, b"fixture").unwrap();
        let file = File::open(&path).unwrap();
        let reference = mac_reference(&file).unwrap();
        let url = mac_handler::file_reference_url(Path::new(&reference)).unwrap();
        std::fs::rename(&path, directory.path().join("renamed.xlsx")).unwrap();
        assert!(url.starts_with("file:///.file/id="));
        assert_eq!(
            mac_handler::file_reference_url(Path::new(&reference)).as_ref(),
            Some(&url)
        );
    }

    #[test]
    fn handler_inspection_refuses_disallowed_files_before_resolving_an_application() {
        let dir = tempfile::tempdir().unwrap();
        let root = BoundLoupistRoot::open_unbound(dir.path().canonicalize().unwrap()).unwrap();
        std::fs::write(root.path().join("script.txt"), b"#!/bin/sh").unwrap();
        std::fs::create_dir(root.path().join("folder.xlsx")).unwrap();
        for (path, reason) in [
            ("../outside.txt", DocumentRefusal::OutsideWorkspace),
            ("page.html", DocumentRefusal::UnsupportedType),
            ("script.txt", DocumentRefusal::UnsafeDocument),
            ("folder.xlsx", DocumentRefusal::InaccessibleFile),
        ] {
            assert_eq!(
                handler(&root, path),
                WorkspaceDocumentHandler::Refused { reason }
            );
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn launch_services_resolves_the_same_default_for_the_inode_reference() {
        let dir = tempfile::tempdir().unwrap();
        let root = BoundLoupistRoot::open_unbound(dir.path().canonicalize().unwrap()).unwrap();
        let path = root.path().join("comparison.xlsx");
        std::fs::write(
            &path,
            include_bytes!("../python_run/workbook/comparison.xlsx"),
        )
        .unwrap();
        let (_, file) = admit(&root, "comparison.xlsx").unwrap();
        let reference = mac_reference(&file).unwrap();
        let name = mac_handler::display_name(&path);
        assert_eq!(mac_handler::display_name(Path::new(&reference)), name);
        assert_eq!(
            handler(&root, "comparison.xlsx"),
            WorkspaceDocumentHandler::Allowed {
                application_name: name.clone()
            },
        );
        eprintln!("LaunchServices workbook handler: {name:?}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn finder_alias_metadata_refuses_a_regular_document_filename() {
        use std::os::fd::AsRawFd;
        let dir = tempfile::tempdir().unwrap();
        let root = BoundLoupistRoot::open_unbound(dir.path().canonicalize().unwrap()).unwrap();
        let file = File::create(root.path().join("alias.txt")).unwrap();
        let mut info = [0u8; 32];
        info[8] = 0x80;
        let ok = unsafe {
            libc::fsetxattr(
                file.as_raw_fd(),
                c"com.apple.FinderInfo".as_ptr(),
                info.as_ptr().cast(),
                info.len(),
                0,
                0,
            )
        };
        assert_eq!(ok, 0);
        assert_eq!(
            admit(&root, "alias.txt").unwrap_err(),
            DocumentRefusal::UnsafeDocument
        );
    }
}
