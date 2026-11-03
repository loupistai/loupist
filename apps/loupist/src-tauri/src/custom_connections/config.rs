use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{credentials::Secrets, http::canonical_resource, ConnectionError};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TransportConfig {
    Http { url: String },
    Stdio { command: String, args: Vec<String> },
}

impl TransportConfig {
    pub fn command_line(&self) -> Option<String> {
        match self {
            Self::Http { .. } => None,
            Self::Stdio { command, args } => Some(
                std::iter::once(command)
                    .chain(args)
                    .map(|arg| {
                        if !arg.is_empty()
                            && arg
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"_./@=:+,-".contains(&b))
                        {
                            arg.clone()
                        } else {
                            format!("'{}'", arg.replace('\'', "'\\''"))
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
        }
    }
}

pub struct ParsedConnection {
    pub name: String,
    pub transport: TransportConfig,
    pub(crate) secrets: Secrets,
}

pub fn validate_name(name: &str) -> Result<(), ConnectionError> {
    if name.trim().is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        return Err(ConnectionError::Configuration);
    }
    Ok(())
}

pub fn validate_command(command: &str, args: &[String]) -> Result<(), ConnectionError> {
    if command.trim().is_empty()
        || command.len() > 4096
        || command.chars().any(char::is_control)
        || args.len() > 128
        || args.iter().any(|v| v.len() > 8192 || v.contains('\0'))
    {
        return Err(ConnectionError::Configuration);
    }
    let basename = Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(ConnectionError::Configuration)?
        .trim_end_matches(".exe");
    if matches!(basename, "npx" | "uvx" | "pipx") {
        let mut packages = Vec::new();
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            if matches!(arg.as_str(), "--package" | "-p" | "--from" | "--spec") {
                index += 1;
                packages.push(
                    args.get(index)
                        .ok_or(ConnectionError::Configuration)?
                        .as_str(),
                );
            } else if let Some((option, value)) = arg.split_once('=') {
                if matches!(option, "--package" | "--from" | "--spec") {
                    packages.push(value);
                } else if arg.starts_with('-') {
                    return Err(ConnectionError::Configuration);
                } else if packages.is_empty() {
                    packages.push(arg);
                    break;
                }
            } else if matches!(arg.as_str(), "-y" | "--yes" | "--" | "run") {
                // Package-runner syntax only. No shell interpolation is performed.
            } else if arg.starts_with('-') {
                return Err(ConnectionError::Configuration);
            } else {
                if packages.is_empty() {
                    packages.push(arg);
                }
                break;
            }
            index += 1;
        }
        if packages.is_empty()
            || packages
                .iter()
                .any(|package| !exact_package_version(package))
        {
            return Err(ConnectionError::Configuration);
        }
    }
    Ok(())
}

fn exact_package_version(package: &str) -> bool {
    let Some((name, version)) = package
        .rsplit_once("==")
        .or_else(|| package.rsplit_once('@'))
    else {
        return false;
    };
    if name.is_empty() || name.contains([' ', ':', '\\']) || version.is_empty() {
        return false;
    }
    let core = version.split(['-', '+']).next().unwrap_or("");
    core.split('.').count() == 3
        && core
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
}

pub(super) fn validate_secrets(secrets: &Secrets) -> Result<(), ConnectionError> {
    if secrets.env.len() > 64 || secrets.headers.len() > 32 {
        return Err(ConnectionError::Configuration);
    }
    for (key, value) in &secrets.env {
        if key.is_empty()
            || key.len() > 128
            || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || key.as_bytes()[0].is_ascii_digit()
            || value.len() > 16 * 1024
            || value.contains('\0')
        {
            return Err(ConnectionError::Configuration);
        }
    }
    for (key, value) in &secrets.headers {
        if reqwest::header::HeaderName::from_bytes(key.as_bytes()).is_err()
            || reqwest::header::HeaderValue::from_str(value).is_err()
            || value.len() > 16 * 1024
            || matches!(
                key.to_ascii_lowercase().as_str(),
                "host"
                    | "content-length"
                    | "transfer-encoding"
                    | "connection"
                    | "cookie"
                    | "mcp-session-id"
                    | "mcp-protocol-version"
                    | "content-type"
                    | "accept"
            )
        {
            return Err(ConnectionError::Configuration);
        }
    }
    Ok(())
}

pub fn parse_json(json: &str) -> Result<Vec<ParsedConnection>, ConnectionError> {
    if json.len() > 1024 * 1024 {
        return Err(ConnectionError::Configuration);
    }
    let document: Value = serde_json::from_str(json).map_err(|_| ConnectionError::Configuration)?;
    let object = document
        .as_object()
        .filter(|o| o.len() == 1)
        .ok_or(ConnectionError::Configuration)?;
    let servers = object
        .get("mcpServers")
        .and_then(Value::as_object)
        .filter(|s| !s.is_empty() && s.len() <= 32)
        .ok_or(ConnectionError::Configuration)?;
    servers
        .iter()
        .map(|(name, entry)| {
            validate_name(name)?;
            let entry = entry.as_object().ok_or(ConnectionError::Configuration)?;
            if entry.keys().any(|key| {
                !["url", "command", "args", "env", "headers", "type"].contains(&key.as_str())
            }) {
                return Err(ConnectionError::Configuration);
            }
            let secrets = Secrets {
                env: entry
                    .get("env")
                    .map(|v| serde_json::from_value(v.clone()))
                    .transpose()
                    .map_err(|_| ConnectionError::Configuration)?
                    .unwrap_or_default(),
                headers: entry
                    .get("headers")
                    .map(|v| serde_json::from_value(v.clone()))
                    .transpose()
                    .map_err(|_| ConnectionError::Configuration)?
                    .unwrap_or_default(),
                ..Default::default()
            };
            validate_secrets(&secrets)?;
            let transport = match (
                entry.get("url").and_then(Value::as_str),
                entry.get("command").and_then(Value::as_str),
            ) {
                (Some(url), None) if secrets.env.is_empty() && !entry.contains_key("args") => {
                    if entry
                        .get("type")
                        .is_some_and(|v| !matches!(v.as_str(), Some("http" | "streamable-http")))
                    {
                        return Err(ConnectionError::Configuration);
                    }
                    TransportConfig::Http {
                        url: canonical_resource(url)?,
                    }
                }
                (None, Some(command)) if secrets.headers.is_empty() => {
                    if entry
                        .get("type")
                        .is_some_and(|v| v.as_str() != Some("stdio"))
                    {
                        return Err(ConnectionError::Configuration);
                    }
                    let args: Vec<String> = entry
                        .get("args")
                        .map(|v| serde_json::from_value(v.clone()))
                        .transpose()
                        .map_err(|_| ConnectionError::Configuration)?
                        .unwrap_or_default();
                    validate_command(command, &args)?;
                    TransportConfig::Stdio {
                        command: command.into(),
                        args,
                    }
                }
                _ => return Err(ConnectionError::Configuration),
            };
            // Never place env/header secrets in persisted or returned command/URL text.
            if crate::mcp::contains_protected_value(
                &serde_json::to_value(&transport).map_err(|_| ConnectionError::Configuration)?,
                &secrets.protected_values(),
            ) {
                return Err(ConnectionError::Configuration);
            }
            Ok(ParsedConnection {
                name: name.clone(),
                transport,
                secrets,
            })
        })
        .collect()
}

fn find_executable(command: &str) -> Result<PathBuf, ConnectionError> {
    let path = Path::new(command);
    let found = if path.components().count() > 1 {
        path.is_absolute().then(|| path.to_path_buf())
    } else {
        std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(command))
                .find(|p| p.is_file())
        })
    }
    .filter(|p| p.is_file())
    .ok_or(ConnectionError::Configuration)?;
    found
        .canonicalize()
        .map_err(|_| ConnectionError::Configuration)
}

/// Resolve a shebang before spawning so process-lease identity is the actual
/// interpreter. This preserves lifecycle recovery for npx/python entry points.
pub fn launch_command(
    command: &str,
    args: &[String],
) -> Result<(PathBuf, Vec<String>), ConnectionError> {
    validate_command(command, args)?;
    let executable = find_executable(command)?;
    use std::io::Read;
    let mut bytes = [0u8; 512];
    let count = std::fs::File::open(&executable)
        .and_then(|mut file| file.read(&mut bytes))
        .map_err(|_| ConnectionError::Configuration)?;
    if bytes[..count].starts_with(b"#!") {
        let line = std::str::from_utf8(bytes[..count].split(|b| *b == b'\n').next().unwrap())
            .map_err(|_| ConnectionError::Configuration)?;
        let mut words = line[2..].split_whitespace();
        let first = words.next().ok_or(ConnectionError::Configuration)?;
        let interpreter = if first == "/usr/bin/env" {
            words
                .next()
                .filter(|w| !w.starts_with('-'))
                .ok_or(ConnectionError::Configuration)?
        } else {
            first
        };
        let mut arguments = words.map(str::to_owned).collect::<Vec<_>>();
        arguments.push(
            executable
                .to_str()
                .ok_or(ConnectionError::Configuration)?
                .into(),
        );
        arguments.extend_from_slice(args);
        return Ok((find_executable(interpreter)?, arguments));
    }
    Ok((executable, args.to_vec()))
}

pub(super) fn runtime_environment(secrets: &Secrets) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for name in [
        "PATH",
        "HOME",
        "USERPROFILE",
        "SYSTEMROOT",
        "TEMP",
        "TMPDIR",
    ] {
        if let Ok(value) = std::env::var(name) {
            env.insert(name.into(), value);
        }
    }
    env.extend(secrets.env.clone());
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_runners_require_exact_versions_without_running_commands() {
        for (command, args) in [
            ("npx", vec!["-y", "@vendor/server@1.2.3"]),
            ("uvx", vec!["server==1.2.3"]),
            ("pipx", vec!["run", "server==1.2.3"]),
        ] {
            assert!(validate_command(
                command,
                &args.into_iter().map(str::to_owned).collect::<Vec<_>>()
            )
            .is_ok());
        }
        for package in [
            "server",
            "server@latest",
            "server@^1.2.3",
            "server@1.2",
            "server@1.2.*",
            "server@https://evil.example",
        ] {
            assert!(validate_command("npx", &[package.into()]).is_err());
        }
        let imported = parse_json(r#"{"mcpServers":{"never-run":{"command":"/does/not/exist","args":["side-effect"],"env":{"API_KEY":"fixture-secret"}},"remote":{"url":"https://read.example/mcp","headers":{"Authorization":"Bearer fixture-secret"}}}}"#).unwrap();
        assert_eq!(imported.len(), 2);
        assert!(imported
            .iter()
            .any(|p| p.transport.command_line().as_deref() == Some("/does/not/exist side-effect")));
        for pending in imported {
            assert!(!serde_json::to_string(&pending.transport)
                .unwrap()
                .contains("fixture-secret"));
        }
    }
}
