use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::{
    app::CommandError,
    commands::map_internal,
    pi::{PI_DARWIN_ARM64_ARCHIVE_SHA256, PI_VERSION},
    util::sha256,
};

pub(crate) fn install(directory: &Path, source: &str) -> Result<(PathBuf, PathBuf), CommandError> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let executable = directory.join("fixture-pi");
    let script = directory.join("fixture-pi.py");
    fs::create_dir_all(directory).map_err(map_internal)?;
    fs::write(&script, source).map_err(map_internal)?;
    let compiler = crate::process_lease::macos_commands::output(
        Command::new("/usr/bin/cc")
            .args([
                "-std=c11",
                "-Wall",
                "-Werror",
                "-DFIXTURE_PYTHON=\"/usr/bin/python3\"",
            ])
            .arg(format!(
                "-DFIXTURE_SCRIPT={}",
                serde_json::to_string(&script).map_err(map_internal)?
            ))
            .arg(manifest.join("src/commands/tests/fixtures/native_activity_launcher.c"))
            .arg("-o")
            .arg(&executable),
    )
    .map_err(map_internal)?;
    if !compiler.status.success() {
        return Err(CommandError::internal(format!(
            "could not compile the RPC fixture: {}",
            String::from_utf8_lossy(&compiler.stderr)
        )));
    }
    let runtime_dir = directory.join("fixture-runtime");
    fs::create_dir_all(&runtime_dir).map_err(map_internal)?;
    for (name, value) in [
        (".pi-version", PI_VERSION.to_owned()),
        (
            ".pi-archive.sha256",
            PI_DARWIN_ARM64_ARCHIVE_SHA256.to_owned(),
        ),
        (
            ".pi-executable.sha256",
            sha256(&fs::read(&executable).map_err(map_internal)?),
        ),
        (
            "package.json",
            serde_json::json!({"version": PI_VERSION}).to_string(),
        ),
    ] {
        fs::write(runtime_dir.join(name), value).map_err(map_internal)?;
    }
    Ok((executable, runtime_dir))
}
