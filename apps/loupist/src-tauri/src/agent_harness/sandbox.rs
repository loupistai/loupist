//! The OS sandbox native Bash runs in (`docs/runtime/permissions.md`): writes
//! confined to the Loupist folder and this run's scratch, the app's data, the
//! Loupist's `.loupist` data, home credential locations and the host's
//! runtime sockets hidden, and the network off after third-party text. After
//! third-party text the Loupist folder itself is read-only and only its
//! existing subfolders, `outputs` and scratch take writes, so a command can
//! leave nothing in the files a later reply loads as instructions. Rust
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
/// The most workspace subfolders bound writable after third-party text; a
/// folder with more top-level subfolders keeps only `outputs` writable.
pub const MAX_WRITABLE_CHILDREN: usize = 256;
/// The workspace folder a command after third-party text can always write,
/// created before the command when it is missing.
pub const OUTPUTS_FOLDER: &str = "outputs";

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

/// The real paths of the entries that exist, each once. A mount destination
/// must be the real path: bubblewrap resolves an absolute link in it against
/// its own setup root, not the new root, and the mount fails. So a link to a
/// file is masked at the file, a link to a folder at the folder, and a
/// dangling link not at all; the link itself still leads to the mask.
fn canonical(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut real_paths: Vec<PathBuf> = Vec::new();
    for path in paths {
        if let Ok(real) = std::fs::canonicalize(&path) {
            if !real_paths.contains(&real) {
                real_paths.push(real);
            }
        }
    }
    real_paths
}

/// Everything the sandbox hides from a command: the Loupist's data folder,
/// the app data root and the home credential locations that exist.
fn hidden(roots: &NativeRoots) -> Vec<PathBuf> {
    let mut paths = vec![roots.loupist_data.clone(), roots.app_data.clone()];
    paths.extend(roots.credential_paths().into_iter().map(|(path, _)| path));
    canonical(paths)
}

/// What stays readable inside a hidden folder: instruction files and Skills,
/// this Chat's attachments.
fn readable_again(roots: &NativeRoots) -> Vec<PathBuf> {
    let mut paths = roots.loupist_data_instructions().to_vec();
    paths.extend(roots.global_instructions());
    paths.push(roots.attachments.clone());
    canonical(paths)
}

/// What a command after third-party text may write in the workspace: every
/// existing top-level entry that is a folder itself (not a link to one) and
/// whose name does not start with `.`, plus `outputs`, created here when it
/// is missing. Beyond `MAX_WRITABLE_CHILDREN` such folders, only `outputs`
/// stays writable. A name that is not valid UTF-8 cannot be passed to
/// bubblewrap and stays read-only.
fn writable_offline(workspace: &Path) -> Vec<PathBuf> {
    let outputs = workspace.join(OUTPUTS_FOLDER);
    let _ = std::fs::create_dir(&outputs);
    let mut children: Vec<PathBuf> = std::fs::read_dir(workspace)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| !name.starts_with('.'))
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
        })
        .map(|entry| entry.path())
        .collect();
    children.sort();
    if children.len() > MAX_WRITABLE_CHILDREN {
        children.retain(|child| *child == outputs);
    }
    children
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
/// is the folder. Online, a workspace `.git` that is a folder of its own is
/// bound onto itself first: a mount point cannot be renamed or removed, so
/// the read-only `config` and `hooks` inside it cannot be swapped for
/// writable copies. Every mask and bind-back names a real path
/// (`canonical`), since bubblewrap cannot mount onto a link. Offline, the
/// workspace is bound read-only and only `writable_offline` folders are
/// bound back writable, so its top level, every dot entry (`.git` included)
/// and the root instruction files take no write at all. The command runs in
/// its own pid namespace with bubblewrap as its init: `--new-session` moves
/// the command out of the process group Pi kills on abort, and killing the
/// init kills everything in the namespace, where `--die-with-parent` alone
/// would spare the command's descendants.
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
    // A `.git` that is a link is not pinned: a mount point cannot be made of a
    // link, and the pin's point is the entry the workspace spells.
    let git_dir = roots.workspace.join(".git");
    let git_kind = git_dir
        .symlink_metadata()
        .map(|metadata| metadata.file_type());
    let git_folder = git_kind.as_ref().is_ok_and(|kind| kind.is_dir());
    if network {
        arguments.extend([
            "--bind".into(),
            text(&roots.workspace),
            text(&roots.workspace),
        ]);
        if git_folder {
            arguments.extend(["--bind".into(), text(&git_dir), text(&git_dir)]);
        } else if git_kind.is_ok_and(|kind| kind.is_file()) {
            arguments.extend(["--ro-bind".into(), text(&git_dir), text(&git_dir)]);
        }
    } else {
        arguments.extend([
            "--ro-bind".into(),
            text(&roots.workspace),
            text(&roots.workspace),
        ]);
        for folder in writable_offline(&roots.workspace) {
            arguments.extend(["--bind".into(), text(&folder), text(&folder)]);
        }
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
    let git_config = canonical(
        (network && git_folder)
            .then(|| roots.workspace_git_config())
            .into_iter()
            .flatten(),
    );
    for path in readable_again(roots).into_iter().chain(git_config) {
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
/// write into an existing subfolder, into `outputs` and into scratch
/// succeeds; a write outside, into the data folder or into a git hook fails;
/// `.git` cannot be renamed; hidden files read as absent or empty;
/// instruction files and attachments still read; `/run` holds nothing but
/// the resolver file bound back; offline mode cannot connect to a listener
/// the probe opened while online mode can; and offline a write at the
/// folder's top level or into a dot folder fails where online it succeeds.
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
        if layout.top.is_file() != network {
            failed.push(format!("top_file_present={}", !network));
        }
        if !layout.inside.is_file() {
            failed.push("inside_file_missing".into());
        }
        if layout.outside.exists() {
            failed.push("outside_file_written".into());
        }
        if !layout
            .roots
            .workspace
            .join(".git/hooks/pre-commit")
            .is_file()
        {
            failed.push("git_dir_moved".into());
        }
        if !failed.is_empty() {
            return Err(ProbeError::Failed(failed));
        }
        let _ = std::fs::remove_file(&layout.inside);
        let _ = std::fs::remove_file(&layout.top);
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

/// What each probe line must say. Online, the connection and the writes at
/// the workspace's top level and into a dot folder differ.
fn expected(line: &str, network: bool) -> bool {
    let Some((name, result)) = line.split_once('=') else {
        return true;
    };
    let ok = match name {
        "write_inside" | "write_outputs" | "write_scratch" | "read_skill" | "read_attachment"
        | "read_hook" | "run_masked" => true,
        "connect" | "write_top" | "write_dot" => network,
        "write_outside" | "write_data" | "write_hook" | "rename_git" | "read_app"
        | "read_netrc" | "read_ssh" | "read_db" => false,
        _ => return true,
    };
    result == if ok { "ok" } else { "fail" }
}

#[cfg(unix)]
struct ProbeLayout {
    roots: NativeRoots,
    /// A file at the workspace's top level: written online, refused offline.
    top: PathBuf,
    /// A file in an existing subfolder: written in both modes.
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
            (workspace.join("notes/a.md"), "note"),
        ] {
            std::fs::create_dir_all(file.parent().expect("file has a parent"))?;
            std::fs::write(file, content)?;
        }
        std::fs::create_dir_all(workspace.join(".agents/skills"))?;
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
            top: workspace.join("top.txt"),
            inside: workspace.join("notes/inside.txt"),
            outside: root.join("outside/outside.txt"),
            home,
        })
    }

    /// The names `/run` may hold inside the sandbox: the first component of
    /// the resolver file bound back, when there is one.
    fn run_entries_allowed() -> Vec<String> {
        resolver_file(&runtime_folders())
            .and_then(|resolver| {
                resolver
                    .strip_prefix("/run")
                    .ok()
                    .and_then(|rest| rest.components().next())
                    .map(|first| first.as_os_str().to_string_lossy().into_owned())
            })
            .into_iter()
            .collect()
    }

    fn script(&self, port: u16) -> String {
        let q = |path: &Path| shell_quote(&path.to_string_lossy());
        let roots = &self.roots;
        let allowed_in_run: String = Self::run_entries_allowed()
            .iter()
            .map(|name| format!(" -e {}", shell_quote(name)))
            .collect();
        format!(
            "w() {{ ( : > \"$1\" ) 2>/dev/null && echo \"$2=ok\" || echo \"$2=fail\"; }}\n\
             r() {{ grep -qs \"$2\" \"$1\" 2>/dev/null && echo \"$3=ok\" || echo \"$3=fail\"; }}\n\
             w {top} write_top\n\
             w {ws}/.agents/skills/planted.md write_dot\n\
             w {inside} write_inside\n\
             w {ws}/{outputs}/out.txt write_outputs\n\
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
             [ -z \"$(ls -A /run 2>/dev/null | grep -vxF -e ''{allowed_in_run})\" ] && echo run_masked=ok || echo run_masked=fail\n\
             (exec 3<>/dev/tcp/127.0.0.1/{port}) 2>/dev/null && echo connect=ok || echo connect=fail\n\
             mv {ws}/.git {ws}/.git-moved 2>/dev/null && echo rename_git=ok || echo rename_git=fail\n\
             echo {end}\n",
            top = q(&self.top),
            inside = q(&self.inside),
            outputs = OUTPUTS_FOLDER,
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
            roots.workspace.join(".agents/skills"),
            roots.workspace.join("notes"),
            home.join(".ssh"),
        ] {
            std::fs::create_dir_all(path).unwrap();
        }
        std::fs::write(home.join(".netrc"), "x").unwrap();
        std::fs::write(roots.workspace.join(".git/config"), "x").unwrap();
        std::fs::write(roots.workspace.join("AGENTS.md"), "x").unwrap();
        std::os::unix::fs::symlink("notes", roots.workspace.join("linked")).unwrap();
        let sandbox = Sandbox::Bubblewrap("/usr/bin/bwrap".into());
        let command = sandbox.command(&roots, false);
        let argv = command.argv.join(" ");
        let at = |argv: &str, needle: &str| {
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
            assert!(
                at(&argv, "--tmpfs /run") < at(&argv, &format!("--ro-bind {resolver} {resolver}"))
            );
        }
        // Offline: the workspace is read-only and only its existing real
        // subfolders and `outputs`, created here, are bound back writable.
        assert!(roots.workspace.join("outputs").is_dir());
        assert!(
            at(&argv, &format!("--ro-bind {ws} {ws} --bind {ws}/notes {ws}/notes --bind {ws}/outputs {ws}/outputs --tmpfs {ws}/.loupist")) > 0
        );
        for unbound in [
            format!("--bind {ws} {ws}"),
            format!("{ws}/.git {ws}/.git"),
            format!("{ws}/.agents"),
            format!("{ws}/linked"),
            format!("{ws}/AGENTS.md"),
        ] {
            assert!(!argv.contains(&unbound), "{unbound}\n{argv}");
        }
        assert!(
            at(&argv, &format!("--tmpfs {app}"))
                < at(&argv, &format!("--bind {0} {0}", roots.scratch.display()))
        );
        assert!(
            at(&argv, &format!("--tmpfs {ws}/.loupist"))
                < at(&argv, &format!("--ro-bind {ws}/.loupist/skills"))
        );
        at(
            &argv,
            &format!("--ro-bind {0} {0}", roots.attachments.display()),
        );
        at(&argv, &format!("--tmpfs {}/.ssh", home.display()));
        at(
            &argv,
            &format!("--ro-bind /dev/null {}/.netrc", home.display()),
        );
        assert!(
            at(&argv, &format!("--bind {0} {0}", roots.scratch.display()))
                < at(&argv, &format!("--remount-ro {app}"))
        );
        at(&argv, &format!("--remount-ro {ws}/.loupist"));
        at(&argv, &format!("--remount-ro {}/.ssh", home.display()));
        assert!(argv.ends_with(&format!(
            "--unshare-net --unshare-pid --die-with-parent --new-session --chdir {ws} --"
        )));
        assert_eq!(
            command.env.get("XDG_CACHE_HOME").map(String::as_str),
            Some(roots.scratch.join("cache").to_str().unwrap())
        );
        assert!(roots.scratch.join("cache").is_dir());

        // Online: the whole workspace is writable, `.git` is pinned onto
        // itself and its config and hooks are read-only.
        let online = sandbox.command(&roots, true).argv.join(" ");
        assert!(!online.contains("--unshare-net"));
        assert!(!online.contains(&format!("--ro-bind {ws} {ws}")));
        assert!(
            at(&online, &format!("--bind {ws} {ws}"))
                < at(&online, &format!("--bind {ws}/.git {ws}/.git"))
        );
        assert!(
            at(&online, &format!("--bind {ws}/.git {ws}/.git"))
                < at(
                    &online,
                    &format!("--ro-bind {ws}/.git/config {ws}/.git/config")
                )
        );
        at(
            &online,
            &format!("--ro-bind {ws}/.git/hooks {ws}/.git/hooks"),
        );

        // A worktree's `.git` is a file naming the real git directory; online
        // it is bound read-only so the pointer cannot be redirected.
        std::fs::remove_dir_all(roots.workspace.join(".git")).unwrap();
        std::fs::write(roots.workspace.join(".git"), "gitdir: ../main/.git").unwrap();
        let online = sandbox.command(&roots, true).argv.join(" ");
        assert!(
            online.contains(&format!("--ro-bind {ws}/.git {ws}/.git")),
            "{online}"
        );
    }

    /// A workspace with more top-level subfolders than the cap keeps only
    /// `outputs` writable offline.
    #[test]
    fn beyond_the_cap_only_outputs_is_writable_offline() {
        let temporary = tempfile::tempdir().unwrap();
        let home = temporary.path().canonicalize().unwrap();
        let roots = roots(&home);
        for index in 0..=MAX_WRITABLE_CHILDREN {
            std::fs::create_dir_all(roots.workspace.join(format!("folder{index}"))).unwrap();
        }
        let argv = Sandbox::Bubblewrap("/usr/bin/bwrap".into())
            .command(&roots, false)
            .argv;
        let bound: Vec<&String> = argv
            .windows(3)
            .filter(|window| {
                window[0] == "--bind" && window[1].starts_with(roots.workspace.to_str().unwrap())
            })
            .map(|window| &window[1])
            .collect();
        assert_eq!(bound, [roots.workspace.join("outputs").to_str().unwrap()]);
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
    /// still works online when the host can resolve. Offline, the workspace's
    /// top level, its dot entries and the root instruction files take no
    /// write by any route, while existing subfolders and `outputs` do.
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
        std::fs::write(layout.roots.workspace.join("AGENTS.md"), "rules").unwrap();
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

        let script = format!(
            "t() {{ eval \"$2\" >/dev/null 2>&1 && echo \"$1=ok\" || echo \"$1=fail\"; }}\n\
             cd '{ws}'\n\
             t root_instruction 'echo planted > AGENTS.md'\n\
             t new_root_instruction 'echo planted > AGENTS.override.md'\n\
             t new_top_folder 'mkdir top'\n\
             t rename_child 'mv notes notes2'\n\
             t remove_child 'rmdir outputs'\n\
             t dot_skill 'echo planted > .agents/skills/evil.md'\n\
             t new_dot_folder 'mkdir .pi'\n\
             t git_commondir 'echo ../x > .git/commondir'\n\
             t git_rename 'mv .git .git-old'\n\
             t through_link 'ln -s ../AGENTS.md notes/l && echo planted > notes/l'\n\
             t hard_link 'ln AGENTS.md notes/h'\n\
             t subfolder_write 'mkdir notes/sub && echo work > notes/sub/f'\n\
             t outputs_write 'echo out > outputs/o'\n\
             t scratch_write 'echo s > \"$TMPDIR/s\"'\n\
             echo {end}\n",
            end = PROBE_END
        );
        let output = run_probe(&sandbox, &layout.roots, false, &script).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        for refused in [
            "root_instruction",
            "new_root_instruction",
            "new_top_folder",
            "rename_child",
            "remove_child",
            "dot_skill",
            "new_dot_folder",
            "git_commondir",
            "git_rename",
            "through_link",
            "hard_link",
        ] {
            assert!(
                lines.contains(&format!("{refused}=fail").as_str()),
                "{refused}\n{output}"
            );
        }
        for allowed in ["subfolder_write", "outputs_write", "scratch_write"] {
            assert!(
                lines.contains(&format!("{allowed}=ok").as_str()),
                "{allowed}\n{output}"
            );
        }
        let workspace = &layout.roots.workspace;
        assert_eq!(
            std::fs::read_to_string(workspace.join("AGENTS.md")).unwrap(),
            "rules"
        );
        assert!(!workspace.join("AGENTS.override.md").exists());
        assert!(!workspace.join(".agents/skills/evil.md").exists());
        assert!(!workspace.join(".git/commondir").exists());
        assert_eq!(
            std::fs::read_to_string(workspace.join("notes/sub/f")).unwrap(),
            "work\n"
        );
        assert_eq!(
            std::fs::read_to_string(workspace.join("outputs/o")).unwrap(),
            "out\n"
        );
    }

    /// Credential entries that are absolute links (`~/.bashrc ->
    /// ~/dotfiles/bashrc`, as dotbot, rcm and home-manager lay a home out)
    /// and a workspace `.git` that is a link do not stop the command: each
    /// mask lands on the link's target, a dangling link masks nothing, and
    /// the targets read as absent or empty through the link and directly.
    #[cfg(target_os = "linux")]
    #[test]
    fn absolute_links_at_credential_entries_and_the_git_directory_still_run_the_command() {
        use std::os::unix::fs::symlink;
        let Some(sandbox) = detect() else {
            eprintln!("skipping: bubblewrap (bwrap) is not installed on this host");
            return;
        };
        let temporary = probe_folder().unwrap();
        let layout = ProbeLayout::create(temporary.path()).unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let home = &layout.home;
        let dotfiles = root.join("dotfiles");
        std::fs::create_dir_all(dotfiles.join("config/gh")).unwrap();
        std::fs::create_dir_all(dotfiles.join("config/other")).unwrap();
        std::fs::rename(home.join(".ssh"), dotfiles.join("ssh")).unwrap();
        std::fs::write(dotfiles.join("bashrc"), "export TOKEN=secret").unwrap();
        std::fs::write(dotfiles.join("config/gh/hosts.yml"), "ghsecret").unwrap();
        std::fs::write(dotfiles.join("config/other/settings"), "otherok").unwrap();
        symlink(dotfiles.join("bashrc"), home.join(".bashrc")).unwrap();
        symlink(dotfiles.join("ssh"), home.join(".ssh")).unwrap();
        symlink(dotfiles.join("config"), home.join(".config")).unwrap();
        symlink(root.join("missing"), home.join(".npmrc")).unwrap();
        let workspace = &layout.roots.workspace;
        let repository = root.join("repo.git");
        std::fs::rename(workspace.join(".git"), &repository).unwrap();
        symlink(&repository, workspace.join(".git")).unwrap();
        let script = format!(
            "e() {{ [ -e \"$1\" ] && [ ! -s \"$1\" ] && echo \"$2=empty\" || echo \"$2=other\"; }}\n\
             a() {{ [ -e \"$1\" ] && echo \"$2=present\" || echo \"$2=absent\"; }}\n\
             e '{home}/.bashrc' bashrc_link\n\
             e '{dotfiles}/bashrc' bashrc_target\n\
             a '{home}/.ssh/id' ssh_link\n\
             a '{dotfiles}/ssh/id' ssh_target\n\
             a '{home}/.config/gh/hosts.yml' gh_link\n\
             a '{dotfiles}/config/gh/hosts.yml' gh_target\n\
             grep -qs otherok '{home}/.config/other/settings' && echo other=ok || echo other=fail\n\
             grep -qs hookok '{ws}/.git/hooks/pre-commit' && echo hook=ok || echo hook=fail\n\
             echo {end}\n",
            home = home.display(),
            dotfiles = dotfiles.display(),
            ws = workspace.display(),
            end = PROBE_END
        );
        for network in [true, false] {
            let output = run_probe(&sandbox, &layout.roots, network, &script)
                .unwrap_or_else(|error| panic!("network={network}: {error:?}"));
            let lines: Vec<&str> = output.lines().collect();
            for line in [
                "bashrc_link=empty",
                "bashrc_target=empty",
                "ssh_link=absent",
                "ssh_target=absent",
                "gh_link=absent",
                "gh_target=absent",
                "other=ok",
                "hook=ok",
            ] {
                assert!(lines.contains(&line), "network={network}: {line}\n{output}");
            }
        }
        assert_eq!(
            std::fs::read_to_string(dotfiles.join("bashrc")).unwrap(),
            "export TOKEN=secret"
        );
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

    /// A real bubblewrap that lost the `/run` tmpfs fails the probe: the
    /// host's runtime sockets would be reachable.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_sandbox_without_the_runtime_socket_mask_fails_the_probe() {
        let Some(Sandbox::Bubblewrap(bwrap)) = detect() else {
            eprintln!("skipping: bubblewrap (bwrap) is not installed on this host");
            return;
        };
        let allowed = ProbeLayout::run_entries_allowed();
        let host_has_other_entries = std::fs::read_dir("/run").is_ok_and(|entries| {
            entries
                .flatten()
                .any(|entry| !allowed.iter().any(|name| **name == *entry.file_name()))
        });
        if !host_has_other_entries {
            eprintln!("skipping: this host's /run holds nothing the sandbox would hide");
            return;
        }
        let temporary = tempfile::tempdir().unwrap();
        let wrapper = temporary.path().join("bwrap-without-run-mask");
        std::fs::write(
            &wrapper,
            format!(
                "#!/bin/bash\n\
                 out=()\n\
                 while [ $# -gt 0 ]; do\n\
                 \tif [ \"$1\" = --tmpfs ]; then case \"$2\" in /run|/var/run) shift 2; continue ;; esac; fi\n\
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
        assert_eq!(error, ProbeError::Failed(vec!["run_masked=fail".into()]));
    }

    /// Each probe check fails on its own when the sandbox is missing, so a
    /// passing probe says something about every rule.
    #[cfg(target_os = "linux")]
    #[test]
    fn without_a_sandbox_the_probe_reports_every_broken_rule() {
        let temporary = tempfile::tempdir().unwrap();
        let layout = ProbeLayout::create(temporary.path()).unwrap();
        std::fs::create_dir(layout.roots.workspace.join(OUTPUTS_FOLDER)).unwrap();
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
                "write_top=ok",
                "write_dot=ok",
                "write_outside=ok",
                "write_data=ok",
                "write_hook=ok",
                "read_app=ok",
                "read_netrc=ok",
                "read_ssh=ok",
                "read_db=ok",
                "run_masked=fail",
                "connect=ok",
                "rename_git=ok"
            ],
            "{output}"
        );
    }
}
