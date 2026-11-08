//! The OS sandbox native Bash runs in (`docs/runtime/permissions.md`): writes
//! confined to the Loupist folder and this run's scratch, the app's data, the
//! Loupist's `.loupist` data and home credential locations hidden, and the
//! network off after third-party text. Rust builds the argv prefix the
//! extension puts before the shell: bubblewrap on Linux, `sandbox-exec` with
//! a generated profile on macOS. A startup probe runs the sandbox against a
//! temporary folder once per app run, and only a passing probe enables it.
#![cfg_attr(not(unix), allow(dead_code))]

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

use super::native_policy::{NativeRoots, Reach, SandboxCommand};

const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_PROBE_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sandbox {
    /// Linux bubblewrap at this path.
    Bubblewrap(PathBuf),
    /// macOS `/usr/bin/sandbox-exec` with a generated profile.
    SandboxExec,
}

static VERIFIED: OnceLock<Option<Sandbox>> = OnceLock::new();

/// Probe in the background at startup so the first Chat does not wait.
pub(crate) fn prewarm() {
    let _ = std::thread::Builder::new()
        .name("bash-sandbox-probe".into())
        .spawn(|| {
            let _ = VERIFIED.get_or_init(detect_and_probe);
        });
}

/// The verified sandbox for a Chat turn, or None when this host has none.
/// The first turn can arrive while the startup probe still runs; waiting for
/// it happens off the async workers and gives up after the probe's own
/// deadline, in which case the turn runs without a sandbox.
pub(crate) async fn verified_for_launch() -> Option<Sandbox> {
    crate::pi::shell_path::cached_off_runtime(
        &VERIFIED,
        detect_and_probe,
        PROBE_TIMEOUT + Duration::from_secs(1),
    )
    .await
    .and_then(Clone::clone)
}

fn detect_and_probe() -> Option<Sandbox> {
    let sandbox = detect()?;
    match probe(&sandbox) {
        Ok(()) => Some(sandbox),
        Err(error) => {
            eprintln!("Bash sandbox is not used: the startup probe failed: {error:?}");
            None
        }
    }
}

/// The sandbox this host offers, before it is verified.
pub fn detect() -> Option<Sandbox> {
    if cfg!(target_os = "linux") {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .chain([PathBuf::from("/usr/bin"), PathBuf::from("/usr/local/bin")])
            .map(|folder| folder.join("bwrap"))
            .find(|candidate| candidate.is_file())
            .map(Sandbox::Bubblewrap)
    } else if cfg!(target_os = "macos") {
        Path::new("/usr/bin/sandbox-exec")
            .is_file()
            .then_some(Sandbox::SandboxExec)
    } else {
        None
    }
}

impl Sandbox {
    /// The prefix and environment for one command in this turn's folders.
    pub fn command(&self, roots: &NativeRoots, network: bool) -> SandboxCommand {
        let _ = std::fs::create_dir_all(roots.scratch.join("cache"));
        let argv = match self {
            Self::Bubblewrap(path) => {
                let mut argv = vec![path.to_string_lossy().into_owned()];
                argv.extend(bwrap_arguments(roots, network));
                argv
            }
            Self::SandboxExec => vec![
                "/usr/bin/sandbox-exec".into(),
                "-p".into(),
                profile(roots, network),
            ],
        };
        SandboxCommand {
            argv,
            env: cache_environment(&roots.scratch),
        }
    }
}

/// Tool caches land in scratch, where the sandbox can write.
fn cache_environment(scratch: &Path) -> BTreeMap<String, String> {
    let cache = scratch.join("cache");
    [
        ("XDG_CACHE_HOME", cache.clone()),
        ("PIP_CACHE_DIR", cache.join("pip")),
        ("UV_CACHE_DIR", cache.join("uv")),
        ("npm_config_cache", cache.join("npm")),
        ("MPLCONFIGDIR", cache.join("matplotlib")),
    ]
    .into_iter()
    .map(|(name, path)| (name.to_owned(), path.to_string_lossy().into_owned()))
    .collect()
}

/// Everything the sandbox hides from a command: the Loupist's data folder,
/// the app data root and the home credential locations that exist.
fn hidden(roots: &NativeRoots) -> Vec<PathBuf> {
    let mut paths = vec![roots.loupist_data.clone(), roots.app_data.clone()];
    paths.extend(roots.credential_paths().into_iter().map(|(path, _)| path));
    paths.retain(|path| path.exists());
    paths
}

/// What stays readable inside a hidden folder: instruction files and Skills,
/// this Chat's attachments.
fn readable_again(roots: &NativeRoots) -> Vec<PathBuf> {
    let mut paths = roots.loupist_data_instructions().to_vec();
    paths.extend(roots.global_instructions());
    paths.push(roots.attachments.clone());
    paths.retain(|path| path.exists());
    paths
}

/// Bubblewrap: the host read-only, the workspace and scratch writable, hidden
/// paths masked (a tmpfs over a folder, `/dev/null` over a file), and the
/// readable parts of hidden folders bound back read-only. Later binds win, so
/// scratch is bound after the app data root is masked. Each masking tmpfs is
/// remounted read-only last, once its children are mounted, so a write into
/// a hidden folder fails instead of landing in a tmpfs the command believes
/// is the folder. The command runs in its own pid namespace with bubblewrap
/// as its init: `--new-session` moves the command out of the process group
/// Pi kills on abort, and killing the init kills everything in the namespace,
/// where `--die-with-parent` alone would spare the command's descendants.
fn bwrap_arguments(roots: &NativeRoots, network: bool) -> Vec<String> {
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let mut arguments: Vec<String> = ["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    arguments.extend(["--tmpfs".into(), "/tmp".into()]);
    arguments.extend([
        "--bind".into(),
        text(&roots.workspace),
        text(&roots.workspace),
    ]);
    let mut masked_folders = Vec::new();
    for path in hidden(roots) {
        if path.is_dir() {
            arguments.extend(["--tmpfs".into(), text(&path)]);
            masked_folders.push(path);
        } else {
            arguments.extend(["--ro-bind".into(), "/dev/null".into(), text(&path)]);
        }
    }
    let read_only = readable_again(roots).into_iter().chain(
        roots
            .workspace_git_config()
            .into_iter()
            .filter(|path| path.exists()),
    );
    for path in read_only {
        arguments.extend(["--ro-bind".into(), text(&path), text(&path)]);
    }
    arguments.extend(["--bind".into(), text(&roots.scratch), text(&roots.scratch)]);
    for path in masked_folders {
        arguments.extend(["--remount-ro".into(), text(&path)]);
    }
    if !network {
        arguments.push("--unshare-net".into());
    }
    arguments.extend([
        "--unshare-pid".into(),
        "--die-with-parent".into(),
        "--new-session".into(),
        "--chdir".into(),
        text(&roots.workspace),
        "--".into(),
    ]);
    arguments
}

/// The macOS seatbelt profile. Every rule is a deny with its own exclusions
/// (`require-all` / `require-not`), so no allow and deny ever match the same
/// operation and path and nothing depends on rule precedence; `allow default`
/// covers the rest.
pub fn profile(roots: &NativeRoots, network: bool) -> String {
    let subpath = |path: &Path| format!("(subpath {})", sbpl_string(path));
    let literal = |path: &Path| format!("(literal {})", sbpl_string(path));
    let mut rules = vec!["(version 1)".to_owned(), "(allow default)".to_owned()];
    rules.push(format!(
        "(deny file-write* (require-not (require-any {} {} (literal \"/dev/null\") (literal \"/dev/stdout\") (literal \"/dev/stderr\") (regex #\"^/dev/tty\") (regex #\"^/dev/fd/\"))))",
        subpath(&roots.workspace),
        subpath(&roots.scratch)
    ));
    let [git_config, git_hooks] = roots.workspace_git_config();
    rules.push(format!(
        "(deny file-write* {} {} {})",
        subpath(&roots.loupist_data),
        subpath(&git_hooks),
        literal(&git_config)
    ));
    let [skills, agents, config] = roots.loupist_data_instructions();
    rules.push(format!(
        "(deny file-read* (require-all {} (require-not (require-any {} {} {} {}))))",
        subpath(&roots.loupist_data),
        subpath(&skills),
        literal(&agents),
        literal(&config),
        subpath(&roots.attachments)
    ));
    let [global_skills, global_agents] = roots.global_instructions();
    rules.push(format!(
        "(deny file-read* (require-all {} (require-not (require-any {} {} {}))))",
        subpath(&roots.app_data),
        subpath(&roots.scratch),
        subpath(&global_skills),
        literal(&global_agents)
    ));
    let credentials: Vec<String> = roots
        .credential_paths()
        .iter()
        .map(|(path, reach)| match reach {
            Reach::Tree => subpath(path),
            Reach::File => literal(path),
        })
        .collect();
    if !credentials.is_empty() {
        rules.push(format!("(deny file-read* {})", credentials.join(" ")));
    }
    if !network {
        rules.push("(deny network*)".to_owned());
    }
    rules.join("\n")
}

fn sbpl_string(path: &Path) -> String {
    let mut text = String::from("\"");
    for character in path.to_string_lossy().chars() {
        if character == '"' || character == '\\' {
            text.push('\\');
        }
        text.push(character);
    }
    text.push('"');
    text
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProbeError {
    Setup,
    Launch,
    Timeout,
    /// Checks that did not come out as required: `name=result`.
    Failed(Vec<String>),
}

const PROBE_END: &str = "__LOUPIST_SANDBOX_PROBE_END__";

/// Runs the sandbox against a temporary folder laid out like a turn's roots:
/// a write inside the folder and scratch succeeds, a write outside, into
/// the data folder or into a git hook fails, hidden files read as absent or
/// empty, instruction files and attachments still read, and offline mode
/// cannot connect to a listener the probe opened while online mode can.
#[cfg(unix)]
pub fn probe(sandbox: &Sandbox) -> Result<(), ProbeError> {
    let temporary = tempfile::tempdir().map_err(|_| ProbeError::Setup)?;
    let layout = ProbeLayout::create(temporary.path()).map_err(|_| ProbeError::Setup)?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|_| ProbeError::Setup)?;
    listener
        .set_nonblocking(true)
        .map_err(|_| ProbeError::Setup)?;
    let port = listener.local_addr().map_err(|_| ProbeError::Setup)?.port();
    for network in [false, true] {
        let output = run_probe(sandbox, &layout.roots, network, &layout.script(port))?;
        let mut failed: Vec<String> = output
            .lines()
            .filter(|line| !expected(line, network))
            .map(str::to_owned)
            .collect();
        let connected = listener.accept().is_ok();
        if connected != network {
            failed.push(format!("listener_connected={connected}"));
        }
        if !layout.inside.is_file() {
            failed.push("inside_file_missing".into());
        }
        if layout.outside.exists() {
            failed.push("outside_file_written".into());
        }
        if !failed.is_empty() {
            return Err(ProbeError::Failed(failed));
        }
        let _ = std::fs::remove_file(&layout.inside);
    }
    Ok(())
}

/// What each probe line must say. Online, only the connection differs.
fn expected(line: &str, network: bool) -> bool {
    let Some((name, result)) = line.split_once('=') else {
        return true;
    };
    let ok = match name {
        "write_inside" | "write_scratch" | "read_skill" | "read_attachment" | "read_hook" => true,
        "connect" => network,
        "write_outside" | "write_data" | "write_hook" | "read_app" | "read_netrc" | "read_ssh"
        | "read_db" => false,
        _ => return true,
    };
    result == if ok { "ok" } else { "fail" }
}

#[cfg(unix)]
struct ProbeLayout {
    roots: NativeRoots,
    inside: PathBuf,
    outside: PathBuf,
    home: PathBuf,
}

#[cfg(unix)]
impl ProbeLayout {
    fn create(root: &Path) -> std::io::Result<Self> {
        let root = root.canonicalize()?;
        let workspace = root.join("ws");
        let data = workspace.join(".loupist");
        let app = root.join("app");
        let scratch = app.join("scratch");
        let attachments = data.join("attachments/chat");
        let home = root.join("home");
        for (file, content) in [
            (data.join("db.sqlite3"), "dbsecret"),
            (data.join("skills/x/SKILL.md"), "skillok"),
            (attachments.join("brief.md"), "attok"),
            (app.join("pi/auth.json"), "authsecret"),
            (home.join(".netrc"), "netrcsecret"),
            (home.join(".ssh/id"), "sshsecret"),
            (workspace.join(".git/hooks/pre-commit"), "hookok"),
        ] {
            std::fs::create_dir_all(file.parent().expect("file has a parent"))?;
            std::fs::write(file, content)?;
        }
        std::fs::create_dir_all(&scratch)?;
        std::fs::create_dir_all(root.join("outside"))?;
        Ok(Self {
            roots: NativeRoots::new(
                &workspace,
                &app,
                &app,
                &data,
                &scratch,
                &attachments,
                Some(&home),
            ),
            inside: workspace.join("inside.txt"),
            outside: root.join("outside/outside.txt"),
            home,
        })
    }

    fn script(&self, port: u16) -> String {
        let q = |path: &Path| shell_quote(&path.to_string_lossy());
        let roots = &self.roots;
        format!(
            "w() {{ ( : > \"$1\" ) 2>/dev/null && echo \"$2=ok\" || echo \"$2=fail\"; }}\n\
             r() {{ grep -qs \"$2\" \"$1\" 2>/dev/null && echo \"$3=ok\" || echo \"$3=fail\"; }}\n\
             w {inside} write_inside\n\
             w {scratch}/scratch.txt write_scratch\n\
             w {outside} write_outside\n\
             w {data}/data.txt write_data\n\
             w {ws}/.git/hooks/post-checkout write_hook\n\
             r {app}/pi/auth.json authsecret read_app\n\
             r {home}/.netrc netrcsecret read_netrc\n\
             r {home}/.ssh/id sshsecret read_ssh\n\
             r {data}/db.sqlite3 dbsecret read_db\n\
             r {data}/skills/x/SKILL.md skillok read_skill\n\
             r {attachments}/brief.md attok read_attachment\n\
             r {ws}/.git/hooks/pre-commit hookok read_hook\n\
             (exec 3<>/dev/tcp/127.0.0.1/{port}) 2>/dev/null && echo connect=ok || echo connect=fail\n\
             echo {end}\n",
            inside = q(&self.inside),
            scratch = q(&roots.scratch),
            outside = q(&self.outside),
            data = q(&roots.loupist_data),
            ws = q(&roots.workspace),
            app = q(&roots.app_data),
            home = q(&self.home),
            attachments = q(&roots.attachments),
            end = PROBE_END,
        )
    }
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Runs `/bin/bash -c <script>` under the sandbox with a cleared
/// environment, no stdin and a deadline; the whole process group is killed
/// at the deadline.
#[cfg(unix)]
fn run_probe(
    sandbox: &Sandbox,
    roots: &NativeRoots,
    network: bool,
    script: &str,
) -> Result<String, ProbeError> {
    use std::{
        io::Read,
        os::unix::process::CommandExt,
        process::{Command, Stdio},
        sync::mpsc,
    };
    let SandboxCommand { argv, env } = sandbox.command(roots, network);
    let (program, prefix) = argv.split_first().ok_or(ProbeError::Launch)?;
    let mut command = Command::new(program);
    command
        .args(prefix)
        .args(["/bin/bash", "-c", script])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &roots.workspace)
        .env("TMPDIR", &roots.scratch)
        .envs(env)
        .current_dir(&roots.workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe and accesses no Rust state.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|_| ProbeError::Launch)?;
    let group = i32::try_from(child.id()).map_err(|_| ProbeError::Launch)?;
    let mut stdout = child.stdout.take().ok_or(ProbeError::Launch)?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut chunk = [0_u8; 4096];
        let result = loop {
            match stdout.read(&mut chunk) {
                Ok(0) => break Ok(output),
                Ok(read) => {
                    output.extend_from_slice(&chunk[..read]);
                    if output.len() > MAX_PROBE_OUTPUT_BYTES {
                        break Err(ProbeError::Launch);
                    }
                    if output
                        .windows(PROBE_END.len())
                        .any(|window| window == PROBE_END.as_bytes())
                    {
                        break Ok(output);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break Err(ProbeError::Launch),
            }
        };
        let _ = sender.send(result);
    });
    let output = receiver
        .recv_timeout(PROBE_TIMEOUT)
        .unwrap_or(Err(ProbeError::Timeout));
    let _ = crate::process_lease::signal_process_group(group, libc::SIGKILL);
    let _ = child.wait();
    let output = output?;
    if !output
        .windows(PROBE_END.len())
        .any(|window| window == PROBE_END.as_bytes())
    {
        return Err(ProbeError::Failed(vec!["probe_incomplete".into()]));
    }
    Ok(String::from_utf8_lossy(&output).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots(temporary: &Path) -> NativeRoots {
        let workspace = temporary.join("Research");
        let app = temporary.join("Library/Application Support/Loupist");
        NativeRoots::new(
            &workspace,
            &app,
            &app,
            &workspace.join(".loupist"),
            &app.join("loupists/a/pi-runtime/chat/temp"),
            &workspace.join(".loupist/attachments/chat"),
            Some(temporary),
        )
    }

    #[test]
    fn the_profile_denies_by_exclusion_and_goes_offline_after_third_party_text() {
        let temporary = tempfile::tempdir().unwrap();
        let home = temporary.path().canonicalize().unwrap();
        let roots = roots(&home);
        let online = profile(&roots, true);
        let offline = profile(&roots, false);
        let ws = home.join("Research").display().to_string();
        let scratch = roots.scratch.display().to_string();
        assert!(online.starts_with("(version 1)\n(allow default)\n"));
        assert!(online.contains(&format!(
            "(deny file-write* (require-not (require-any (subpath \"{ws}\") (subpath \"{scratch}\")"
        )));
        assert!(online.contains(&format!(
            "(deny file-write* (subpath \"{ws}/.loupist\") (subpath \"{ws}/.git/hooks\") (literal \"{ws}/.git/config\"))"
        )));
        assert!(online.contains(&format!(
            "(deny file-read* (require-all (subpath \"{ws}/.loupist\") (require-not (require-any (subpath \"{ws}/.loupist/skills\") (literal \"{ws}/.loupist/AGENTS.md\") (literal \"{ws}/.loupist/config.toml\") (subpath \"{ws}/.loupist/attachments/chat\"))))"
        )));
        assert!(online.contains(&format!(
            "(deny file-read* (require-all (subpath \"{}\") (require-not (require-any (subpath \"{scratch}\") (subpath \"{}/skills\") (literal \"{}/AGENTS.md\"))))",
            roots.app_data.display(),
            roots.app_data.display(),
            roots.app_data.display()
        )));
        assert!(online.contains(&format!(
            "(deny file-read* (subpath \"{}/.ssh\") (subpath \"{}/.gnupg\")",
            home.display(),
            home.display()
        )));
        assert!(online.contains(&format!("(literal \"{}/.netrc\")", home.display())));
        // No rule allows anything but the default, so none can shadow a deny.
        assert_eq!(online.matches("(allow ").count(), 1);
        assert!(!online.contains("(deny network*)"));
        assert!(offline.ends_with("(deny network*)"));
        assert_eq!(
            sbpl_string(Path::new("/Users/me/Re\"search\\x")),
            "\"/Users/me/Re\\\"search\\\\x\""
        );
    }

    #[test]
    fn bubblewrap_masks_before_it_binds_scratch_back_and_unshares_the_network_offline() {
        let temporary = tempfile::tempdir().unwrap();
        let home = temporary.path().canonicalize().unwrap();
        let roots = roots(&home);
        for path in [
            roots.scratch.clone(),
            roots.loupist_data.join("skills"),
            roots.attachments.clone(),
            roots.workspace.join(".git/hooks"),
            home.join(".ssh"),
        ] {
            std::fs::create_dir_all(path).unwrap();
        }
        std::fs::write(home.join(".netrc"), "x").unwrap();
        std::fs::write(roots.workspace.join(".git/config"), "x").unwrap();
        let sandbox = Sandbox::Bubblewrap("/usr/bin/bwrap".into());
        let command = sandbox.command(&roots, false);
        let argv = command.argv.join(" ");
        let at = |needle: &str| {
            argv.find(needle)
                .unwrap_or_else(|| panic!("{needle}\n{argv}"))
        };
        let app = roots.app_data.display().to_string();
        let ws = roots.workspace.display().to_string();
        assert!(
            argv.starts_with("/usr/bin/bwrap --ro-bind / / --dev /dev --proc /proc --tmpfs /tmp")
        );
        assert!(at(&format!("--bind {ws} {ws}")) < at(&format!("--tmpfs {ws}/.loupist")));
        assert!(
            at(&format!("--tmpfs {app}")) < at(&format!("--bind {0} {0}", roots.scratch.display()))
        );
        assert!(
            at(&format!("--tmpfs {ws}/.loupist")) < at(&format!("--ro-bind {ws}/.loupist/skills"))
        );
        at(&format!("--ro-bind {0} {0}", roots.attachments.display()));
        at(&format!("--tmpfs {}/.ssh", home.display()));
        at(&format!("--ro-bind /dev/null {}/.netrc", home.display()));
        at(&format!("--ro-bind {ws}/.git/config {ws}/.git/config"));
        at(&format!("--ro-bind {ws}/.git/hooks {ws}/.git/hooks"));
        assert!(
            at(&format!("--bind {0} {0}", roots.scratch.display()))
                < at(&format!("--remount-ro {app}"))
        );
        at(&format!("--remount-ro {ws}/.loupist"));
        at(&format!("--remount-ro {}/.ssh", home.display()));
        assert!(argv.ends_with(&format!(
            "--unshare-net --unshare-pid --die-with-parent --new-session --chdir {ws} --"
        )));
        assert!(!sandbox
            .command(&roots, true)
            .argv
            .contains(&"--unshare-net".to_owned()));
        assert_eq!(
            command.env.get("XDG_CACHE_HOME").map(String::as_str),
            Some(roots.scratch.join("cache").to_str().unwrap())
        );
        assert!(roots.scratch.join("cache").is_dir());
    }

    /// The real sandbox on this host: everything the probe requires, both
    /// online and offline. Skipped where bubblewrap is not installed.
    #[cfg(target_os = "linux")]
    #[test]
    fn bubblewrap_confines_a_command_as_the_probe_requires() {
        let Some(sandbox) = detect() else {
            eprintln!("skipping: bubblewrap (bwrap) is not installed on this host");
            return;
        };
        assert_eq!(probe(&sandbox), Ok(()));
    }

    /// Each probe check fails on its own when the sandbox is missing, so a
    /// passing probe says something about every rule.
    #[cfg(target_os = "linux")]
    #[test]
    fn without_a_sandbox_the_probe_reports_every_broken_rule() {
        let temporary = tempfile::tempdir().unwrap();
        let layout = ProbeLayout::create(temporary.path()).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let output = std::process::Command::new("/bin/bash")
            .arg("-c")
            .arg(layout.script(port))
            .output()
            .unwrap();
        let output = String::from_utf8_lossy(&output.stdout);
        let broken: Vec<&str> = output
            .lines()
            .filter(|line| !expected(line, false))
            .collect();
        assert_eq!(
            broken,
            [
                "write_outside=ok",
                "write_data=ok",
                "write_hook=ok",
                "read_app=ok",
                "read_netrc=ok",
                "read_ssh=ok",
                "read_db=ok",
                "connect=ok"
            ],
            "{output}"
        );
    }
}
