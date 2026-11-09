//! PATH for native file search (`rg`). A GUI-launched app inherits the launchd
//! PATH, so executables the user's shell startup files add (Homebrew) are not
//! found. Resolve the login-shell PATH once per app run with a short,
//! interactive login probe without a terminal, after the bundled workspace tools.
//! Any failure keeps the inherited PATH. The probe reads `$PATH` from the user's
//! login shell; that shell name is not a Loupist tool.
#![cfg_attr(not(unix), allow(dead_code))]
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

const BEGIN: &str = "__LOUPIST_LOGIN_PATH_BEGIN__";
const END: &str = "__LOUPIST_LOGIN_PATH_END__";
#[cfg(unix)]
const PROBE: &str =
    "printf '%s%s%s' __LOUPIST_LOGIN_PATH_BEGIN__ \"$PATH\" __LOUPIST_LOGIN_PATH_END__";
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const MAX_PATH_BYTES: usize = 16 * 1024;
const MAX_PATH_ENTRIES: usize = 128;
/// Shells known to accept `-l -i -c <command>` and expand `"$PATH"` as one
/// colon-joined string. csh/tcsh accept `-l` only as their sole flag.
const SUPPORTED_SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "mksh", "fish"];
const FALLBACK_PROBE_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

static LOGIN_PATH: OnceLock<Option<Vec<PathBuf>>> = OnceLock::new();

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ShellPathError {
    UnsupportedShell,
    Launch,
    Timeout,
    OutputTooLarge,
    MissingMarkers,
    InvalidPath,
}

/// Resolve in the background at startup so the first Chat does not wait.
pub(crate) fn prewarm() {
    let _ = std::thread::Builder::new()
        .name("login-shell-path".into())
        .spawn(|| {
            let _ = login_shell_path();
        });
}

/// Cached login-shell PATH entries, or `None` when unavailable on this host.
/// Blocks while another thread resolves; async callers use
/// [`login_shell_path_for_launch`].
pub(crate) fn login_shell_path() -> Option<&'static [PathBuf]> {
    LOGIN_PATH.get_or_init(resolve_for_current_user).as_deref()
}

/// The login PATH for a Pi launch. The first launch can arrive while the
/// startup prewarm still runs the probe; waiting for it happens on the
/// blocking pool, never on an async worker, and gives up after the probe's own
/// deadline so the launch keeps the inherited PATH. The probe never joins its
/// output reader, so a profile's background job cannot hold this forever.
pub(crate) async fn login_shell_path_for_launch() -> Option<&'static [PathBuf]> {
    cached_off_runtime(
        &LOGIN_PATH,
        resolve_for_current_user,
        RESOLVE_TIMEOUT + Duration::from_secs(1),
    )
    .await?
    .as_deref()
}

/// A once-per-app-run value resolved on the blocking pool, or None when the
/// resolution outlasts `wait` (it keeps running for the next caller).
pub(crate) async fn cached_off_runtime<T: Sync + Send + 'static>(
    cell: &'static OnceLock<T>,
    resolve: fn() -> T,
    wait: Duration,
) -> Option<&'static T> {
    if let Some(cached) = cell.get() {
        return Some(cached);
    }
    let lookup = tokio::task::spawn_blocking(move || cell.get_or_init(resolve));
    tokio::time::timeout(wait, lookup).await.ok()?.ok()
}

// Unit tests never run the developer's shell; the probe has its own tests
// with stand-in shells below.
#[cfg(all(unix, not(test)))]
fn resolve_for_current_user() -> Option<Vec<PathBuf>> {
    let shell = std::env::var_os("SHELL").map(PathBuf::from)?;
    let environment = minimal_environment(|name| std::env::var_os(name));
    resolve(&shell, &environment, RESOLVE_TIMEOUT).ok()
}

#[cfg(any(not(unix), test))]
fn resolve_for_current_user() -> Option<Vec<PathBuf>> {
    let _ = RESOLVE_TIMEOUT;
    None
}

/// PATH for the Pi process and its native file-search children: bundled tools first, then
/// the login-shell entries, then inherited entries the login PATH lacks.
/// Without a login PATH the inherited value is kept exactly.
pub(crate) fn bash_path(
    tools: &Path,
    login: Option<&[PathBuf]>,
    inherited: Option<&str>,
) -> Option<String> {
    let mut paths = vec![tools.to_path_buf()];
    let inherited = inherited.map(|value| std::env::split_paths(value).collect::<Vec<_>>());
    match login {
        Some(login) => {
            for entry in login.iter().chain(inherited.iter().flatten()) {
                if !paths.contains(entry) {
                    paths.push(entry.clone());
                }
            }
        }
        None => paths.extend(inherited.into_iter().flatten()),
    }
    std::env::join_paths(paths)
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
}

/// Only what a login profile needs to compute PATH; never app credentials.
pub(crate) fn minimal_environment(
    lookup: impl Fn(&str) -> Option<OsString>,
) -> Vec<(OsString, OsString)> {
    let mut environment = ["HOME", "USER", "LOGNAME", "SHELL", "TMPDIR", "LANG"]
        .into_iter()
        .filter_map(|name| lookup(name).map(|value| (OsString::from(name), value)))
        .collect::<Vec<_>>();
    environment.push((
        "PATH".into(),
        lookup("PATH").unwrap_or_else(|| FALLBACK_PROBE_PATH.into()),
    ));
    environment.push(("TERM".into(), "dumb".into()));
    environment
}

/// Extract and validate the PATH printed between the probe markers. Profile
/// output before or after the markers is ignored.
pub(crate) fn parse_probe_output(output: &[u8]) -> Result<Vec<PathBuf>, ShellPathError> {
    let start = output
        .windows(BEGIN.len())
        .position(|part| part == BEGIN.as_bytes())
        .map(|index| index + BEGIN.len())
        .ok_or(ShellPathError::MissingMarkers)?;
    let length = output[start..]
        .windows(END.len())
        .position(|part| part == END.as_bytes())
        .ok_or(ShellPathError::MissingMarkers)?;
    let value = std::str::from_utf8(&output[start..start + length])
        .map_err(|_| ShellPathError::InvalidPath)?;
    if value.is_empty() || value.len() > MAX_PATH_BYTES || value.chars().any(char::is_control) {
        return Err(ShellPathError::InvalidPath);
    }
    let mut entries: Vec<PathBuf> = Vec::new();
    // Empty and relative entries resolve against the command's directory.
    for entry in value.split(':').filter(|entry| entry.starts_with('/')) {
        let entry = PathBuf::from(entry);
        if !entries.contains(&entry) {
            entries.push(entry);
        }
        if entries.len() == MAX_PATH_ENTRIES {
            break;
        }
    }
    if entries.is_empty() {
        return Err(ShellPathError::InvalidPath);
    }
    Ok(entries)
}

fn supported_shell(shell: &Path) -> bool {
    shell.is_absolute()
        && shell
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| SUPPORTED_SHELLS.contains(&name))
        && shell.is_file()
}

/// Run `<shell> -l -i -c <probe>` with only `environment`, no stdin or
/// controlling terminal and a bounded output budget. The whole process group
/// is killed at the deadline.
#[cfg(unix)]
pub(crate) fn resolve(
    shell: &Path,
    environment: &[(OsString, OsString)],
    deadline: Duration,
) -> Result<Vec<PathBuf>, ShellPathError> {
    use std::{
        io::Read,
        os::unix::process::CommandExt,
        process::{Command, Stdio},
        sync::mpsc,
    };

    if !supported_shell(shell) {
        return Err(ShellPathError::UnsupportedShell);
    }
    let home = environment
        .iter()
        .find(|(name, _)| name == "HOME")
        .map(|(_, value)| PathBuf::from(value))
        .filter(|home| home.is_dir())
        .unwrap_or_else(|| PathBuf::from("/"));
    let mut command = Command::new(shell);
    command
        .args(["-l", "-i", "-c", PROBE])
        .env_clear()
        .envs(environment.iter().map(|(name, value)| (name, value)))
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe and accesses no Rust state. A new
    // session also prevents interactive startup hooks from opening /dev/tty.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|_| ShellPathError::Launch)?;
    let group = i32::try_from(child.id()).map_err(|_| ShellPathError::Launch)?;
    let mut stdout = child.stdout.take().ok_or(ShellPathError::Launch)?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        // Stop at the closing marker: a background process started by the
        // profile can keep the pipe open after the shell exits.
        let mut output = Vec::new();
        let mut chunk = [0_u8; 4096];
        let result = loop {
            match stdout.read(&mut chunk) {
                Ok(0) => break Ok(output),
                Ok(read) => {
                    output.extend_from_slice(&chunk[..read]);
                    if output.len() > MAX_OUTPUT_BYTES {
                        break Err(ShellPathError::OutputTooLarge);
                    }
                    if output
                        .windows(END.len())
                        .any(|window| window == END.as_bytes())
                    {
                        break Ok(output);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break Err(ShellPathError::Launch),
            }
        };
        let _ = sender.send(result);
    });
    let output = receiver
        .recv_timeout(deadline)
        .unwrap_or(Err(ShellPathError::Timeout));
    // The probe is finished or abandoned. Signal before reaping: the unreaped
    // leader keeps the group id reserved, so this cannot reach a reused group,
    // and it also stops anything the profile left running in the group.
    let _ = crate::process_lease::signal_process_group(group, libc::SIGKILL);
    let _ = child.wait();
    parse_probe_output(&output?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_output_keeps_absolute_entries_once_and_ignores_profile_noise() {
        let output = format!(
            "Welcome back\n{BEGIN}/opt/homebrew/bin::bin:./scripts:/usr/bin:/opt/homebrew/bin:/bin{END}\nbye"
        );
        assert_eq!(
            parse_probe_output(output.as_bytes()).unwrap(),
            ["/opt/homebrew/bin", "/usr/bin", "/bin"].map(PathBuf::from)
        );
        let noisy = [b"\xff\x1b[31m".as_slice(), output.as_bytes(), b"\xfe"].concat();
        assert_eq!(
            parse_probe_output(&noisy),
            parse_probe_output(output.as_bytes())
        );
    }

    #[test]
    fn probe_output_without_a_valid_path_is_rejected() {
        for (output, expected) in [
            ("/usr/bin".to_owned(), ShellPathError::MissingMarkers),
            (format!("{BEGIN}/usr/bin"), ShellPathError::MissingMarkers),
            (format!("{BEGIN}{END}"), ShellPathError::InvalidPath),
            (format!("{BEGIN}bin:.{END}"), ShellPathError::InvalidPath),
            (
                format!("{BEGIN}/usr/bin\n/bin{END}"),
                ShellPathError::InvalidPath,
            ),
            (
                format!("{BEGIN}/{}{END}", "a".repeat(MAX_PATH_BYTES)),
                ShellPathError::InvalidPath,
            ),
        ] {
            assert_eq!(
                parse_probe_output(output.as_bytes()),
                Err(expected),
                "{output:.40}"
            );
        }
        assert_eq!(
            parse_probe_output(&[BEGIN.as_bytes(), b"/\xff", END.as_bytes()].concat()),
            Err(ShellPathError::InvalidPath)
        );
        let many = (0..MAX_PATH_ENTRIES + 10)
            .map(|index| format!("/p{index}"))
            .collect::<Vec<_>>()
            .join(":");
        let parsed = parse_probe_output(format!("{BEGIN}{many}{END}").as_bytes()).unwrap();
        assert_eq!(parsed.len(), MAX_PATH_ENTRIES);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_launch_waits_for_the_prewarm_without_blocking_the_async_runtime() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        static CELL: OnceLock<Option<Vec<PathBuf>>> = OnceLock::new();
        static STARTED: AtomicBool = AtomicBool::new(false);
        fn slow_probe() -> Option<Vec<PathBuf>> {
            STARTED.store(true, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(400));
            Some(vec![PathBuf::from("/opt/login/bin")])
        }
        // The startup prewarm is mid-probe when the first Pi launches.
        let prewarm = std::thread::spawn(|| CELL.get_or_init(slow_probe).is_some());
        while !STARTED.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let ticks = std::sync::Arc::new(AtomicUsize::new(0));
        let ticker = tokio::spawn({
            let ticks = ticks.clone();
            async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    ticks.fetch_add(1, Ordering::SeqCst);
                }
            }
        });
        let path = cached_off_runtime(&CELL, slow_probe, Duration::from_secs(5)).await;
        assert_eq!(
            path.and_then(|path| path.as_deref()),
            Some(&[PathBuf::from("/opt/login/bin")][..])
        );
        // Other tasks on this single-threaded runtime kept running meanwhile.
        assert!(
            ticks.load(Ordering::SeqCst) >= 5,
            "the async runtime was blocked"
        );
        ticker.abort();
        assert!(prewarm.join().unwrap());

        // A probe that outlives the wait leaves the launch with the inherited PATH.
        static STUCK: OnceLock<Option<Vec<PathBuf>>> = OnceLock::new();
        fn stuck_probe() -> Option<Vec<PathBuf>> {
            std::thread::sleep(Duration::from_secs(2));
            None
        }
        let started = std::time::Instant::now();
        assert_eq!(
            cached_off_runtime(&STUCK, stuck_probe, Duration::from_millis(100)).await,
            None
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn bash_path_puts_tools_first_then_login_then_unseen_inherited_entries() {
        let tools = Path::new("/app/workspace-tools");
        let login = ["/opt/homebrew/bin", "/usr/bin", "/bin"].map(PathBuf::from);
        let merged = bash_path(tools, Some(&login), Some("/usr/bin:/bin:/usr/sbin")).unwrap();
        assert_eq!(
            merged,
            "/app/workspace-tools:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin"
        );
    }

    #[test]
    fn bash_path_without_a_login_path_keeps_the_inherited_value() {
        let tools = Path::new("/app/workspace-tools");
        assert_eq!(
            bash_path(tools, None, Some("/usr/bin:/bin:/usr/bin")).unwrap(),
            "/app/workspace-tools:/usr/bin:/bin:/usr/bin"
        );
        assert_eq!(
            bash_path(tools, None, None).unwrap(),
            "/app/workspace-tools"
        );
    }

    #[test]
    fn probe_environment_is_minimal_and_has_a_path() {
        let environment = minimal_environment(|name| match name {
            "HOME" => Some("/Users/a".into()),
            "OPENAI_API_KEY" | "LOUPIST_BROKER_TOKEN" => Some("secret".into()),
            _ => None,
        });
        assert_eq!(
            environment,
            [
                ("HOME", "/Users/a"),
                ("PATH", FALLBACK_PROBE_PATH),
                ("TERM", "dumb")
            ]
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        );
    }

    #[cfg(unix)]
    fn stand_in_shell(directory: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = directory.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn resolve_runs_an_interactive_login_shell_with_only_the_given_environment() {
        let directory = tempfile::tempdir().unwrap();
        let log = directory.path().join("seen");
        let shell = stand_in_shell(
            directory.path(),
            "zsh",
            &format!(
                "printf '%s|%s|%s|%s\\n' \"$1\" \"$2\" \"$3\" \"$#\" > '{log}'\nenv >> '{log}'\necho 'profile says hi'\nPATH=/opt/login/bin:$PATH\nexport PATH\nexec /bin/sh -c \"$4\"",
                log = log.display()
            ),
        );
        let home = directory.path().to_string_lossy().into_owned();
        let environment = minimal_environment(|name| match name {
            "HOME" => Some(home.clone().into()),
            "PATH" => Some("/usr/bin:/bin".into()),
            _ => None,
        });
        let entries = resolve(&shell, &environment, Duration::from_secs(10)).unwrap();
        assert_eq!(
            entries,
            ["/opt/login/bin", "/usr/bin", "/bin"].map(PathBuf::from)
        );
        let seen = std::fs::read_to_string(&log).unwrap();
        let mut lines = seen.lines();
        assert_eq!(lines.next(), Some("-l|-i|-c|4"));
        assert!(seen.lines().any(|line| line == format!("HOME={home}")));
        // Nothing from the test process leaks in; the stand-in's own /bin/sh
        // adds only its bookkeeping variables.
        for line in lines {
            let name = line.split('=').next().unwrap_or_default();
            assert!(
                [
                    "HOME",
                    "PATH",
                    "TERM",
                    "PWD",
                    "OLDPWD",
                    "SHLVL",
                    "_",
                    "__CF_USER_TEXT_ENCODING"
                ]
                .contains(&name),
                "unexpected variable {name}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn resolve_reads_zshrc_without_a_terminal_or_stdin() {
        let shell = Path::new("/bin/zsh");
        if !shell.is_file() {
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join(".zshrc"),
            "[[ -o interactive && -o login ]] || exit 2\n\
             [[ ! -t 0 && ! -t 1 && ! -t 2 ]] || exit 3\n\
             if ( : </dev/tty ) 2>/dev/null; then exit 4; fi\n\
             read -r answer && exit 5\n\
             print 'Welcome to the interactive shell'\n\
             export PATH=/opt/only-in-zshrc/bin:/usr/bin:/bin\n",
        )
        .unwrap();
        let environment = minimal_environment(|name| match name {
            "HOME" => Some(directory.path().as_os_str().to_owned()),
            "PATH" => Some("/usr/bin:/bin".into()),
            _ => None,
        });
        let entries = resolve(shell, &environment, Duration::from_secs(10)).unwrap();
        assert_eq!(
            entries,
            ["/opt/only-in-zshrc/bin", "/usr/bin", "/bin"].map(PathBuf::from)
        );
        let merged = bash_path(Path::new("/app/tools"), Some(&entries), Some("/bin"));
        assert_eq!(
            merged.as_deref(),
            Some("/app/tools:/opt/only-in-zshrc/bin:/usr/bin:/bin")
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_gives_up_on_slow_noisy_or_unsupported_shells() {
        let directory = tempfile::tempdir().unwrap();
        let environment = minimal_environment(|_| None);
        let slow = stand_in_shell(directory.path(), "bash", "sleep 30");
        let started = std::time::Instant::now();
        assert_eq!(
            resolve(&slow, &environment, Duration::from_millis(300)),
            Err(ShellPathError::Timeout)
        );
        assert!(started.elapsed() < Duration::from_secs(5));

        let noisy_directory = tempfile::tempdir().unwrap();
        let noisy = stand_in_shell(
            noisy_directory.path(),
            "bash",
            "yes loupist | head -c 200000",
        );
        assert_eq!(
            resolve(&noisy, &environment, Duration::from_secs(10)),
            Err(ShellPathError::OutputTooLarge)
        );

        // A profile that leaves a process holding stdout neither blocks the
        // probe nor outlives it.
        let holder_directory = tempfile::tempdir().unwrap();
        let group_file = holder_directory.path().join("holder.group");
        let holder = stand_in_shell(
            holder_directory.path(),
            "dash",
            &format!(
                "sleep 30 &\necho $$ > '{}'\nexec /bin/sh -c \"$4\"",
                group_file.display()
            ),
        );
        let started = std::time::Instant::now();
        assert!(resolve(&holder, &environment, Duration::from_secs(10)).is_ok());
        assert!(started.elapsed() < Duration::from_secs(5));
        let holder_group: i32 = std::fs::read_to_string(&group_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let stopped = (0..200).any(|_| {
            // Container PID 1 may leave an exited orphan unreaped. Its PID
            // still exists, but only a live group member can keep running or
            // hold the probe's stdout open.
            let stopped = crate::process_lease::process_group_has_exited(holder_group).unwrap();
            if !stopped {
                std::thread::sleep(Duration::from_millis(10));
            }
            stopped
        });
        assert!(stopped, "the profile's background process survived");

        let silent_directory = tempfile::tempdir().unwrap();
        let silent = stand_in_shell(silent_directory.path(), "sh", "exit 0");
        assert_eq!(
            resolve(&silent, &environment, Duration::from_secs(10)),
            Err(ShellPathError::MissingMarkers)
        );

        let csh = stand_in_shell(directory.path(), "tcsh", "exit 0");
        assert_eq!(
            resolve(&csh, &environment, Duration::from_secs(1)),
            Err(ShellPathError::UnsupportedShell)
        );
        assert_eq!(
            resolve(Path::new("zsh"), &environment, Duration::from_secs(1)),
            Err(ShellPathError::UnsupportedShell)
        );
    }
}
