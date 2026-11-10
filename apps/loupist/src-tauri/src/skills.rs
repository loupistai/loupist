//! User-facing skill discovery and file management. Configuration owns enablement.
use crate::{
    agent_harness,
    app::CommandError,
    scoped_config::{ConfigSnapshot, ResolvedSkillConfig},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

mod package;
#[cfg(test)]
use package::Package;
pub(crate) use package::{from_folder, from_github, from_zip, install};

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
pub fn is_fixed(name: &str) -> bool {
    matches!(name, "world" | "lens" | "journal" | "visualize")
}
pub(crate) fn invalid(message: &str) -> CommandError {
    CommandError::invalid(message)
}
pub(crate) fn io(error: impl std::fmt::Display) -> CommandError {
    CommandError::new(
        "skill_unavailable",
        format!("Could not access this skill: {error}"),
    )
}

#[derive(Debug)]
pub(crate) struct Manifest {
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub yaml: serde_yaml_ng::Mapping,
}
pub(crate) fn parse(source: &str) -> Result<Manifest, CommandError> {
    if source.len() > 64 * 1024 || source.contains('\0') {
        return Err(invalid("SKILL.md must be UTF-8 text no larger than 64 KiB"));
    }
    let normalized = source.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .ok_or_else(|| invalid("SKILL.md needs YAML name and description at the beginning"))?;
    let (yaml, body) = rest
        .split_once("\n---")
        .filter(|(_, body)| body.is_empty() || body.starts_with('\n'))
        .ok_or_else(|| invalid("The skill's YAML frontmatter is not closed"))?;
    let yaml: serde_yaml_ng::Mapping = serde_yaml_ng::from_str(yaml)
        .map_err(|_| invalid("The skill's YAML frontmatter is invalid"))?;
    let value = |key: &str| {
        yaml.get(serde_yaml_ng::Value::String(key.into()))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    };
    let name = value("name").ok_or_else(|| invalid("The skill needs a name"))?;
    let description = value("description")
        .filter(|s| !s.trim().is_empty() && s.len() <= 4096)
        .ok_or_else(|| invalid("The skill needs a description of up to 4096 bytes"))?;
    if !valid_name(&name) {
        return Err(invalid("The skill identifier must use lowercase letters, numbers and single hyphens (up to 64 characters)"));
    }
    Ok(Manifest {
        name,
        description,
        instructions: body.trim_start_matches('\n').to_owned(),
        yaml,
    })
}
pub(crate) fn read(path: &Path) -> Result<String, CommandError> {
    plain(path)?;
    String::from_utf8(
        crate::fs::artifact_trust::read_private_regular_file_bounded(path, 64 * 1024)
            .map_err(io)?,
    )
    .map_err(|_| invalid("SKILL.md must be UTF-8 text"))
}
pub(crate) fn plain(path: &Path) -> Result<(), CommandError> {
    if !path.is_absolute() {
        return Err(invalid("Choose an absolute skill location"));
    }
    for part in path.ancestors() {
        if fs::symlink_metadata(part)
            .map_err(io)?
            .file_type()
            .is_symlink()
        {
            return Err(invalid("Skill files cannot be symbolic links"));
        }
    }
    if path.canonicalize().map_err(io)? != path {
        return Err(invalid("The skill location changed; reload and try again"));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ManagedSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub path: Option<String>,
    pub enabled: bool,
    pub fixed: bool,
    pub editable: bool,
    pub shadowed: bool,
    pub revision: String,
    pub error: Option<String>,
}
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalog {
    pub skills: Vec<ManagedSkill>,
    pub configuration: ConfigSnapshot,
}
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SkillDocument {
    pub skill: ManagedSkill,
    pub instructions: String,
    pub files: Vec<String>,
}

pub fn catalog(
    global: &Path,
    workspace: Option<&Path>,
    config: ConfigSnapshot,
) -> Result<SkillCatalog, CommandError> {
    let mut entries = agent_harness::skill_catalog(&config.effective.enabled_skill_ids())
        .into_iter()
        .map(|skill| ManagedSkill {
            revision: crate::util::sha256(
                agent_harness::bundled_skill_content(&skill.id).expect("bundled skill"),
            ),
            id: skill.id,
            name: skill.name,
            description: skill.description,
            enabled: skill.enabled,
            fixed: skill.fixed,
            source: "bundled".into(),
            path: None,
            editable: false,
            shadowed: false,
            error: None,
        })
        .collect::<Vec<_>>();
    let mut names = entries
        .iter()
        .map(|s| s.id.clone())
        .collect::<BTreeSet<_>>();
    let resources = agent_harness::workspace::discover_skills(global, workspace).map_err(|_| {
        invalid("Could not scan skill folders; check their size and file permissions")
    })?;
    let owned_root = workspace
        .map(|p| p.join(".loupist"))
        .unwrap_or_else(|| global.to_path_buf())
        .join("skills");
    for resource in resources {
        let path = resource.path;
        let source = match resource.scope {
            agent_harness::workspace::WorkspaceResourceScope::Global => "personal",
            _ => "loupist",
        }
        .to_owned();
        let parsed = read(&path).and_then(|text| parse(&text));
        let (id, mut name, description, error) = match parsed {
            Ok(skill) => (skill.name.clone(), skill.name, skill.description, None),
            Err(error) => {
                let name = path
                    .parent()
                    .and_then(Path::file_name)
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                (name.clone(), name, String::new(), Some(error.message))
            }
        };
        let metadata = path
            .parent()
            .expect("skill parent")
            .join("agents/openai.yaml");
        if let Ok(text) = read(&metadata) {
            #[derive(Deserialize)]
            struct Metadata {
                interface: Option<Interface>,
            }
            #[derive(Deserialize)]
            struct Interface {
                display_name: Option<String>,
            }
            if let Ok(Metadata {
                interface: Some(interface),
            }) = serde_yaml_ng::from_str(&text)
            {
                if let Some(display) = interface
                    .display_name
                    .filter(|s| !s.trim().is_empty() && s.len() <= 256)
                {
                    name = display;
                }
            }
        }
        let shadowed = !names.insert(id.clone());
        let editable = path == owned_root.join(&id).join("SKILL.md") && !shadowed;
        let enabled = error.is_none()
            && !shadowed
            && config
                .effective
                .skills
                .get(&id)
                .map(|skill| skill.enabled)
                .unwrap_or_else(|| agent_harness::skill_default_enabled(&id));
        entries.push(ManagedSkill {
            id,
            name,
            description,
            source,
            path: Some(path.to_string_lossy().into()),
            enabled,
            fixed: false,
            editable,
            shadowed,
            revision: resource.sha256,
            error,
        });
    }
    Ok(SkillCatalog {
        skills: entries,
        configuration: config,
    })
}

pub(crate) fn document(entry: ManagedSkill) -> Result<SkillDocument, CommandError> {
    if let Some(path) = &entry.path {
        let text = read(Path::new(path))?;
        let instructions = parse(&text)
            .map(|manifest| manifest.instructions)
            .unwrap_or(text);
        let files = if Path::new(path)
            .file_name()
            .is_some_and(|name| name != "SKILL.md")
        {
            vec![Path::new(path)
                .file_name()
                .expect("skill filename")
                .to_string_lossy()
                .into_owned()]
        } else {
            from_folder(Path::new(path).parent().expect("skill folder"))?
                .keys()
                .cloned()
                .collect()
        };
        Ok(SkillDocument {
            skill: entry,
            instructions,
            files,
        })
    } else {
        let source = std::str::from_utf8(
            agent_harness::bundled_skill_content(&entry.id)
                .ok_or_else(|| invalid("Skill not found"))?,
        )
        .map_err(io)?;
        let files = agent_harness::bundled_skill_file_names(&entry.id);
        Ok(SkillDocument {
            skill: entry,
            instructions: parse(source)?.instructions,
            files,
        })
    }
}
pub(crate) fn create(
    root: &Path,
    name: &str,
    description: &str,
    instructions: &str,
) -> Result<(), CommandError> {
    if name.trim().is_empty() || name.len() > 256 {
        return Err(invalid("Enter a skill name of up to 256 bytes"));
    }
    let slug = name
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let id = if valid_name(&slug) {
        slug
    } else {
        format!("skill-{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
    };
    let header = BTreeMap::from([("name", id.as_str()), ("description", description)]);
    let text = format!(
        "---\n{}---\n{}\n",
        serde_yaml_ng::to_string(&header).map_err(io)?,
        instructions.trim()
    );
    let metadata = serde_yaml_ng::to_string(&serde_json::json!({"interface": {"display_name": name.trim(), "short_description": description}})).map_err(io)?;
    let package = BTreeMap::from([
        ("SKILL.md".into(), text.into_bytes()),
        ("agents/openai.yaml".into(), metadata.into_bytes()),
    ]);
    install(root, package)
}
pub(crate) fn save(
    entry: &ManagedSkill,
    revision: &str,
    description: &str,
    instructions: &str,
) -> Result<(), CommandError> {
    if !entry.editable {
        return Err(invalid(
            "This skill is read-only here; choose its original scope to edit it",
        ));
    }
    let path = PathBuf::from(
        entry
            .path
            .as_deref()
            .ok_or_else(|| invalid("Bundled skills cannot be edited"))?,
    );
    let source = read(&path)?;
    if crate::util::sha256(source.as_bytes()) != revision {
        return Err(CommandError::conflict(
            "Skill changed outside this view; reload before saving",
        ));
    }
    let mut manifest = parse(&source)?;
    manifest.yaml.insert(
        serde_yaml_ng::Value::String("description".into()),
        serde_yaml_ng::Value::String(description.into()),
    );
    let source = format!(
        "---\n{}---\n{}\n",
        serde_yaml_ng::to_string(&manifest.yaml).map_err(io)?,
        instructions.trim()
    );
    parse(&source)?;
    let mut temp =
        tempfile::NamedTempFile::new_in(path.parent().expect("skill folder")).map_err(io)?;
    std::io::Write::write_all(&mut temp, source.as_bytes()).map_err(io)?;
    temp.as_file().sync_all().map_err(io)?;
    plain(&path)?;
    if crate::util::sha256(read(&path)?.as_bytes()) != revision {
        return Err(CommandError::conflict(
            "Skill changed outside this view; reload before saving",
        ));
    }
    temp.persist(path).map_err(io)?;
    Ok(())
}
pub(crate) fn remove(entry: &ManagedSkill, revision: &str) -> Result<(), CommandError> {
    if !entry.editable {
        return Err(invalid("Only skills added in this scope can be deleted"));
    }
    let path = Path::new(
        entry
            .path
            .as_deref()
            .ok_or_else(|| invalid("Bundled skills cannot be deleted"))?,
    );
    if crate::util::sha256(read(path)?.as_bytes()) != revision {
        return Err(CommandError::conflict(
            "Skill changed outside this view; reload before deleting",
        ));
    }
    plain(path.parent().expect("skill folder"))?;
    fs::remove_dir_all(path.parent().expect("skill folder")).map_err(io)
}

pub(crate) fn enabled(name: &str, preferences: &BTreeMap<String, ResolvedSkillConfig>) -> bool {
    preferences.get(name).is_none_or(|s| s.enabled)
}
#[cfg(test)]
mod tests;
