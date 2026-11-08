//! The OS sandbox native Bash runs in (`docs/runtime/permissions.md`): writes
//! confined to the Loupist folder and this run's scratch, the app's data, the
//! Loupist's `.loupist` data, home credential locations and the host's
//! runtime sockets hidden, and the network off after third-party text. Rust
//! builds the argv prefix the extension puts before the shell: bubblewrap on
//! Linux. Other hosts have no sandbox, so a reply that read third-party text
//! refuses Bash there. A startup probe runs the sandbox against a private
//! folder once per app run, and only a passing probe enables it.
#![cfg_attr(not(unix), allow(dead_code))]

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

use super::native_policy::{NativeRoots, SandboxCommand};

const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_PROBE_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sandbox {
    /// Linux bubblewrap at this path.
    Bubblewrap(PathBuf),
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

/// The sandbox this host offers, before it is verified: bubblewrap on Linux,
/// nothing elsewhere.
pub fn detect() -> Option<Sandbox> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .chain([PathBuf::from("/usr/bin"), PathBuf::from("/usr/local/bin")])
        .map(|folder| folder.join("bwrap"))
        .find(|candidate| candidate.is_file())
        .map(Sandbox::Bubblewrap)
}

impl Sandbox {
    /// The prefix and environment for one command in this turn's folders.
    pub fn command(&self, roots: &NativeRoots, network: bool) -> SandboxCommand {
        let _ = std::fs::create_dir_all(roots.scratch.join("cache"));
        let Self::Bubblewrap(path) = self;
        let mut argv = vec![path.to_string_lossy().into_owned()];
        argv.extend(bwrap_arguments(roots, network));
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

/// The host's runtime socket folders: `/run` (D-Bus, `XDG_RUNTIME_DIR`
/// with the session bus, ssh-agent, gpg-agent, PulseAudio and Wayland
/// sockets, `docker.sock`) and `/var/run` when it is a folder of its own.
fn runtime_folders() -> Vec<PathBuf> {
    let mut folders = vec![PathBuf::from("/run")];
    let var_run = Path::new("/var/run");
    if var_run.is_dir()
        && !var_run
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        folders.push(var_run.to_path_buf());
    }
    folders
}

/// The file `/etc/resolv.conf` resolves to when it lives under a hidden
/// runtime folder (systemd-resolved, NetworkManager, resolvconf), bound back
/// read-only so name resolution keeps working online.
fn resolver_file(runtime: &[PathBuf]) -> Option<PathBuf> {
    let target = std::fs::canonicalize("/etc/resolv.conf").ok()?;
    runtime
        .iter()
        .any(|folder| target.starts_with(folder))
        .then_some(target)
}

/// Bubblewrap: the host read-only, the workspace and scratch writable, hidden
/// paths masked (a tmpfs over a folder, `/dev/null` over a file), and the
/// readable parts of hidden folders bound back read-only. Later binds win, so
/// scratch is bound after the app data root is masked. Each masking tmpfs is
/// remounted read-only last, once its children are mounted, so a write into
/// a hidden folder fails instead of landing in a tmpfs the command believes
/// is the folder. The workspace's `.git` is bound onto itself first: a mount
/// point cannot be renamed or removed, so the read-only `config` and `hooks`
/// inside it cannot be swapped for writable copies. The command runs in its
/// own pid namespace with bubblewrap as its init: `--new-session` moves the
/// command out of the process group Pi kills on abort, and killing the init
/// kills everything in the namespace, where `--die-with-parent` alone would
/// spare the command's descendants.
fn bwrap_arguments(roots: &NativeRoots, network: bool) -> Vec<String> {
    let text = |path: &Path| path.to_string_lossy().into_owned();
    let mut arguments: Vec<String> = ["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    arguments.extend(["--tmpfs".into(), "/tmp".into()]);
    let runtime = runtime_folders();
    for folder in &runtime {
        arguments.extend(["--tmpfs".into(), text(folder)]);
    }
    if let Some(resolver) = resolver_file(&runtime) {
        arguments.extend(["--ro-bind".into(), text(&resolver), text(&resolver)]);
    }
    arguments.extend([
        "--bind".into(),
        text(&roots.workspace),
        text(&roots.workspace),
    ]);
    let git_dir = roots.workspace.join(".git");
    if git_dir.is_dir() {
        arguments.extend(["--bind".into(), text(&git_dir), text(&git_dir)]);
    } else if git_dir.is_file() {
        arguments.extend(["--ro-bind".into(), text(&git_dir), text(&git_dir)]);
    }
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

#[derive(Debug, PartialEq, Eq)]
pub enum ProbeError {
    Setup,
    Launch,
    Timeout,
    /// Checks that did not come out as required: `name=result`.
    Failed(Vec<String>),
}

const PROBE_END: &str = "__LOUPIST_SANDBOX_PROBE_END__";
const PROBE_STDERR_BYTES: usize = 2048;

/// Runs the sandbox against a private folder laid out like a turn's roots: a
/// write inside the folder and scratch succeeds, a write outside, into the
/// data folder or into a git hook fails, hidden files read as absent or
/// empty, instruction files and attachments still read, and offline mode
/// cannot connect to a listener the probe opened while online mode can.
#[cfg(unix)]
pub fn probe(sandbox: &Sandbox) -> Result<(), ProbeError> {
    let temporary = probe_folder().map_err(|_| ProbeError::Setup)?;
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

/// A private folder for the probe's layout under the user's cache folder.
/// The sandbox replaces `/tmp` with an empty tmpfs, so a layout there would
/// read as absent whether or not the masks work; the cache folder is on the
/// host's own tree, where only a mask can hide a file.
#[cfg(unix)]
fn probe_folder() -> std::io::Result<tempfile::TempDir> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(".cache"))
        })
        .ok_or_else(|| std::io::Error::other("no cache folder"))?;
    let parent = cache.join("loupist");
    std::fs::create_dir_all(&parent)?;
    tempfile::Builder::new()
        .prefix("sandbox-probe-")
        .tempdir_in(parent)
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
            (workspace.join(".git/config"), "[core]"),
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
        .stderr(Stdio::piped());
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
    let mut stderr = child.stderr.take().ok_or(ProbeError::Launch)?;
    // The sandbox's own error (a mount or namespace it could not set up) is
    // the only explanation when the script never ran; keep its first line.
    let diagnostic = std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = (&mut stderr)
            .take(PROBE_STDERR_BYTES as u64)
            .read_to_end(&mut text);
        String::from_utf8_lossy(&text)
            .lines()
            .find(|line| !line.trim().is_empty())
            .map(|line| line.trim().chars().take(240).collect::<String>())
    });
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
    let diagnostic = diagnostic.join().ok().flatten();
    let output = output?;
    if !output
        .windows(PROBE_END.len())
        .any(|window| window == PROBE_END.as_bytes())
    {
        let mut failed = vec!["probe_incomplete".to_owned()];
        failed.extend(diagnostic.map(|line| format!("stderr={line}")));
        return Err(ProbeError::Failed(failed));
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
        assert!(argv.starts_with(
            "/usr/bin/bwrap --ro-bind / / --dev /dev --proc /proc --tmpfs /tmp --tmpfs /run"
        ));
        // The host's runtime sockets are gone; name resolution survives when
        // the resolver file lives there.
        if let Some(resolver) = resolver_file(&runtime_folders()) {
            let resolver = resolver.display().to_string();
            assert!(at("--tmpfs /run") < at(&format!("--ro-bind {resolver} {resolver}")));
        }
        assert!(at(&format!("--bind {ws} {ws}")) < at(&format!("--bind {ws}/.git {ws}/.git")));
        assert!(
            at(&format!("--bind {ws}/.git {ws}/.git"))
                < at(&format!("--ro-bind {ws}/.git/config {ws}/.git/config"))
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

        // A worktree's `.git` is a file naming the real git directory; it is
        // bound read-only so the pointer cannot be redirected.
        std::fs::remove_dir_all(roots.workspace.join(".git")).unwrap();
        std::fs::write(roots.workspace.join(".git"), "gitdir: ../main/.git").unwrap();
        let argv = sandbox.command(&roots, false).argv.join(" ");
        assert!(
            argv.contains(&format!("--ro-bind {ws}/.git {ws}/.git")),
            "{argv}"
        );
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

    /// Inside the real sandbox the workspace's `.git` cannot be renamed or
    /// replaced, the host's runtime sockets are gone, and name resolution
    /// still works online when the host can resolve.
    #[cfg(target_os = "linux")]
    #[test]
    fn bubblewrap_pins_the_git_directory_and_hides_runtime_sockets() {
        let Some(sandbox) = detect() else {
            eprintln!("skipping: bubblewrap (bwrap) is not installed on this host");
            return;
        };
        let temporary = probe_folder().unwrap();
        let layout = ProbeLayout::create(temporary.path()).unwrap();
        let ws = layout.roots.workspace.display().to_string();
        // An entry of the host's `/run` that is not on the way to the
        // resolver file, which is bound back.
        let resolver = resolver_file(&runtime_folders());
        let host_run_entry = std::fs::read_dir("/run").ok().and_then(|entries| {
            entries.flatten().map(|entry| entry.path()).find(|path| {
                resolver
                    .as_deref()
                    .is_none_or(|resolver| !resolver.starts_with(path))
            })
        });
        let script = format!(
            "mv '{ws}/.git' '{ws}/.git-old' 2>/dev/null && echo rename=ok || echo rename=fail\n\
             rmdir '{ws}/.git' 2>/dev/null && echo rmdir=ok || echo rmdir=fail\n\
             (: > '{ws}/.git/config') 2>/dev/null && echo config=ok || echo config=fail\n\
             [ -e '{entry}' ] && echo run_entry=visible || echo run_entry=hidden\n\
             [ -e /run/user ] && echo run_user=visible || echo run_user=hidden\n\
             getent hosts localhost >/dev/null 2>&1 && echo localhost=ok || echo localhost=fail\n\
             echo {end}\n",
            entry = host_run_entry
                .as_deref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "/run/nothing".into()),
            end = PROBE_END
        );
        let output = run_probe(&sandbox, &layout.roots, true, &script).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert!(lines.contains(&"rename=fail"), "{output}");
        assert!(lines.contains(&"rmdir=fail"), "{output}");
        assert!(lines.contains(&"config=fail"), "{output}");
        assert!(lines.contains(&"run_entry=hidden"), "{output}");
        assert!(lines.contains(&"run_user=hidden"), "{output}");
        assert!(lines.contains(&"localhost=ok"), "{output}");
        assert!(layout
            .roots
            .workspace
            .join(".git/hooks/pre-commit")
            .is_file());
        if std::process::Command::new("getent")
            .args(["hosts", "example.com"])
            .output()
            .is_ok_and(|output| output.status.success())
        {
            let script = format!(
                "getent hosts example.com >/dev/null 2>&1 && echo dns=ok || echo dns=fail\n\
                 echo {PROBE_END}\n"
            );
            let output = run_probe(&sandbox, &layout.roots, true, &script).unwrap();
            assert!(output.contains("dns=ok"), "{output}");
        }
    }

    /// A sandbox that cannot start never runs the script; the probe says so
    /// and carries the sandbox's own error, which the startup log prints.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_sandbox_that_cannot_start_reports_its_own_error() {
        let error = probe(&Sandbox::Bubblewrap("/bin/sh".into())).unwrap_err();
        let ProbeError::Failed(lines) = error else {
            panic!("expected a failed probe, got {error:?}");
        };
        assert_eq!(lines.first().map(String::as_str), Some("probe_incomplete"));
        assert!(
            lines
                .get(1)
                .is_some_and(|line| line.starts_with("stderr=/bin/sh")),
            "{lines:?}"
        );
    }

    /// A real bubblewrap that lost the masks outside the workspace (no app
    /// data tmpfs, no credential masks) fails the probe on those reads: the
    /// layout lives on the host's tree, not under the `/tmp` the sandbox
    /// replaces, so only a mask can make a secret read as absent.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_sandbox_without_the_credential_and_app_data_masks_fails_the_probe() {
        let Some(Sandbox::Bubblewrap(bwrap)) = detect() else {
            eprintln!("skipping: bubblewrap (bwrap) is not installed on this host");
            return;
        };
        let temporary = tempfile::tempdir().unwrap();
        let wrapper = temporary.path().join("bwrap-without-masks");
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/bash\n\
                 out=()\n\
                 while [ $# -gt 0 ]; do\n\
                 \tcase \"$1\" in\n\
                 \t\t--tmpfs|--remount-ro) case \"$2\" in /tmp|/run|/var/run|*/ws/*) ;; *) shift 2; continue ;; esac ;;\n\
                 \t\t--ro-bind) if [ \"$2\" = /dev/null ]; then shift 3; continue; fi ;;\n\
                 \tesac\n\
                 \tout+=(\"$1\"); shift\n\
                 done\n\
                 exec '{}' \"${{out[@]}}\"\n",
                bwrap.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let error = probe(&Sandbox::Bubblewrap(wrapper)).unwrap_err();
        let ProbeError::Failed(lines) = error else {
            panic!("expected a failed probe, got {error:?}");
        };
        for check in ["read_app=ok", "read_netrc=ok", "read_ssh=ok"] {
            assert!(lines.iter().any(|line| line == check), "{check}: {lines:?}");
        }
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
