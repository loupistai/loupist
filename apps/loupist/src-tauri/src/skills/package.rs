use super::{invalid, io, parse, plain};
use crate::app::CommandError;
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Write},
    path::{Component, Path},
    time::Duration,
};

pub type Package = BTreeMap<String, Vec<u8>>;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_FILES: usize = 512;

fn relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', '\0'])
        && Path::new(path)
            .components()
            .all(|p| matches!(p, Component::Normal(_)))
}
fn add(package: &mut Package, path: String, bytes: Vec<u8>) -> Result<(), CommandError> {
    if !relative(&path) || package.contains_key(&path) {
        return Err(invalid("The package contains an unsafe or duplicate path"));
    }
    if package.len() >= MAX_FILES
        || bytes.len() + package.values().map(Vec::len).sum::<usize>() > MAX_BYTES
    {
        return Err(invalid(
            "A skill package may contain up to 512 files and 16 MiB",
        ));
    }
    package.insert(path, bytes);
    Ok(())
}
pub fn from_folder(root: &Path) -> Result<Package, CommandError> {
    plain(root)?;
    fn visit(
        root: &Path,
        dir: &Path,
        depth: usize,
        package: &mut Package,
        visited: &mut usize,
    ) -> Result<(), CommandError> {
        if depth > 8 {
            return Err(invalid("The skill folder is nested too deeply"));
        }
        for entry in fs::read_dir(dir).map_err(io)?.take(MAX_FILES + 1) {
            let entry = entry.map_err(io)?;
            *visited += 1;
            if *visited > MAX_FILES {
                return Err(invalid("The skill folder contains more than 512 entries"));
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            let path = entry.path();
            let kind = entry.file_type().map_err(io)?;
            if kind.is_symlink() {
                return Err(invalid("Skill packages cannot contain symbolic links"));
            }
            if kind.is_dir() {
                visit(root, &path, depth + 1, package, visited)?;
            } else if kind.is_file() {
                plain(&path)?;
                let bytes = crate::fs::artifact_trust::read_private_regular_file_bounded(
                    &path,
                    MAX_BYTES as u64,
                )
                .map_err(io)?;
                add(
                    package,
                    path.strip_prefix(root)
                        .map_err(io)?
                        .to_string_lossy()
                        .replace('\\', "/"),
                    bytes,
                )?;
            } else {
                return Err(invalid("Skill packages must contain ordinary files"));
            }
        }
        Ok(())
    }
    let mut package = Package::new();
    visit(root, root, 0, &mut package, &mut 0)?;
    Ok(package)
}
pub fn from_zip(bytes: Vec<u8>, selected: Option<&str>) -> Result<Package, CommandError> {
    if bytes.len() > MAX_BYTES {
        return Err(invalid("The ZIP is larger than 16 MiB"));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| invalid("Choose a valid skill ZIP package"))?;
    if zip.len() > 20_000 {
        return Err(invalid("The ZIP contains too many entries"));
    }
    zip.validate_header_consistency().map_err(io)?;
    let mut prefixes = Vec::new();
    for index in 0..zip.len() {
        let file = zip.by_index(index).map_err(io)?;
        let name = file.name().map_err(io)?;
        if !relative(name.trim_end_matches('/')) {
            return Err(invalid("The ZIP contains an unsafe path"));
        }
        if name.ends_with("SKILL.md")
            && Path::new(name.as_ref())
                .file_name()
                .is_some_and(|n| n == "SKILL.md")
        {
            let prefix = name.strip_suffix("SKILL.md").expect("skill suffix");
            if selected.is_none_or(|selected| {
                prefix.split_once('/').is_some_and(|(_, rest)| {
                    rest.trim_end_matches('/') == selected.trim_matches('/')
                })
            }) {
                prefixes.push(prefix.to_owned());
            }
        }
    }
    if prefixes.len() != 1 {
        return Err(invalid(
            "Choose a package with one skill, or a GitHub URL pointing to its exact folder",
        ));
    }
    let prefix = &prefixes[0];
    let mut package = Package::new();
    for index in 0..zip.len() {
        let file = zip.by_index(index).map_err(io)?;
        let Some(path) = file
            .name()
            .map_err(io)?
            .strip_prefix(prefix)
            .map(str::to_owned)
        else {
            continue;
        };
        if path.is_empty() || file.is_dir() {
            continue;
        }
        if file
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(invalid("Skill packages cannot contain symbolic links"));
        }
        if Path::new(&path).components().any(|p| {
            p.as_os_str().to_string_lossy().starts_with('.') || p.as_os_str() == "node_modules"
        }) {
            continue;
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(io)?;
        add(&mut package, path, bytes)?;
    }
    Ok(package)
}
pub async fn from_github(url: &str) -> Result<Package, CommandError> {
    let url = reqwest::Url::parse(url).map_err(|_| invalid("Enter a GitHub skill-folder URL"))?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid(
            "Use a public https://github.com folder URL without credentials or query parameters",
        ));
    }
    let parts = url.path().trim_matches('/').split('/').collect::<Vec<_>>();
    if parts.len() < 5
        || parts[2] != "tree"
        || !parts.iter().all(|p| {
            relative(p)
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        })
    {
        return Err(invalid(
            "Use https://github.com/owner/repo/tree/ref/path-to-skill",
        ));
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Loupist-SkillInstaller")
        .build()
        .map_err(io)?;
    let response = client
        .get(format!(
            "https://codeload.github.com/{}/{}/zip/{}",
            parts[0], parts[1], parts[3]
        ))
        .send()
        .await
        .map_err(|_| {
            invalid("Could not download this GitHub skill. Try again or import a local copy.")
        })?;
    if !response.status().is_success() {
        return Err(invalid("The public GitHub download is unavailable. Check the folder URL or import a local checkout."));
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return Err(invalid(
            "The repository ZIP exceeds 16 MiB; import only the skill folder locally",
        ));
    }
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| invalid("The skill download was interrupted"))?;
        if bytes.len() + chunk.len() > MAX_BYTES {
            return Err(invalid(
                "The repository ZIP exceeds 16 MiB; import only the skill folder locally",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    from_zip(bytes, Some(&parts[4..].join("/")))
}
pub fn install(root: &Path, package: Package) -> Result<(), CommandError> {
    let text = std::str::from_utf8(
        package
            .get("SKILL.md")
            .ok_or_else(|| invalid("The selected folder needs SKILL.md"))?,
    )
    .map_err(|_| invalid("SKILL.md must be UTF-8 text"))?;
    let manifest = parse(text)?;
    if crate::agent_harness::bundled_skill_content(&manifest.name).is_some() {
        return Err(invalid(
            "That name is reserved by an included skill; choose another skill name",
        ));
    }
    plain(root)?;
    let directory = root.join("skills");
    match fs::create_dir(&directory) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(io(e)),
    }
    plain(&directory)?;
    let temp = tempfile::Builder::new()
        .prefix(".skill-")
        .tempdir_in(&directory)
        .map_err(io)?;
    for (path, bytes) in package {
        if !relative(&path) {
            return Err(invalid("The package contains an unsafe path"));
        }
        let path = temp.path().join(path);
        fs::create_dir_all(path.parent().expect("package parent")).map_err(io)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(io)?;
        file.write_all(&bytes).map_err(io)?;
        file.sync_all().map_err(io)?;
    }
    plain(&directory)?;
    #[cfg(unix)]
    {
        use rustix::fs::{open, renameat_with, Mode, OFlags, RenameFlags};
        let root = open(
            &directory,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(io)?;
        renameat_with(
            &root,
            temp.path().file_name().expect("temporary name"),
            &root,
            manifest.name.as_str(),
            RenameFlags::NOREPLACE,
        )
        .map_err(|error| {
            if error == rustix::io::Errno::EXIST {
                CommandError::conflict(
                    "A skill with that name already exists; it was left unchanged",
                )
            } else {
                io(error)
            }
        })?;
    }
    #[cfg(not(unix))]
    fs::rename(temp.path(), directory.join(&manifest.name)).map_err(io)?;
    Ok(())
}
