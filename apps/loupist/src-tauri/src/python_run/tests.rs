use std::{
    fs,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use tempfile::TempDir;

use crate::{broker::BrokerError, web::SearchCancel};

use super::snapshot::Prepared;

const TOKEN: &str = "APPTOKEN-9f3c";
const README: &str = "readme-original";

struct Fixture {
    _tmp: TempDir,
    roots: crate::agent_harness::native_policy::NativeRoots,
    outside: PathBuf,
}

fn sample() -> Fixture {
    let tmp = TempDir::new().expect("temp dir");
    let workspace = tmp.path().join("loupist");
    let app = tmp.path().join("app");
    let scratch = app.join("scratch");
    let attachments = app.join("attachments");
    let home = tmp.path().join("home");
    let outside = tmp.path().join("outside");
    for path in [
        workspace.join("notes"),
        workspace.join(".hidden"),
        workspace.join(".git"),
        workspace.join(".loupist"),
        scratch.clone(),
        attachments.clone(),
        home.clone(),
        outside.clone(),
    ] {
        fs::create_dir_all(&path).unwrap();
    }
    fs::write(workspace.join("README.md"), README).unwrap();
    fs::write(workspace.join("AGENTS.md"), "instructions").unwrap();
    fs::write(workspace.join("notes/keep.txt"), "keep").unwrap();
    fs::write(workspace.join("notes/linked.txt"), "linked").unwrap();
    fs::write(workspace.join(".hidden/token.txt"), TOKEN).unwrap();
    fs::write(workspace.join(".git/config"), "git").unwrap();
    fs::write(app.join("secret.txt"), TOKEN).unwrap();
    fs::write(attachments.join("clip.txt"), "clip").unwrap();
    fs::write(scratch.join("pad.txt"), "pad").unwrap();
    fs::write(outside.join("marker"), "OUTSIDE").unwrap();
    fs::hard_link(
        workspace.join("notes/linked.txt"),
        workspace.join("notes/dup.txt"),
    )
    .unwrap();
    std::os::unix::fs::symlink(outside.join("marker"), workspace.join("notes/escape")).unwrap();
    let roots = crate::agent_harness::native_policy::NativeRoots::new(
        &workspace,
        &app,
        &app,
        &workspace.join(".loupist"),
        &scratch,
        &attachments,
        Some(&home),
    );
    Fixture {
        _tmp: tmp,
        roots,
        outside,
    }
}

fn workspace(fixture: &Fixture) -> PathBuf {
    fixture.roots.workspace.clone()
}

fn prepare(fixture: &Fixture) -> Prepared {
    super::snapshot::prepare(&fixture.roots).expect("snapshot")
}

fn mode_bits(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn snap_file(prepared: &Prepared, relative: &str) -> PathBuf {
    prepared.snapshot_dir().join("workspace").join(relative)
}

fn host_text(fixture: &Fixture, relative: &str) -> String {
    fs::read_to_string(workspace(fixture).join(relative)).unwrap()
}

fn absent(path: &Path) -> bool {
    fs::symlink_metadata(path).is_err()
}

fn conflicts(report: &super::snapshot::WriteReport) -> Vec<(String, String)> {
    report
        .conflicts
        .iter()
        .map(|conflict| (conflict.path.clone(), conflict.reason.to_owned()))
        .collect()
}

fn lock_worker() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn shared_cache() -> &'static Path {
    static CACHE: OnceLock<TempDir> = OnceLock::new();
    CACHE
        .get_or_init(|| TempDir::new().expect("python cache"))
        .path()
}

fn require_runtime() {
    assert!(
        super::runtime_dir().is_some(),
        "staged Python runtime is missing"
    );
    assert!(
        super::worker_path().is_some(),
        "Python worker binary is missing"
    );
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

async fn warm() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let fixture = sample();
    let value = super::execute(
        &fixture.roots,
        shared_cache(),
        "print('warm')",
        120,
        SearchCancel::never(),
    )
    .await
    .expect("warmup");
    assert_eq!(value["status"], "completed", "{value}");
    assert!(
        value["stdout"].as_str().unwrap_or("").contains("warm"),
        "{value}"
    );
}

const CHART_CODE: &str = r#"
import numpy as np
import pandas as pd
import scipy.stats
import statsmodels.api as sm
from sklearn.linear_model import LinearRegression
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
x = np.array([1.0, 2.0, 3.0, 4.0])
y = 2 * x + 1
slope = float(scipy.stats.linregress(x, y).slope)
frame = pd.DataFrame({"x": x, "y": y})
fitted = sm.OLS(frame["y"], sm.add_constant(frame["x"])).fit()
learned = float(LinearRegression().fit(x.reshape(-1, 1), y).coef_[0])
print(f"slope={slope:.1f}")
print(f"ols={float(fitted.params.iloc[1]):.1f}")
print(f"sklearn={learned:.1f}")
plt.plot(x, y)
plt.savefig("/workspace/outputs/chart.png")
open("/workspace/notes/result.txt", "w").write(f"{slope:.1f}\n")
"#;

fn run_python(fixture: &Fixture, code: &str, timeout: u64) -> Value {
    runtime().block_on(async {
        warm().await;
        super::execute(
            &fixture.roots,
            shared_cache(),
            code,
            timeout,
            SearchCancel::never(),
        )
        .await
        .expect("python.run")
    })
}

#[test]
fn parse_params_accepts_raw_code_and_a_bounded_timeout() {
    let (code, timeout) = super::parse_params(&json!({"code": "  print(1)  "})).unwrap();
    assert_eq!(code, "  print(1)  ");
    assert_eq!(timeout, 60);
    let (code, timeout) =
        super::parse_params(&json!({"code": "x", "timeout_seconds": 120})).unwrap();
    assert_eq!(code, "x");
    assert_eq!(timeout, 120);
    let big = "a".repeat(64 * 1024);
    assert!(super::parse_params(&json!({"code": big})).is_ok());

    let invalid = |value: Value, field: &str| match super::parse_params(&value) {
        Err(BrokerError::InvalidArguments { field: got, .. }) => assert_eq!(got, field),
        other => panic!("expected invalid {field}, got {other:?}"),
    };
    invalid(json!({"code": ""}), "code");
    invalid(json!({"code": format!("{big}\n")}), "code");
    invalid(json!({"code": "a\0b"}), "code");
    invalid(json!({"code": 1}), "code");
    invalid(json!({}), "code");
    invalid(
        json!({"code": "x", "timeout_seconds": 0}),
        "timeout_seconds",
    );
    invalid(
        json!({"code": "x", "timeout_seconds": 121}),
        "timeout_seconds",
    );
    invalid(
        json!({"code": "x", "timeout_seconds": 1.5}),
        "timeout_seconds",
    );
    invalid(
        json!({"code": "x", "timeout_seconds": "60"}),
        "timeout_seconds",
    );
    assert!(matches!(
        super::parse_params(&json!([])),
        Err(BrokerError::Malformed)
    ));
    assert!(matches!(
        super::parse_params(&json!({"code": "x", "command": "ls"})),
        Err(BrokerError::Malformed)
    ));
}

#[test]
fn the_desktop_crate_does_not_link_wasmtime() {
    let cargo = include_str!("../../Cargo.toml");
    assert!(!cargo.to_ascii_lowercase().contains("wasmtime"));
}

#[test]
fn results_replace_host_paths_and_keep_the_guest_text() {
    let text = super::redact(
        b"wrote /Users/secret/workspace/notes and said ok",
        &["/Users/secret/workspace".into()],
    );
    assert_eq!(text, "wrote [path]/notes and said ok");
    assert!(!text.contains("/Users/secret"));
    let paths = ["/private/app".into(), "/private/app/scratch".into()];
    for secrets in [paths.to_vec(), paths.into_iter().rev().collect()] {
        assert_eq!(
            super::redact(b"/private/app/scratch/notes /private/app", &secrets),
            "[path]/notes [path]"
        );
    }
}

#[test]
fn results_report_both_snapshot_omissions_and_writeback_truncation() {
    let fixture = sample();
    let prepared = prepare(&fixture);
    let mut report = prepared.writeback().unwrap();
    report.truncated = true;
    let outcome = super::WorkerOutcome {
        status: "completed",
        exit_code: 0,
        stdout: Vec::new(),
        stderr: b"guest warning".to_vec(),
        stdout_truncated: false,
        stderr_truncated: false,
    };
    let result = super::result_value(
        &outcome,
        &report,
        2,
        &["notes/left-out.csv".into()],
        false,
        &[],
    );
    assert_eq!(result["stderr"], "guest warning\nSome workspace changes were not copied back.\nSome files or folders were left out of the snapshot because of its limits.\n");
}

#[test]
fn snapshot_reports_an_outputs_file_without_changing_it() {
    let fixture = sample();
    fs::write(workspace(&fixture).join("outputs"), "keep").unwrap();
    let error = super::snapshot::prepare(&fixture.roots).unwrap_err();
    assert_eq!(
        error.message,
        "workspace outputs must be a directory; rename the outputs file before running Python"
    );
    assert_eq!(host_text(&fixture, "outputs"), "keep");
}

#[tokio::test]
async fn a_symlink_worker_is_rejected_before_it_can_run() {
    let dir = TempDir::new().unwrap();
    let worker = dir.path().join("worker");
    std::os::unix::fs::symlink("/bin/cat", &worker).unwrap();
    let result = super::supervise_worker(
        &worker,
        dir.path(),
        b"{}",
        1,
        SearchCancel::never(),
        SearchCancel::never(),
    )
    .await;
    assert!(
        matches!(result, Err(BrokerError::Execution(message)) if message == "Python runtime is untrusted")
    );
}

#[test]
fn writeback_preserves_existing_file_permissions() {
    use std::os::unix::fs::PermissionsExt;
    for mode in [0o600, 0o755] {
        let fixture = sample();
        let target = workspace(&fixture).join("notes/keep.txt");
        fs::set_permissions(&target, fs::Permissions::from_mode(mode)).unwrap();
        let prepared = prepare(&fixture);
        fs::write(snap_file(&prepared, "notes/keep.txt"), "changed").unwrap();
        let report = prepared.writeback().unwrap();
        assert!(report.changed.contains(&"notes/keep.txt".to_owned()));
        assert_eq!(host_text(&fixture, "notes/keep.txt"), "changed");
        assert_eq!(mode_bits(&target), mode);
    }
}

#[test]
fn failed_writeback_removes_partial_new_and_replacement_files() {
    const CHILD: &str = "LOUPIST_TEST_WRITEBACK_FILE_LIMIT";
    if std::env::var_os(CHILD).is_none() {
        for kind in ["replacement", "new"] {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "python_run::tests::failed_writeback_removes_partial_new_and_replacement_files",
                    "--nocapture",
                ])
                .env(CHILD, kind)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    }
    let fixture = sample();
    let prepared = prepare(&fixture);
    let target = if std::env::var(CHILD).as_deref() == Ok("new") {
        "notes/new.txt"
    } else {
        "notes/keep.txt"
    };
    fs::write(snap_file(&prepared, target), "changed").unwrap();
    // Limit only this test subprocess, after its fixture is ready. A real
    // partial file write now fails without filling the shared host's disk.
    unsafe {
        libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
        let limit = libc::rlimit {
            rlim_cur: 1,
            rlim_max: 1,
        };
        assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &limit), 0);
    }
    assert!(prepared.writeback().is_err());
    assert_eq!(host_text(&fixture, "notes/keep.txt"), "keep");
    assert!(!workspace(&fixture).join("notes/new.txt").exists());
    assert!(fs::read_dir(workspace(&fixture).join("notes"))
        .unwrap()
        .all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("pywrite-")
        }));
}

#[test]
fn snapshot_leaves_out_links_hidden_files_and_oversized_files() {
    let fixture = sample();
    let big = workspace(&fixture).join("notes/big.bin");
    fs::write(&big, vec![1u8; super::MAX_FILE_BYTES as usize + 1]).unwrap();
    let prepared = prepare(&fixture);
    assert!(snap_file(&prepared, "README.md").is_file());
    assert!(snap_file(&prepared, "notes/keep.txt").is_file());
    assert!(absent(&snap_file(&prepared, "notes/dup.txt")));
    assert!(absent(&snap_file(&prepared, "notes/linked.txt")));
    assert!(absent(&snap_file(&prepared, "notes/escape")));
    assert!(absent(&snap_file(&prepared, "AGENTS.md")));
    assert!(absent(&snap_file(&prepared, ".hidden/token.txt")));
    assert!(absent(&snap_file(&prepared, ".git/config")));
    assert!(absent(&snap_file(&prepared, "notes/big.bin")));
    assert!(prepared.omitted.iter().any(|path| path == "notes/big.bin"));
    assert_eq!(mode_bits(&prepared.snapshot_dir().join("workspace")), 0o755);
    assert_eq!(mode_bits(&snap_file(&prepared, "README.md")), 0o444);
    assert_eq!(
        mode_bits(&prepared.snapshot_dir().join("workspace/notes")),
        0o755
    );
    assert_eq!(mode_bits(&snap_file(&prepared, "notes/keep.txt")), 0o644);
    assert_eq!(
        mode_bits(&prepared.snapshot_dir().join("attachments/clip.txt")),
        0o444
    );
    assert_eq!(
        mode_bits(&prepared.snapshot_dir().join("scratch/pad.txt")),
        0o644
    );
    assert!(prepared.writable_names().contains(&"notes".to_owned()));
    assert!(prepared.writable_names().contains(&"outputs".to_owned()));
    assert!(!prepared
        .writable_names()
        .iter()
        .any(|name| name == "README.md"));
    let report = prepared.writeback().unwrap();
    assert!(report.changed.is_empty(), "{:?}", report.changed);
    assert_eq!(host_text(&fixture, "notes/keep.txt"), "keep");
    assert_eq!(host_text(&fixture, "notes/dup.txt"), "linked");
    assert_eq!(host_text(&fixture, "notes/linked.txt"), "linked");
    assert_eq!(
        fs::read_to_string(fixture.outside.join("marker")).unwrap(),
        "OUTSIDE"
    );
}

fn snapshot_timestamp(path: &Path, seconds: u64) {
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(
            fs::FileTimes::new()
                .set_modified(std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)),
        )
        .unwrap();
}

#[test]
fn snapshot_file_selection_is_deterministic_with_outputs_then_recency_then_path() {
    let fixture = sample();
    fs::create_dir(workspace(&fixture).join("outputs")).unwrap();
    fs::write(workspace(&fixture).join("outputs/old.txt"), "output").unwrap();
    snapshot_timestamp(&workspace(&fixture).join("outputs/old.txt"), 1);
    for index in (0..super::MAX_SNAPSHOT_FILES + 10).rev() {
        let path = workspace(&fixture).join(format!("notes/file-{index:05}.txt"));
        fs::write(&path, "data").unwrap();
        snapshot_timestamp(&path, 100);
    }
    fs::write(workspace(&fixture).join("notes/recent.txt"), "recent").unwrap();
    snapshot_timestamp(&workspace(&fixture).join("notes/recent.txt"), 200);
    let first = prepare(&fixture);
    let second = prepare(&fixture);
    assert_eq!(first.omitted, second.omitted);
    assert_eq!(first.omitted_count, second.omitted_count);
    assert!(first.omitted_count >= 10);
    assert!(!first.scan_truncated);
    for path in [
        "outputs/old.txt",
        "notes/recent.txt",
        "notes/file-00000.txt",
    ] {
        assert!(snap_file(&first, path).is_file(), "{path}");
    }
    assert!(!snap_file(&first, "notes/file-02009.txt").exists());
    assert!(first.writeback().unwrap().changed.is_empty());
    assert_eq!(host_text(&fixture, "notes/file-02009.txt"), "data");
}

#[test]
fn python_runs_large_folders_and_reports_both_file_and_byte_omissions() {
    for files in [true, false] {
        let fixture = sample();
        if files {
            for index in 0..super::MAX_SNAPSHOT_FILES + 2 {
                fs::write(
                    workspace(&fixture).join(format!("notes/f-{index:05}.txt")),
                    "x",
                )
                .unwrap();
            }
        } else {
            for index in 0..5 {
                let path = workspace(&fixture).join(format!("notes/block-{index}.bin"));
                fs::File::create(path)
                    .unwrap()
                    .set_len(super::MAX_FILE_BYTES)
                    .unwrap();
            }
        }
        let value = run_python(&fixture, "from pathlib import Path\nprint('large-folder-ok')\nPath('/workspace/outputs/completed.txt').write_text('done')", 120);
        assert_eq!(value["status"], "completed", "{value}");
        assert!(value["stdout"]
            .as_str()
            .unwrap()
            .contains("large-folder-ok"));
        assert!(value["snapshot_omitted_count"].as_u64().unwrap() > 0);
        assert!(!value["snapshot_omitted_paths"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(host_text(&fixture, "outputs/completed.txt"), "done");
        assert!(!value.to_string().contains(TOKEN));
    }
}

#[test]
fn snapshot_omits_files_over_the_byte_budget_instead_of_refusing_the_folder() {
    let fixture = sample();
    let notes = workspace(&fixture).join("notes");
    let payload = vec![0u8; super::MAX_FILE_BYTES as usize];
    for name in ["a.bin", "b.bin", "c.bin", "d.bin"] {
        fs::write(notes.join(name), &payload).unwrap();
        snapshot_timestamp(&notes.join(name), 100);
    }
    fs::write(notes.join("e.bin"), b"e").unwrap();
    snapshot_timestamp(&notes.join("e.bin"), 200);
    let prepared = prepare(&fixture);
    assert!(prepared.omitted_count > 0);
    assert!(snap_file(&prepared, "notes/e.bin").is_file());
    let copied = ["a.bin", "b.bin", "c.bin", "d.bin"]
        .iter()
        .filter(|name| snap_file(&prepared, &format!("notes/{name}")).exists())
        .count();
    assert!(copied < 4);
}

#[test]
fn writeback_copies_granted_edits_and_refuses_link_races() {
    let fixture = sample();
    let prepared = prepare(&fixture);
    fs::write(snap_file(&prepared, "notes/keep.txt"), "guest").unwrap();
    fs::write(snap_file(&prepared, "notes/created.txt"), "created").unwrap();
    fs::write(
        snap_file(&prepared, "outputs/chart.png"),
        b"\x89PNG\r\n\x1a\nchart",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    let readme = snap_file(&prepared, "README.md");
    fs::set_permissions(&readme, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(&readme, "pwned-readme").unwrap();
    fs::write(
        prepared.snapshot_dir().join("scratch/pad.txt"),
        "guest-scratch",
    )
    .unwrap();
    std::os::unix::fs::symlink("/etc/passwd", snap_file(&prepared, "notes/planted")).unwrap();
    let report = prepared.writeback().unwrap();
    assert_eq!(host_text(&fixture, "notes/keep.txt"), "guest");
    assert_eq!(host_text(&fixture, "notes/created.txt"), "created");
    assert_eq!(host_text(&fixture, "README.md"), README);
    assert_eq!(
        fs::read(workspace(&fixture).join("outputs/chart.png")).unwrap(),
        b"\x89PNG\r\n\x1a\nchart"
    );
    assert!(absent(&workspace(&fixture).join("notes/planted")));
    assert_eq!(
        fs::read_to_string(fixture.roots.scratch.join("pad.txt")).unwrap(),
        "pad"
    );
    assert!(report.changed.iter().any(|path| path == "notes/keep.txt"));
    assert!(report
        .changed
        .iter()
        .any(|path| path == "outputs/chart.png"));
    assert!(!report
        .changed
        .iter()
        .any(|path| path == "README.md" || path.contains("pad")));
    let chart = report
        .artifacts
        .iter()
        .find(|item| item.path == "outputs/chart.png")
        .unwrap();
    assert_eq!(chart.kind, "chart");
    assert_eq!(chart.media_type, "image/png");
    assert!(conflicts(&report)
        .iter()
        .any(|(path, reason)| path == "notes/planted" && reason == "symlink"));

    let raced = sample();
    let prepared = prepare(&raced);
    fs::write(workspace(&raced).join("notes/keep.txt"), "host-edit").unwrap();
    fs::write(snap_file(&prepared, "notes/keep.txt"), "guest-edit").unwrap();
    let report = prepared.writeback().unwrap();
    assert_eq!(host_text(&raced, "notes/keep.txt"), "host-edit");
    assert!(conflicts(&report)
        .iter()
        .any(|(path, reason)| path == "notes/keep.txt" && reason == "host_changed"));

    let linked = sample();
    let prepared = prepare(&linked);
    fs::hard_link(
        workspace(&linked).join("notes/keep.txt"),
        workspace(&linked).join("notes/alias.txt"),
    )
    .unwrap();
    fs::write(snap_file(&prepared, "notes/keep.txt"), "through-link").unwrap();
    let report = prepared.writeback().unwrap();
    assert_eq!(host_text(&linked, "notes/keep.txt"), "keep");
    assert_eq!(host_text(&linked, "notes/alias.txt"), "keep");
    assert!(conflicts(&report)
        .iter()
        .any(|(path, reason)| path == "notes/keep.txt" && reason == "hard_link"));

    let swapped = sample();
    let prepared = prepare(&swapped);
    fs::remove_file(workspace(&swapped).join("notes/keep.txt")).unwrap();
    std::os::unix::fs::symlink(
        swapped.outside.join("marker"),
        workspace(&swapped).join("notes/keep.txt"),
    )
    .unwrap();
    fs::write(snap_file(&prepared, "notes/keep.txt"), "replaced-target").unwrap();
    let report = prepared.writeback().unwrap();
    assert!(
        fs::symlink_metadata(workspace(&swapped).join("notes/keep.txt"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_to_string(swapped.outside.join("marker")).unwrap(),
        "OUTSIDE"
    );
    assert!(conflicts(&report)
        .iter()
        .any(|(path, reason)| path == "notes/keep.txt" && reason == "symlink"));

    let outputs = sample();
    std::os::unix::fs::symlink(&outputs.outside, workspace(&outputs).join("outputs")).unwrap();
    let prepared = prepare(&outputs);
    fs::write(snap_file(&prepared, "outputs/pwned.txt"), "pwned").unwrap();
    let report = prepared.writeback().unwrap();
    assert!(fs::symlink_metadata(workspace(&outputs).join("outputs"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(absent(&outputs.outside.join("pwned.txt")));
    assert_eq!(
        fs::read_to_string(outputs.outside.join("marker")).unwrap(),
        "OUTSIDE"
    );
    assert!(conflicts(&report)
        .iter()
        .any(|(path, reason)| path == "outputs" && reason == "symlink"));

    let fresh = sample();
    let prepared = prepare(&fresh);
    fs::write(workspace(&fresh).join("notes/new.txt"), "host-new").unwrap();
    fs::write(snap_file(&prepared, "notes/new.txt"), "guest-new").unwrap();
    let report = prepared.writeback().unwrap();
    assert_eq!(host_text(&fresh, "notes/new.txt"), "host-new");
    assert!(conflicts(&report)
        .iter()
        .any(|(path, reason)| path == "notes/new.txt" && reason == "host_changed"));
}

#[test]
fn writeback_stops_at_the_file_cap() {
    let fixture = sample();
    let prepared = prepare(&fixture);
    let outputs = prepared.snapshot_dir().join("workspace/outputs");
    for index in 0..300 {
        fs::write(outputs.join(format!("f{index}.txt")), "x").unwrap();
    }
    let report = prepared.writeback().unwrap();
    assert!(report.truncated);
    assert!(report.changed.len() <= super::MAX_WRITEBACK_FILES);
    let copied = fs::read_dir(workspace(&fixture).join("outputs"))
        .unwrap()
        .count();
    assert!(copied <= super::MAX_WRITEBACK_FILES);
    assert!(copied < 300);
}

#[test]
fn writeback_rechecks_new_paths_against_the_host_policy() {
    let mut fixture = sample();
    fixture.roots.app_data = workspace(&fixture).join("notes/private");
    fixture.roots.global = workspace(&fixture).join("notes/instructions");
    let prepared = prepare(&fixture);
    for relative in [
        "notes/research.git/config",
        "notes/research.git/hooks/pre-commit",
        "notes/private/state.json",
        "notes/instructions/AGENTS.md",
        "notes/instructions/skills/example/SKILL.md",
        "notes/ordinary.txt",
    ] {
        let path = snap_file(&prepared, relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "guest result").unwrap();
    }
    let report = prepared.writeback().unwrap();
    assert_eq!(report.changed, ["notes/ordinary.txt"]);
    assert_eq!(host_text(&fixture, "notes/ordinary.txt"), "guest result");
    for relative in [
        "notes/research.git/config",
        "notes/research.git/hooks",
        "notes/private",
        "notes/instructions/AGENTS.md",
        "notes/instructions/skills/example",
    ] {
        assert!(absent(&workspace(&fixture).join(relative)), "{relative}");
        assert!(
            conflicts(&report).contains(&(relative.into(), "protected".into())),
            "{relative}"
        );
    }
}

#[test]
fn writeback_does_not_create_outputs_over_a_protected_root() {
    let mut fixture = sample();
    fixture.roots.app_data = workspace(&fixture).join("outputs");
    let prepared = prepare(&fixture);
    fs::write(snap_file(&prepared, "outputs/result.txt"), "result").unwrap();
    let report = prepared.writeback().unwrap();
    assert!(absent(&workspace(&fixture).join("outputs")));
    assert!(conflicts(&report).contains(&("outputs".into(), "protected".into())));
}

#[test]
fn writeback_never_exceeds_the_total_byte_budget() {
    let fixture = sample();
    let prepared = prepare(&fixture);
    // Five legal-sized files total 35 MiB: the fifth must stay private.
    for index in 0..5 {
        fs::write(
            snap_file(&prepared, &format!("outputs/{index}.bin")),
            vec![42; 7 * 1024 * 1024],
        )
        .unwrap();
    }
    let report = prepared.writeback().unwrap();
    assert!(report.truncated);
    assert_eq!(report.artifacts.len(), 4);
    let host_bytes: u64 = fs::read_dir(workspace(&fixture).join("outputs"))
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
    assert_eq!(host_bytes, 28 * 1024 * 1024);
    assert!(host_bytes <= super::MAX_WRITEBACK_BYTES);
}

#[test]
fn private_snapshot_is_removed_after_use() {
    let fixture = sample();
    fs::create_dir_all(fixture.roots.attachments.join("nested")).unwrap();
    fs::write(fixture.roots.attachments.join("nested/clip.txt"), "clip").unwrap();
    let prepared = prepare(&fixture);
    let path = prepared.snapshot_dir().to_path_buf();
    drop(prepared);
    assert!(
        !path.exists(),
        "private snapshot remains: {}",
        path.display()
    );
}

#[test]
fn oversized_files_do_not_discard_other_results() {
    let fixture = sample();
    fs::write(workspace(&fixture).join("notes/deleted.txt"), "original").unwrap();
    let prepared = prepare(&fixture);
    fs::remove_file(snap_file(&prepared, "notes/deleted.txt")).unwrap();
    fs::write(
        workspace(&fixture).join("notes/deleted.txt"),
        vec![0; super::MAX_FILE_BYTES as usize + 1],
    )
    .unwrap();
    fs::write(
        snap_file(&prepared, "outputs/large.bin"),
        vec![0; super::MAX_FILE_BYTES as usize + 1],
    )
    .unwrap();
    fs::write(snap_file(&prepared, "outputs/kept.txt"), "kept").unwrap();
    fs::write(
        workspace(&fixture).join("notes/keep.txt"),
        vec![0; super::MAX_FILE_BYTES as usize + 1],
    )
    .unwrap();
    fs::write(snap_file(&prepared, "notes/keep.txt"), "guest").unwrap();
    let report = prepared.writeback().unwrap();
    assert_eq!(host_text(&fixture, "outputs/kept.txt"), "kept");
    assert!(!workspace(&fixture).join("outputs/large.bin").exists());
    assert!(conflicts(&report).contains(&("outputs/large.bin".into(), "too_large".into())));
    assert!(conflicts(&report).contains(&("notes/keep.txt".into(), "host_changed".into())));
    assert!(conflicts(&report).contains(&("notes/deleted.txt".into(), "host_changed".into())));
}

#[test]
fn removing_an_existing_directory_reports_a_conflict() {
    let fixture = sample();
    fs::create_dir(workspace(&fixture).join("notes/archive")).unwrap();
    fs::write(
        workspace(&fixture).join("notes/archive/data.txt"),
        "original",
    )
    .unwrap();
    let prepared = prepare(&fixture);
    fs::remove_dir_all(snap_file(&prepared, "notes/archive")).unwrap();
    let report = prepared.writeback().unwrap();
    assert_eq!(host_text(&fixture, "notes/archive/data.txt"), "original");
    assert!(conflicts(&report).contains(&("notes/archive".into(), "directory_removed".into())));
}

#[test]
fn directory_traversal_and_writable_roots_are_bounded() {
    let fixture = sample();
    let prepared = prepare(&fixture);
    let deep = format!("outputs/{}/end.txt", vec!["nested"; 40].join("/"));
    let path = snap_file(&prepared, &deep);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "deep").unwrap();
    assert!(prepared.writeback().unwrap().truncated);
    assert!(!workspace(&fixture).join(deep).exists());
    for index in 0..super::MAX_WRITABLE_DIRS {
        fs::create_dir(workspace(&fixture).join(format!("folder-{index}"))).unwrap();
    }
    let prepared = prepare(&fixture);
    assert!(prepared.writable_names().len() <= super::MAX_WRITABLE_DIRS);
    assert!(prepared.scan_truncated);
    assert!(prepared.omitted_count > 0);
}

#[test]
fn the_worker_dies_when_its_parent_shell_exits() {
    let worker = super::worker_path().expect("worker binary");
    let dir = TempDir::new().unwrap();
    let pid_path = dir.path().join("pid");
    let script = format!(
        "exec 3<&0\n\"{worker}\" 0<&3 >/dev/null 2>&1 &\necho $! > \"{pid}\"\nexec 3<&-\necho ready\nsleep 2\n",
        worker = worker.display(),
        pid = pid_path.display(),
    );
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("shell");
    let _stdin = child.stdin.take().expect("stdin");
    let mut stdout = child.stdout.take().expect("stdout");
    let mut seen = Vec::new();
    let mut byte = [0u8; 1];
    let ready_deadline = Instant::now() + Duration::from_secs(5);
    while !seen.ends_with(b"ready\n") {
        assert!(
            Instant::now() < ready_deadline,
            "shell did not start the worker"
        );
        assert_eq!(stdout.read(&mut byte).unwrap(), 1);
        seen.push(byte[0]);
    }
    let pid: i32 = fs::read_to_string(&pid_path)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        unsafe { libc::kill(pid, 0) },
        0,
        "worker exited before its parent"
    );
    let status = child.wait().unwrap();
    assert!(status.success());
    let mut dead = false;
    for _ in 0..50 {
        if unsafe { libc::kill(pid, 0) } != 0 {
            dead = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !dead {
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    assert!(dead, "worker {pid} survived its parent");
}

fn host_call_wasm() -> Vec<u8> {
    fn leb(mut value: usize) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            out.push(byte);
            if value == 0 {
                break;
            }
        }
        out
    }
    fn section(id: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![id];
        out.extend(leb(body.len()));
        out.extend_from_slice(body);
        out
    }
    let mut module = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    let types = vec![
        2, 0x60, 4, 0x7f, 0x7f, 0x7f, 0x7f, 1, 0x7f, 0x60, 0, 1, 0x7f,
    ];
    module.extend(section(1, &types));
    let mut imports = vec![1, 4];
    imports.extend(b"host");
    imports.push(4);
    imports.extend(b"call");
    imports.extend_from_slice(&[0x00, 0x00]);
    module.extend(section(2, &imports));
    module.extend(section(3, &[1, 1]));
    let mut exports = vec![1, 3];
    exports.extend(b"run");
    exports.extend_from_slice(&[0x00, 0x01]);
    module.extend(section(7, &exports));
    let body = [
        0x00, 0x41, 0x00, 0x41, 0x00, 0x41, 0x00, 0x41, 0x00, 0x10, 0x00, 0x0b,
    ];
    let mut code = vec![1];
    code.extend(leb(body.len()));
    code.extend_from_slice(&body);
    module.extend(section(10, &code));
    let _ = types;
    module
}

#[test]
fn host_call_traps_without_leaking_the_runtime_path() {
    let worker = super::worker_path().expect("worker binary");
    let dir = TempDir::new().unwrap();
    let runtime = dir.path().join("runtime");
    let snapshot = dir.path().join("snapshot");
    for path in [
        runtime.join("lib"),
        runtime.join("mpl-data"),
        snapshot.join("workspace"),
        snapshot.join("attachments"),
        snapshot.join("scratch"),
        dir.path().join("cache"),
    ] {
        fs::create_dir_all(path).unwrap();
    }
    fs::write(runtime.join("python-snapshot.wasm"), host_call_wasm()).unwrap();
    let request = json!({
        "code": "print(1)",
        "timeout_seconds": 30,
        "runtime": runtime,
        "cache": dir.path().join("cache"),
        "snapshot": snapshot,
        "writable": [],
    });
    let mut child = Command::new(&worker)
        .current_dir(dir.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("worker");
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("reap");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(126),
        "stdout {} stderr {stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        !stderr.contains(dir.path().to_string_lossy().as_ref()),
        "{stderr}"
    );
    assert!(!stderr.contains("python-snapshot.wasm"), "{stderr}");
}

#[test]
fn managed_workspace_reads_input_and_copies_back_outputs_without_app_data() {
    let _guard = lock_worker();
    let mut fixture = sample();
    let managed = fixture
        .roots
        .app_data
        .join("loupists/portfolio-012345678abc");
    fs::create_dir_all(managed.parent().unwrap()).unwrap();
    fs::rename(&fixture.roots.workspace, &managed).unwrap();
    fixture.roots.workspace = managed;
    fixture.roots.loupist_data = fixture.roots.workspace.join(".loupist");
    let value = run_python(
        &fixture,
        r#"
from pathlib import Path
assert Path('/workspace/notes/keep.txt').read_text() == 'keep'
assert not Path('/workspace/.loupist').exists()
assert not Path('/workspace/.git').exists()
assert not Path('/workspace/notes/escape').exists()
assert not Path('/workspace/../secret.txt').exists()
Path('/workspace/outputs/managed.txt').write_text('managed-result')
Path('/workspace/notes/result.txt').write_text('existing-folder')
"#,
        60,
    );
    assert_eq!(value["exit_code"], 0, "{value}");
    assert_eq!(value["conflicts"], json!([]), "{value}");
    assert_eq!(host_text(&fixture, "outputs/managed.txt"), "managed-result");
    assert_eq!(host_text(&fixture, "notes/result.txt"), "existing-folder");
    assert_eq!(
        fs::read_to_string(fixture.roots.app_data.join("secret.txt")).unwrap(),
        TOKEN
    );
}

#[test]
fn run_python_computes_with_six_offline_packages_and_writes_a_chart() {
    let _lock = lock_worker();
    require_runtime();
    let fixture = sample();
    let marker = fixture.outside.join("marker");
    let before = fs::read_to_string(&marker).unwrap();
    let value = run_python(&fixture, CHART_CODE, 120);
    assert_eq!(value["status"], "completed", "{value}");
    assert_eq!(value["exit_code"], 0, "{value}");
    let stdout = value["stdout"].as_str().unwrap();
    assert!(stdout.contains("slope=2.0"), "{value}");
    assert!(stdout.contains("ols=2.0"), "{value}");
    assert!(stdout.contains("sklearn=2.0"), "{value}");
    let chart = fs::read(workspace(&fixture).join("outputs/chart.png")).expect("chart");
    assert!(chart.starts_with(b"\x89PNG\r\n\x1a\n"), "{value}");
    assert_eq!(host_text(&fixture, "notes/result.txt"), "2.0\n");
    assert_eq!(host_text(&fixture, "README.md"), README);
    assert_eq!(fs::read_to_string(marker).unwrap(), before);
    let workspace = workspace(&fixture);
    let rendered = format!("{value}");
    assert!(!rendered.contains(&workspace.to_string_lossy().to_string()));
    let artifacts = value["artifacts"].as_array().unwrap();
    let chart_meta = artifacts
        .iter()
        .find(|item| item["path"] == "outputs/chart.png")
        .expect("artifact");
    assert_eq!(chart_meta["kind"], "chart");
    assert_eq!(chart_meta["media_type"], "image/png");
}

#[test]
fn run_python_refuses_paths_network_dns_javascript_and_subprocesses() {
    let _lock = lock_worker();
    require_runtime();
    let fixture = sample();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let touched = fixture.outside.join("touched-by-python");
    let marker = serde_json::to_string(&touched.to_string_lossy().to_string()).unwrap();
    let code = format!(
        r#"
import os, socket, subprocess
marker = {marker}
def show(label, action):
    try:
        value = action()
    except Exception:
        print(label + "-blocked")
        return
    print(label + "-ok")
    if value is not None:
        print(value)
show("readme", lambda: open("/workspace/README.md", "w").write("pwned"))
show("hidden", lambda: open("/workspace/.hidden/token.txt").read())
show("passwd", lambda: open("/etc/passwd").read())
show("escape", lambda: open("/workspace/notes/../../secret-escape", "w").write("pwned"))
show("attach", lambda: open("/attachments/clip.txt", "w").write("pwned"))
show("plant", lambda: os.symlink("/etc/passwd", "/workspace/notes/planted"))
show("tcp", lambda: socket.create_connection(("127.0.0.1", {port}), 1))
show("dns", lambda: socket.getaddrinfo("example.invalid", 80))
show("subprocess", lambda: subprocess.check_call(["/bin/touch", marker]))
show("node", lambda: subprocess.check_call(["node", "-e", "open(marker,'w').write('js')"]))
show("js", lambda: __import__("js"))
try:
    print("system", os.system("touch " + marker))
except Exception:
    print("system-blocked")
print("notes", ",".join(sorted(os.listdir("/workspace/notes"))))
print("scan-done")
"#
    );
    let value = run_python(&fixture, &code, 120);
    let stdout = value["stdout"].as_str().unwrap_or("");
    assert!(stdout.contains("scan-done"), "{value}");
    assert!(!stdout.contains(TOKEN), "{value}");
    assert!(!stdout.contains("tcp-ok"), "{value}");
    assert!(!stdout.contains("dns-ok"), "{value}");
    assert!(!stdout.contains("subprocess-ok"), "{value}");
    assert!(!stdout.contains("node-ok"), "{value}");
    assert!(!stdout.contains("js-ok"), "{value}");
    let notes = stdout
        .lines()
        .find(|line| line.starts_with("notes "))
        .unwrap_or(stdout);
    assert!(!notes.contains("dup"), "{value}");
    assert!(!notes.contains("linked"), "{value}");
    assert!(!notes.contains("escape"), "{value}");
    let mut accepts = 0;
    loop {
        match listener.accept() {
            Ok(_) => accepts += 1,
            Err(error) if error.kind() == ErrorKind::WouldBlock => break,
            Err(error) => panic!("{error}"),
        }
    }
    assert_eq!(accepts, 0);
    assert!(absent(&touched));
    assert!(absent(&fixture._tmp.path().join("secret-escape")));
    assert!(absent(&workspace(&fixture).join("secret-escape")));
    assert!(absent(&workspace(&fixture).join("notes/planted")));
    assert_eq!(host_text(&fixture, "README.md"), README);
    assert_eq!(
        fs::read_to_string(fixture.roots.attachments.join("clip.txt")).unwrap(),
        "clip"
    );
    assert_eq!(
        fs::read_to_string(fixture.roots.app_data.join("secret.txt")).unwrap(),
        TOKEN
    );
    assert_eq!(
        fs::read_to_string(fixture.outside.join("marker")).unwrap(),
        "OUTSIDE"
    );
    assert_eq!(host_text(&fixture, "notes/keep.txt"), "keep");
    assert!(
        fs::symlink_metadata(workspace(&fixture).join("notes/escape"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn an_output_flood_keeps_only_the_file_written_before_the_cap() {
    let _lock = lock_worker();
    require_runtime();
    let fixture = sample();
    let code = r#"
import sys
open("/workspace/outputs/before.txt", "w").write("before")
sys.stdout.write("x" * 200000)
sys.stdout.flush()
open("/workspace/outputs/after.txt", "w").write("after")
"#;
    let value = run_python(&fixture, code, 120);
    assert_eq!(value["status"], "failed", "{value}");
    assert_eq!(value["stdout_truncated"], true, "{value}");
    assert!(value["stdout"].as_str().unwrap().len() <= 64 * 1024);
    assert_eq!(host_text(&fixture, "outputs/before.txt"), "before");
    assert!(absent(&workspace(&fixture).join("outputs/after.txt")));
    assert_eq!(host_text(&fixture, "README.md"), README);
}

#[test]
fn a_memory_allocation_past_the_cap_does_not_publish_a_file() {
    let _lock = lock_worker();
    require_runtime();
    let fixture = sample();
    let code = r#"
open("/workspace/outputs/before.txt", "w").write("before")
chunk = bytearray(800 * 1024 * 1024)
open("/workspace/outputs/allocated.txt", "w").write(str(len(chunk)))
"#;
    let value = run_python(&fixture, code, 120);
    assert_ne!(value["status"], "completed", "{value}");
    assert_eq!(host_text(&fixture, "outputs/before.txt"), "before");
    assert!(absent(&workspace(&fixture).join("outputs/allocated.txt")));
}

#[test]
fn cancellation_copies_back_the_file_written_before_the_kill() {
    let _lock = lock_worker();
    require_runtime();
    let fixture = sample();
    let code = r#"
open("/workspace/outputs/before.txt", "w").write("before")
import time
time.sleep(300)
open("/workspace/outputs/after.txt", "w").write("after")
"#;
    let value = runtime().block_on(async {
        warm().await;
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let (started_tx, mut started_rx) = tokio::sync::oneshot::channel();
        let roots = fixture.roots.clone();
        let cache = shared_cache().to_path_buf();
        let mut task = tokio::spawn(async move {
            super::execute_observed(
                &roots,
                &cache,
                code,
                120,
                SearchCancel::from_watch(cancel_rx),
                started_tx,
            )
            .await
        });
        let snapshot = tokio::select! {
            path = &mut started_rx => path.expect("snapshot"),
            joined = &mut task => panic!("python ended before the snapshot was ready: {joined:?}"),
        };
        let before = snapshot.join("workspace/outputs/before.txt");
        let deadline = Instant::now() + Duration::from_secs(100);
        while fs::read_to_string(&before).ok().as_deref() != Some("before") {
            if Instant::now() > deadline || task.is_finished() {
                let ended = task.await.ok();
                panic!("before.txt was not written: {ended:?}");
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        cancel_tx.send(true).unwrap();
        task.await.expect("join").expect("python.run")
    });
    assert_eq!(value["status"], "cancelled", "{value}");
    assert_eq!(host_text(&fixture, "outputs/before.txt"), "before");
    assert!(absent(&workspace(&fixture).join("outputs/after.txt")));
    assert_eq!(host_text(&fixture, "README.md"), README);
    assert_eq!(
        fs::read_to_string(fixture.outside.join("marker")).unwrap(),
        "OUTSIDE"
    );
}

#[test]
fn queued_calls_cancel_and_time_out_before_creating_a_snapshot() {
    let _lock = lock_worker();
    let fixture = sample();
    runtime().block_on(async {
        let _busy = super::WORKERS.acquire().await.unwrap();
        let (cancel, cancelled) = tokio::sync::watch::channel(false);
        let (started, ready) = tokio::sync::oneshot::channel();
        let pending = super::execute_observed(
            &fixture.roots,
            shared_cache(),
            "open('/workspace/outputs/queued.txt', 'w').write('unexpected')",
            120,
            SearchCancel::from_watch(cancelled),
            started,
        );
        cancel.send_replace(true);
        let error = tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .expect("queued cancellation must not wait for the active worker")
            .unwrap_err();
        assert!(error.to_string().contains("cancelled"), "{error}");
        assert!(ready.await.is_err(), "queued call created a snapshot");

        let (started, ready) = tokio::sync::oneshot::channel();
        let error = super::execute_observed(
            &fixture.roots,
            shared_cache(),
            "print('unexpected')",
            1,
            SearchCancel::never(),
            started,
        )
        .await
        .unwrap_err();
        assert!(
            error.to_string().contains("timed out while waiting"),
            "{error}"
        );
        assert!(ready.await.is_err(), "queued call created a snapshot");
        assert!(absent(&workspace(&fixture).join("outputs/queued.txt")));
    });
}

#[test]
fn a_disconnected_caller_still_reaps_and_copies_back_prior_files() {
    let _lock = lock_worker();
    require_runtime();
    let fixture = sample();
    runtime().block_on(async {
        warm().await;
        let roots = fixture.roots.clone();
        let (started, ready) = tokio::sync::oneshot::channel();
        let caller = tokio::spawn(async move {
            super::execute_observed(&roots, shared_cache(),
                "import time\nopen('/workspace/outputs/before.txt', 'w').write('before')\ntime.sleep(90)\nopen('/workspace/outputs/after.txt', 'w').write('after')",
                120, SearchCancel::never(), started).await
        });
        let snapshot = ready.await.unwrap();
        let deadline = Instant::now() + Duration::from_secs(100);
        while fs::read_to_string(snapshot.join("workspace/outputs/before.txt"))
            .ok()
            .as_deref()
            != Some("before")
        {
            assert!(Instant::now() < deadline && !caller.is_finished(), "worker never produced the file");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        // The broker drops this future when the extension disconnects.
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        let deadline = Instant::now() + Duration::from_secs(10);
        while snapshot.exists() {
            assert!(Instant::now() < deadline, "worker cleanup did not finish");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert_eq!(host_text(&fixture, "outputs/before.txt"), "before");
        assert!(!workspace(&fixture).join("outputs/after.txt").exists());
    });
}

fn run_timed(fixture: &Fixture, cache: &Path, timeout: u64) -> (Value, Duration, u64) {
    let peak_kb = std::sync::Arc::new(AtomicU64::new(0));
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let peak_task = peak_kb.clone();
    let stop_task = stop.clone();
    let sampler = std::thread::spawn(move || {
        while !stop_task.load(Ordering::Relaxed) {
            if let Ok(output) = Command::new("ps")
                .args(["-ax", "-o", "rss=,command="])
                .output()
            {
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    if !line.contains("loupist-python-worker") {
                        continue;
                    }
                    let Some(rss) = line
                        .split_whitespace()
                        .next()
                        .and_then(|value| value.parse::<u64>().ok())
                    else {
                        continue;
                    };
                    peak_task.fetch_max(rss, Ordering::Relaxed);
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    });
    let started = Instant::now();
    let value = runtime().block_on(async {
        super::execute(
            &fixture.roots,
            cache,
            CHART_CODE,
            timeout,
            SearchCancel::never(),
        )
        .await
        .expect("python.run")
    });
    let elapsed = started.elapsed();
    stop.store(true, Ordering::Relaxed);
    sampler.join().expect("sampler");
    (value, elapsed, peak_kb.load(Ordering::Relaxed))
}

#[test]
fn a_cold_cache_finishes_the_default_chart_and_a_warm_call_reuses_it() {
    let _lock = lock_worker();
    require_runtime();
    let cache = TempDir::new().unwrap();
    let cold_fixture = sample();
    let (cold, cold_elapsed, cold_rss_kb) = run_timed(&cold_fixture, cache.path(), 60);
    assert_eq!(
        cold["status"], "completed",
        "cold {:?} rss {} KiB {cold}",
        cold_elapsed, cold_rss_kb
    );
    assert!(
        cold["stdout"].as_str().unwrap_or("").contains("slope=2.0"),
        "{cold}"
    );
    assert!(fs::read(workspace(&cold_fixture).join("outputs/chart.png"))
        .expect("cold chart")
        .starts_with(b"\x89PNG\r\n\x1a\n"));
    let warm_fixture = sample();
    let (warm, warm_elapsed, warm_rss_kb) = run_timed(&warm_fixture, cache.path(), 60);
    assert_eq!(
        warm["status"], "completed",
        "warm {:?} rss {} KiB {warm}",
        warm_elapsed, warm_rss_kb
    );
    assert!(
        warm["stdout"].as_str().unwrap_or("").contains("slope=2.0"),
        "{warm}"
    );
    assert!(fs::read(workspace(&warm_fixture).join("outputs/chart.png"))
        .expect("warm chart")
        .starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(
        warm_elapsed < cold_elapsed,
        "warm {warm_elapsed:?} rss {warm_rss_kb} KiB was not faster than cold {cold_elapsed:?} rss {cold_rss_kb} KiB"
    );
    eprintln!(
        "python cache cold {cold_elapsed:?} peak {} MiB; warm {warm_elapsed:?} peak {} MiB",
        cold_rss_kb / 1024,
        warm_rss_kb / 1024
    );
}

#[test]
fn a_wall_timeout_copies_back_the_file_written_before_the_kill() {
    let _lock = lock_worker();
    require_runtime();
    let fixture = sample();
    let code = r#"
open("/workspace/outputs/before.txt", "w").write("before")
import time
time.sleep(300)
open("/workspace/outputs/after.txt", "w").write("after")
"#;
    // A warm start writes the first file in about a second; the limit only has
    // to outlast that, and the whole wait is spent inside the serial worker.
    let value = run_python(&fixture, code, 20);
    assert_eq!(value["status"], "timed_out", "{value}");
    assert_eq!(host_text(&fixture, "outputs/before.txt"), "before");
    assert!(absent(&workspace(&fixture).join("outputs/after.txt")));
    assert_eq!(host_text(&fixture, "README.md"), README);
}
