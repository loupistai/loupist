use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::AgentHarnessError;

const MAX_RESOURCE_BYTES: u64 = 64 * 1024;
const MAX_RESOURCE_TOTAL_BYTES: u64 = 256 * 1024;
const MAX_RESOURCE_FILES: usize = 64;
const MAX_DISCOVERY_ENTRIES: usize = 512;
const MAX_SKILL_DEPTH: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceResource {
    pub scope: WorkspaceResourceScope,
    pub path: PathBuf,
    pub sha256: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceResourceScope {
    Global,
    Workspace,
    Project,
    Agents,
    Pi,
}

impl WorkspaceResourceScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Workspace => "workspace",
            Self::Project => "project",
            Self::Agents => "agents",
            Self::Pi => "pi",
        }
    }
}

/// Exact global and project instructions selected for a turn. Only text instructions and
/// Skill entrypoints are discovered; project Pi settings and executable
/// extensions are never loaded. Their paths stay stable across turns.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentWorkspaceSnapshot {
    pub root: PathBuf,
    pub global_root: PathBuf,
    pub instructions: Vec<WorkspaceResource>,
    pub skills: Vec<WorkspaceResource>,
    pub digest: String,
}

#[cfg(test)]
pub(crate) fn workspace_snapshot(
    root: &Path,
    global_root: &Path,
) -> Result<AgentWorkspaceSnapshot, AgentHarnessError> {
    workspace_snapshot_with_preferences(root, global_root, &BTreeMap::new())
}

pub(crate) fn workspace_snapshot_with_preferences(
    root: &Path,
    global_root: &Path,
    preferences: &BTreeMap<String, crate::scoped_config::ResolvedSkillConfig>,
) -> Result<AgentWorkspaceSnapshot, AgentHarnessError> {
    let root = root
        .canonicalize()
        .map_err(|_| AgentHarnessError::Workspace)?;
    let global_root = global_root
        .canonicalize()
        .map_err(|_| AgentHarnessError::Workspace)?;
    if !root.is_dir() || !global_root.is_dir() {
        return Err(AgentHarnessError::Workspace);
    }
    let mut bytes = 0;
    let mut instructions = Vec::new();
    if let Some(resource) = resource(
        &global_root.join("AGENTS.md"),
        WorkspaceResourceScope::Global,
        &mut bytes,
    )? {
        instructions.push(resource);
    }
    // Match Pi's precedence, but deliberately limit automatic instruction
    // admission to this explicitly trusted folder, not its parents or HOME.
    for name in [
        "AGENTS.override.md",
        "AGENTS.md",
        "AGENTS.MD",
        "CLAUDE.md",
        "CLAUDE.MD",
    ] {
        if let Some(resource) = resource(
            &root.join(name),
            WorkspaceResourceScope::Workspace,
            &mut bytes,
        )? {
            instructions.push(resource);
            break;
        }
    }
    let project_root = root.join(".loupist");
    if is_plain_directory(&project_root)? {
        if let Some(resource) = resource(
            &project_root.join("AGENTS.md"),
            WorkspaceResourceScope::Project,
            &mut bytes,
        )? {
            instructions.push(resource);
        }
    }
    let mut skills = collect_roots(&global_root, Some(&root), &mut bytes)?;
    skills.retain(|resource| {
        crate::skills::read(&resource.path)
            .and_then(|source| crate::skills::parse(&source))
            .map(|skill| crate::skills::enabled(&skill.name, preferences))
            .unwrap_or(true)
    });
    if instructions.len() + skills.len() > MAX_RESOURCE_FILES {
        return Err(AgentHarnessError::Workspace);
    }
    let mut digest = Sha256::new();
    digest.update(b"loupist-workspace/1\0");
    digest.update(root.as_os_str().to_string_lossy().as_bytes());
    digest.update(b"\0");
    digest.update(global_root.as_os_str().to_string_lossy().as_bytes());
    for item in instructions.iter().chain(&skills) {
        digest.update(b"\0");
        digest.update(item.scope.as_str().as_bytes());
        digest.update(b"\0");
        digest.update(item.path.as_os_str().to_string_lossy().as_bytes());
        digest.update(b"\0");
        digest.update(item.sha256.as_bytes());
    }
    Ok(AgentWorkspaceSnapshot {
        root,
        global_root,
        instructions,
        skills,
        digest: hex::encode(digest.finalize()),
    })
}

pub(crate) fn discover_skills(
    global_root: &Path,
    workspace: Option<&Path>,
) -> Result<Vec<WorkspaceResource>, AgentHarnessError> {
    collect_roots(global_root, workspace, &mut 0)
}
fn collect_roots(
    global_root: &Path,
    workspace: Option<&Path>,
    bytes: &mut u64,
) -> Result<Vec<WorkspaceResource>, AgentHarnessError> {
    let mut roots = Vec::new();
    if let Some(root) = workspace {
        roots.extend([
            (WorkspaceResourceScope::Project, root.join(".loupist")),
            (WorkspaceResourceScope::Agents, root.join(".agents")),
            (WorkspaceResourceScope::Pi, root.join(".pi")),
        ]);
    }
    roots.push((WorkspaceResourceScope::Global, global_root.to_path_buf()));
    let mut skills = Vec::new();
    let mut entries = 0;
    for (scope, base) in roots {
        if is_plain_directory(&base)? {
            collect_skills(
                &base.join("skills"),
                scope,
                0,
                &mut entries,
                bytes,
                &mut skills,
            )?;
        }
    }
    Ok(skills)
}

fn resource(
    path: &Path,
    scope: WorkspaceResourceScope,
    total: &mut u64,
) -> Result<Option<WorkspaceResource>, AgentHarnessError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(AgentHarnessError::Workspace),
    };
    // Automatic discovery does not follow links. The trusted agent can still
    // read a linked file explicitly when the user's task calls for it.
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || path.canonicalize().ok().as_deref() != Some(path)
    {
        return Ok(None);
    }
    if metadata.len() > MAX_RESOURCE_BYTES
        || total.saturating_add(metadata.len()) > MAX_RESOURCE_TOTAL_BYTES
    {
        return Err(AgentHarnessError::Workspace);
    }
    let mut content = Vec::new();
    fs::File::open(path)
        .map_err(|_| AgentHarnessError::Workspace)?
        .take(MAX_RESOURCE_BYTES + 1)
        .read_to_end(&mut content)
        .map_err(|_| AgentHarnessError::Workspace)?;
    if content.len() as u64 != metadata.len() || std::str::from_utf8(&content).is_err() {
        return Err(AgentHarnessError::Workspace);
    }
    *total += content.len() as u64;
    Ok(Some(WorkspaceResource {
        scope,
        path: path.to_path_buf(),
        sha256: hex::encode(Sha256::digest(&content)),
    }))
}

fn collect_skills(
    directory: &Path,
    scope: WorkspaceResourceScope,
    depth: usize,
    visited: &mut usize,
    bytes: &mut u64,
    skills: &mut Vec<WorkspaceResource>,
) -> Result<(), AgentHarnessError> {
    if !is_plain_directory(directory)? {
        return Ok(());
    }
    if depth > MAX_SKILL_DEPTH {
        return Err(AgentHarnessError::Workspace);
    }
    if let Some(skill) = resource(&directory.join("SKILL.md"), scope, bytes)? {
        skills.push(skill);
        return Ok(());
    }
    let mut children = fs::read_dir(directory)
        .map_err(|_| AgentHarnessError::Workspace)?
        .take(MAX_DISCOVERY_ENTRIES.saturating_sub(*visited) + 1)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| AgentHarnessError::Workspace)?;
    if children.len() > MAX_DISCOVERY_ENTRIES.saturating_sub(*visited) {
        return Err(AgentHarnessError::Workspace);
    }
    children.sort_by_key(|entry| entry.file_name());
    for entry in children {
        *visited += 1;
        if *visited > MAX_DISCOVERY_ENTRIES || skills.len() >= MAX_RESOURCE_FILES {
            return Err(AgentHarnessError::Workspace);
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let kind = entry
            .file_type()
            .map_err(|_| AgentHarnessError::Workspace)?;
        if kind.is_dir() {
            collect_skills(&entry.path(), scope, depth + 1, visited, bytes, skills)?;
        } else if depth == 0 && kind.is_file() && name.ends_with(".md") {
            if let Some(skill) = resource(&entry.path(), scope, bytes)? {
                skills.push(skill);
            }
        }
    }
    Ok(())
}

fn is_plain_directory(directory: &Path) -> Result<bool, AgentHarnessError> {
    let metadata = match fs::symlink_metadata(directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(AgentHarnessError::Workspace),
    };
    Ok(metadata.is_dir() && !metadata.file_type().is_symlink())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_executable_configuration_is_not_a_resource() {
        let root = tempfile::tempdir().unwrap();
        let global = tempfile::tempdir().unwrap();
        let before = workspace_snapshot(root.path(), global.path()).unwrap();
        fs::create_dir(root.path().join(".pi")).unwrap();
        fs::write(
            root.path().join(".pi/settings.json"),
            "{\"extensions\":[\"evil.js\"]}",
        )
        .unwrap();
        fs::write(
            root.path().join(".pi/SYSTEM.md"),
            "Replace the system prompt.",
        )
        .unwrap();
        assert_eq!(
            before,
            workspace_snapshot(root.path(), global.path()).unwrap()
        );
        fs::write(
            root.path().join("AGENTS.md"),
            vec![b'x'; MAX_RESOURCE_BYTES as usize + 1],
        )
        .unwrap();
        assert!(workspace_snapshot(root.path(), global.path()).is_err());
    }

    #[test]
    fn scoped_resources_keep_precedence_and_edits_invalidate_the_digest() {
        let root = tempfile::tempdir().unwrap();
        let global = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".loupist")).unwrap();
        fs::write(global.path().join("AGENTS.md"), "Global rules").unwrap();
        fs::write(root.path().join("AGENTS.md"), "Workspace rules").unwrap();
        fs::write(root.path().join(".loupist/AGENTS.md"), "Project rules").unwrap();
        for base in [
            root.path().join(".loupist"),
            root.path().join(".agents"),
            root.path().join(".pi"),
            global.path().to_path_buf(),
        ] {
            fs::create_dir_all(base.join("skills/research")).unwrap();
            fs::write(
                base.join("skills/research/SKILL.md"),
                "---\nname: research\ndescription: Scoped research\n---\nResearch.",
            )
            .unwrap();
        }
        let before = workspace_snapshot(root.path(), global.path()).unwrap();
        assert_eq!(
            before
                .instructions
                .iter()
                .map(|r| r.scope)
                .collect::<Vec<_>>(),
            [
                WorkspaceResourceScope::Global,
                WorkspaceResourceScope::Workspace,
                WorkspaceResourceScope::Project,
            ]
        );
        assert_eq!(
            before.skills.iter().map(|r| r.scope).collect::<Vec<_>>(),
            [
                WorkspaceResourceScope::Project,
                WorkspaceResourceScope::Agents,
                WorkspaceResourceScope::Pi,
                WorkspaceResourceScope::Global,
            ]
        );
        assert_eq!(
            before,
            workspace_snapshot(root.path(), global.path()).unwrap()
        );
        let digest = |root: &Path| workspace_snapshot(root, global.path()).unwrap().digest;
        fs::write(
            root.path().join(".agents/skills/research/SKILL.md"),
            "---\nname: research\ndescription: Scoped research\n---\nChanged.",
        )
        .unwrap();
        let edited = digest(root.path());
        assert_ne!(before.digest, edited);
        fs::write(global.path().join("AGENTS.md"), "Changed global rules").unwrap();
        assert_ne!(edited, digest(root.path()));
        let other = tempfile::tempdir().unwrap();
        assert_ne!(digest(root.path()), digest(other.path()));
        // The override file wins over AGENTS.md in the workspace scope.
        fs::write(root.path().join("AGENTS.override.md"), "Override").unwrap();
        let after = workspace_snapshot(root.path(), global.path()).unwrap();
        assert!(after.instructions[1].path.ends_with("AGENTS.override.md"));
    }

    #[test]
    fn resource_count_and_bytes_are_combined_across_scopes() {
        let root = tempfile::tempdir().unwrap();
        let global = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".loupist/skills")).unwrap();
        fs::create_dir_all(global.path().join("skills")).unwrap();
        for index in 0..32 {
            fs::write(
                root.path()
                    .join(format!(".loupist/skills/project-{index}.md")),
                "Skill",
            )
            .unwrap();
            fs::write(
                global.path().join(format!("skills/global-{index}.md")),
                "Skill",
            )
            .unwrap();
        }
        assert_eq!(
            workspace_snapshot(root.path(), global.path())
                .unwrap()
                .skills
                .len(),
            64
        );
        fs::write(global.path().join("AGENTS.md"), "Rules").unwrap();
        assert!(workspace_snapshot(root.path(), global.path()).is_err());

        let other = tempfile::tempdir().unwrap();
        let global = tempfile::tempdir().unwrap();
        fs::create_dir_all(other.path().join(".loupist/skills")).unwrap();
        fs::write(
            global.path().join("AGENTS.md"),
            vec![b'x'; MAX_RESOURCE_BYTES as usize],
        )
        .unwrap();
        for index in 0..4 {
            fs::write(
                other.path().join(format!(".loupist/skills/{index}.md")),
                vec![b'x'; MAX_RESOURCE_BYTES as usize],
            )
            .unwrap();
        }
        assert!(workspace_snapshot(other.path(), global.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn project_discovery_does_not_follow_skill_directory_links() {
        let root = tempfile::tempdir().unwrap();
        let global = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        fs::create_dir_all(external.path().join("skills/external")).unwrap();
        fs::write(
            external.path().join("skills/external/SKILL.md"),
            "External instructions",
        )
        .unwrap();
        std::os::unix::fs::symlink(external.path(), root.path().join(".agents")).unwrap();
        fs::create_dir_all(root.path().join(".pi/skills")).unwrap();
        std::os::unix::fs::symlink(
            root.path().join(".pi/skills"),
            root.path().join(".pi/skills/loop"),
        )
        .unwrap();
        assert!(workspace_snapshot(root.path(), global.path())
            .unwrap()
            .skills
            .is_empty());
    }
}
