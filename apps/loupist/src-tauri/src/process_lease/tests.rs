#[cfg(any(target_os = "macos", target_os = "linux"))]
mod unix {
    use super::super::*;
    use std::{
        os::unix::{fs::OpenOptionsExt, process::CommandExt},
        process::{Command, Stdio},
    };

    const HELPER_DIRECTORY_ENV: &str = "LOUPIST_LEASE_HELPER_DIRECTORY";
    const HELPER_READY_ENV: &str = "LOUPIST_LEASE_HELPER_READY";

    fn spawn_sleep() -> std::process::Child {
        spawn_sleep_at(Path::new("/bin/sleep"))
    }

    fn spawn_sleep_at(executable: &Path) -> std::process::Child {
        let mut command = Command::new(executable);
        command
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        command.spawn().unwrap()
    }

    fn kill_and_reap(child: &mut std::process::Child, process_group_id: i32) {
        signal_process_group(process_group_id, libc::SIGKILL).unwrap();
        child.wait().unwrap();
        assert!(wait_for_group_exit_blocking(process_group_id, Duration::from_secs(2)).unwrap());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn proc_stat_identity_tolerates_arbitrary_command_names_and_rejects_invalid_fields() {
        let mut fields = vec!["S", "1", "4321"];
        fields.extend(std::iter::repeat_n("0", 16));
        fields.push("98765");
        let mut stat = b"1234 (a weird ) name\n\xff) ".to_vec();
        stat.extend_from_slice(fields.join(" ").as_bytes());
        assert_eq!(
            parse_linux_process_stat(1234, &stat).unwrap(),
            LinuxBasicProcess {
                process_group_id: 4321,
                start_ticks: 98765,
                zombie: false
            }
        );
        assert!(parse_linux_process_stat(1235, &stat).is_err());
        assert!(parse_linux_process_stat(1234, b"1234 (truncated) S 1 4321").is_err());
        assert!(parse_linux_process_stat(1234, b"1234 no command delimiters").is_err());
        fields[0] = "Z";
        let zombie = format!("1234 (finished) {}", fields.join(" "));
        assert!(
            parse_linux_process_stat(1234, zombie.as_bytes())
                .unwrap()
                .zombie
        );
        fields[19] = "invalid";
        let invalid = format!("1234 (bad ticks) {}", fields.join(" "));
        assert!(parse_linux_process_stat(1234, invalid.as_bytes()).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn filtered_or_missing_procfs_cannot_prove_a_group_has_exited() {
        let mount = "53 52 0:30 / /proc rw,nosuid,nodev,noexec,relatime - proc proc ";
        assert!(linux_procfs_mount_visible(&format!("{mount}rw")));
        assert!(linux_procfs_mount_visible(&format!("{mount}rw,hidepid=0")));
        for options in [
            "rw,hidepid=1",
            "rw,hidepid=2",
            "rw,hidepid=invisible",
            "rw,hidepid=ptraceable",
        ] {
            assert!(!linux_procfs_mount_visible(&format!("{mount}{options}")));
        }
        assert!(!linux_procfs_mount_visible(""));
        assert!(!linux_procfs_mount_visible(
            "53 52 0:30 / /proc rw - tmpfs tmpfs rw"
        ));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn linux_zombies_confirm_exit_without_authorizing_a_live_identity() {
        let temporary = tempfile::tempdir().unwrap();
        let leases = temporary.path().join("leases");
        let mut child = spawn_sleep();
        let group = child.id() as i32;
        let lease = ChildProcessLease::register(
            &leases,
            ProcessKind::Pi,
            group,
            group,
            Path::new("/bin/sleep"),
        )
        .unwrap();
        signal_process_group(group, libc::SIGKILL).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !linux_basic_process(group)
            .unwrap()
            .is_some_and(|info| info.zombie)
        {
            assert!(
                Instant::now() < deadline,
                "owned leader did not become a zombie"
            );
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        assert_eq!(unsafe { libc::kill(group, 0) }, 0);
        assert!(query_process(group).unwrap().is_none());
        assert_eq!(process_group_state(group).unwrap(), GroupState::ZombiesOnly);
        assert!(confirm_process_group_exit(group, Duration::from_secs(1))
            .await
            .is_confirmed());
        recover_orphaned_processes(&leases).unwrap();
        assert_eq!(fs::read_dir(&leases).unwrap().count(), 0);
        child.wait().unwrap();
        lease.complete().unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_different_boot_identity_never_authorizes_the_live_process() {
        let temporary = tempfile::tempdir().unwrap();
        let leases = temporary.path().join("leases");
        let mut child = spawn_sleep();
        let pid = child.id() as i32;
        let lease = ChildProcessLease::register(
            &leases,
            ProcessKind::Pi,
            pid,
            pid,
            Path::new("/bin/sleep"),
        )
        .unwrap();
        let lease_path = leases.join(&lease.file_name);
        let mut record: ProcessLeaseRecord =
            serde_json::from_slice(&fs::read(&lease_path).unwrap()).unwrap();
        record.start.source = format!("linux_proc_stat_v1:{}", Uuid::new_v4());
        fs::write(&lease_path, serde_json::to_vec(&record).unwrap()).unwrap();
        recover_orphaned_processes(&leases).unwrap();
        assert!(!lease_path.exists());
        recover_orphaned_processes(&leases).unwrap();
        assert!(child.try_wait().unwrap().is_none());
        kill_and_reap(&mut child, pid);
        lease.complete().unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_live_descendant_keeps_a_zombie_leaders_group_live() {
        let temporary = tempfile::tempdir().unwrap();
        let leases = temporary.path().join("leases");
        let ready = temporary.path().join("ready");
        let exit = temporary.path().join("exit");
        let mut child = Command::new("/bin/sh")
            .args([
                "-c",
                "sleep 30 & printf '%s' $! > \"$1\"; while ! test -e \"$2\"; do sleep 0.01; done",
                "lease-child",
            ])
            .arg(&ready)
            .arg(&exit)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        let group = child.id() as i32;
        let deadline = Instant::now() + Duration::from_secs(2);
        while !ready.exists() {
            assert!(
                Instant::now() < deadline,
                "descendant fixture did not start"
            );
            std::thread::sleep(POLL_INTERVAL);
        }
        let lease = ChildProcessLease::register(
            &leases,
            ProcessKind::Mcp,
            group,
            group,
            Path::new("/bin/sh"),
        )
        .unwrap();
        fs::write(&exit, b"exit").unwrap();
        while !linux_basic_process(group)
            .unwrap()
            .is_some_and(|info| info.zombie)
        {
            assert!(
                Instant::now() < deadline,
                "owned leader did not become a zombie"
            );
            std::thread::sleep(POLL_INTERVAL);
        }
        assert_eq!(process_group_state(group).unwrap(), GroupState::Live);
        assert!(query_process(group).unwrap().is_none());
        assert!(matches!(
            recover_orphaned_processes(&leases),
            Err(ProcessLeaseError::UnknownProcessGroup)
        ));
        assert!(leases.join(&lease.file_name).is_file());
        kill_and_reap(&mut child, group);
        lease.complete().unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_reaped_after_stat_is_opened_is_absent() {
        let mut child = spawn_sleep();
        let pid = child.id() as i32;
        let stat = fs::File::open(format!("/proc/{pid}/stat")).unwrap();
        kill_and_reap(&mut child, pid);
        assert!(read_linux_process_stat(pid, stat).unwrap().is_none());
    }

    #[test]
    fn completed_lease_removal_is_idempotent_during_shutdown() {
        let temporary = tempfile::tempdir().unwrap();
        let leases = temporary.path().join("leases");
        prepare_lease_directory(&leases).unwrap();
        let name = lease_file_name(&Uuid::new_v4().simple().to_string());
        // Both cleanup owners have independently confirmed exit before removal.
        fs::write(leases.join(&name), "finished").unwrap();
        remove_record(&leases, &name).unwrap();
        remove_record(&leases, &name).unwrap();
        assert_eq!(fs::read_dir(&leases).unwrap().count(), 0);
        assert!(matches!(
            remove_record(&leases, "../escape"),
            Err(ProcessLeaseError::InvalidRecord)
        ));
    }

    #[tokio::test]
    async fn group_exit_is_confirmed_only_after_the_group_is_gone() {
        let mut child = spawn_sleep();
        let process_group_id = child.id() as i32;
        let outcome = confirm_process_group_exit(process_group_id, Duration::from_millis(10)).await;
        assert!(matches!(outcome, ProcessGroupTermination::Unconfirmed));

        kill_and_reap(&mut child, process_group_id);
        let outcome = confirm_process_group_exit(process_group_id, Duration::from_millis(10)).await;
        assert!(
            outcome.is_confirmed(),
            "unexpected exit observation: {outcome:?}"
        );
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn an_unreaped_zombie_confirms_group_exit_without_authorizing_a_live_identity() {
        let mut child = spawn_sleep();
        let group = child.id() as i32;
        signal_process_group(group, libc::SIGKILL).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if macos_group_member(group)
                .unwrap()
                .is_some_and(|info| info.zombie)
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "owned leader did not become a zombie"
            );
            tokio::time::sleep(POLL_INTERVAL).await;
        }
        // The PID remains reserved until wait(), while group liveness sees
        // the zombie as exited. A live executable lease cannot be issued.
        assert_eq!(unsafe { libc::kill(group, 0) }, 0);
        assert!(query_process(group).is_err());
        assert!(confirm_process_group_exit(group, Duration::from_secs(1))
            .await
            .is_confirmed());
        child.wait().unwrap();
    }

    #[test]
    fn identity_mismatch_never_kills_the_live_process() {
        let temporary = tempfile::tempdir().unwrap();
        let lease_directory = temporary.path().join("leases");
        let mut child = spawn_sleep();
        let pid = child.id() as i32;
        let lease = ChildProcessLease::register(
            &lease_directory,
            ProcessKind::Pi,
            pid,
            pid,
            Path::new("/bin/sleep"),
        )
        .unwrap();

        let lease_path = lease_directory.join(&lease.file_name);
        let mut record: ProcessLeaseRecord =
            serde_json::from_slice(&fs::read(&lease_path).unwrap()).unwrap();
        record.start.primary = record.start.primary.saturating_add(1);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&lease_path)
            .unwrap();
        file.write_all(&serde_json::to_vec(&record).unwrap())
            .unwrap();
        file.sync_all().unwrap();

        recover_orphaned_processes(&lease_directory).unwrap();
        assert!(!lease_path.exists());
        recover_orphaned_processes(&lease_directory).unwrap();
        assert!(child.try_wait().unwrap().is_none());
        kill_and_reap(&mut child, pid);
        lease.complete().unwrap();
    }

    #[test]
    fn same_start_different_executable_hash_keeps_the_lease() {
        let temporary = tempfile::tempdir().unwrap();
        let leases = temporary.path().join("leases");
        let mut child = spawn_sleep();
        let pid = child.id() as i32;
        let lease = ChildProcessLease::register(
            &leases,
            ProcessKind::Mcp,
            pid,
            pid,
            Path::new("/bin/sleep"),
        )
        .unwrap();
        let path = leases.join(&lease.file_name);
        let original = fs::read(&path).unwrap();
        let mut record: ProcessLeaseRecord = serde_json::from_slice(&original).unwrap();
        record.executable.path_sha256 = "0".repeat(64);
        let changed = serde_json::to_vec(&record).unwrap();
        fs::write(&path, &changed).unwrap();

        for _ in 0..2 {
            recover_orphaned_processes(&leases).unwrap();
            assert_eq!(fs::read(&path).unwrap(), changed);
            assert!(child.try_wait().unwrap().is_none());
        }
        // Retaining the lease permits a later fully verified recovery.
        fs::write(&path, original).unwrap();
        recover_orphaned_processes(&leases).unwrap();
        assert!(!path.exists());
        child.wait().unwrap();
        lease.complete().unwrap();
    }

    #[test]
    fn an_unlinked_executable_does_not_block_startup_or_forget_the_same_process() {
        for reused in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let executable = temporary.path().join("sleep");
            fs::copy("/bin/sleep", &executable).unwrap();
            let leases = temporary.path().join("leases");
            let mut child = spawn_sleep_at(&executable);
            let pid = child.id() as i32;
            let lease =
                ChildProcessLease::register(&leases, ProcessKind::Pi, pid, pid, &executable)
                    .unwrap();
            let path = leases.join(&lease.file_name);
            let mut record: ProcessLeaseRecord =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            if reused {
                record.start.primary = record.start.primary.saturating_add(1);
                fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
            }
            fs::remove_file(&executable).unwrap();

            recover_orphaned_processes(&leases).unwrap();
            assert_eq!(path.exists(), !reused);
            assert!(child.try_wait().unwrap().is_none());
            kill_and_reap(&mut child, pid);
            recover_orphaned_processes(&leases).unwrap();
            assert!(!path.exists());
            lease.complete().unwrap();
        }
    }

    #[tokio::test]
    async fn cancelled_sidecars_are_reaped_without_exhausting_lease_capacity() {
        let temporary = tempfile::tempdir().unwrap();
        let lease_directory = temporary.path().join("leases");
        for _ in 0..(MAX_LEASE_ENTRIES + 4) {
            let mut child = spawn_sleep();
            let process_group_id = child.id() as i32;
            let lease = ChildProcessLease::register(
                &lease_directory,
                ProcessKind::Pi,
                process_group_id,
                process_group_id,
                Path::new("/bin/sleep"),
            )
            .unwrap();
            terminate_and_reap_process_group(process_group_id, lease);
            let _ = child.wait();
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if fs::read_dir(&lease_directory).unwrap().count() == 0 {
                        break;
                    }
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
            })
            .await
            .unwrap();
        }
    }

    #[test]
    fn lease_reads_are_bounded_and_do_not_follow_symlinks() {
        let temporary = tempfile::tempdir().unwrap();
        let lease_directory = temporary.path().join("leases");
        prepare_lease_directory(&lease_directory).unwrap();
        let nonce = "a".repeat(32);
        let target = temporary.path().join("outside");
        fs::write(&target, b"not a lease").unwrap();
        std::os::unix::fs::symlink(&target, lease_directory.join(lease_file_name(&nonce))).unwrap();
        assert!(matches!(
            recover_orphaned_processes(&lease_directory),
            Err(ProcessLeaseError::UnsafeFilesystemEntry)
        ));
        assert_eq!(fs::read(&target).unwrap(), b"not a lease");

        fs::remove_file(lease_directory.join(lease_file_name(&nonce))).unwrap();
        let oversized = lease_directory.join(lease_file_name(&"b".repeat(32)));
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(oversized)
            .unwrap();
        file.write_all(&vec![b'x'; MAX_LEASE_BYTES as usize + 1])
            .unwrap();
        file.sync_all().unwrap();
        assert!(matches!(
            recover_orphaned_processes(&lease_directory),
            Err(ProcessLeaseError::LimitExceeded)
        ));
    }

    #[test]
    fn forced_parent_crash_is_recovered_on_the_next_start() {
        let temporary = tempfile::tempdir().unwrap();
        let lease_directory = temporary.path().join("leases");
        let ready = temporary.path().join("ready");
        let helper_name = "process_lease::tests::unix::crash_parent_helper";
        let mut helper = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", helper_name, "--nocapture"])
            .env(HELPER_DIRECTORY_ENV, &lease_directory)
            .env(HELPER_READY_ENV, &ready)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() && Instant::now() < deadline {
            std::thread::sleep(POLL_INTERVAL);
        }
        assert!(ready.exists(), "crash helper did not register its lease");
        let process_group_id: i32 = fs::read_to_string(&ready).unwrap().parse().unwrap();
        assert!(!helper.wait().unwrap().success());

        recover_orphaned_processes(&lease_directory).unwrap();
        assert_ne!(
            process_group_state(process_group_id).unwrap(),
            GroupState::Live
        );
        assert_eq!(fs::read_dir(&lease_directory).unwrap().count(), 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parallel_group_disappearance_stress_completes_every_lease() {
        const WORKERS: usize = 2;
        const ITERATIONS: usize = 128;

        std::thread::scope(|scope| {
            let mut workers = Vec::new();
            for _ in 0..WORKERS {
                workers.push(scope.spawn(|| {
                    let temporary = tempfile::tempdir().unwrap();
                    let lease_directory = temporary.path().join("leases");
                    for _ in 0..ITERATIONS {
                        let mut child = spawn_sleep();
                        let process_group_id = child.id() as i32;
                        let lease = ChildProcessLease::register(
                            &lease_directory,
                            ProcessKind::Pi,
                            process_group_id,
                            process_group_id,
                            Path::new("/bin/sleep"),
                        )
                        .unwrap();
                        signal_process_group(process_group_id, libc::SIGTERM).unwrap();
                        child.wait().unwrap();
                        assert!(
                            wait_for_group_exit_blocking(process_group_id, Duration::from_secs(2),)
                                .unwrap(),
                            "process group {process_group_id} did not exit"
                        );
                        lease.complete().unwrap();
                    }
                    assert_eq!(fs::read_dir(&lease_directory).unwrap().count(), 0);
                }));
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
    }

    #[test]
    fn crash_parent_helper() {
        let (Ok(directory), Ok(ready)) = (
            std::env::var(HELPER_DIRECTORY_ENV),
            std::env::var(HELPER_READY_ENV),
        ) else {
            return;
        };
        let child = spawn_sleep();
        let pid = child.id() as i32;
        let _lease = ChildProcessLease::register(
            Path::new(&directory),
            ProcessKind::Mcp,
            pid,
            pid,
            Path::new("/bin/sleep"),
        )
        .unwrap();
        let ready = Path::new(&ready);
        let pending_marker = ready.with_extension(format!("{pid}.tmp"));
        let mut marker = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&pending_marker)
            .unwrap();
        write!(marker, "{pid}").unwrap();
        marker.sync_all().unwrap();
        drop(marker);
        fs::rename(pending_marker, ready).unwrap();

        // SAFETY: this helper is a disposable subprocess created specifically
        // to emulate an ungraceful parent crash.
        unsafe { libc::kill(libc::getpid(), libc::SIGKILL) };
        std::process::abort();
    }
}

#[cfg(windows)]
mod windows_tests {
    use super::super::*;
    use std::{process::Stdio, time::Duration};
    use tokio::{process::Command, time::timeout};

    #[tokio::test]
    async fn job_object_owns_and_terminates_the_suspended_child() {
        let mut command = Command::new("cmd.exe");
        command
            .args(["/D", "/S", "/C", "ping -n 30 127.0.0.1 > nul"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        ChildProcessJob::configure_command(&mut command);
        let mut child = command.spawn().unwrap();
        let job = ChildProcessJob::assign(&child).unwrap();
        assert!(child.try_wait().unwrap().is_none());

        job.terminate_and_wait(Duration::from_secs(2))
            .await
            .unwrap();
        timeout(Duration::from_secs(2), child.wait())
            .await
            .expect("Job Object did not terminate its child")
            .unwrap();
    }

    #[test]
    fn orphan_recovery_is_a_noop_after_job_object_cleanup() {
        let temporary = tempfile::tempdir().unwrap();
        let leases = temporary.path().join("process-leases");
        recover_orphaned_processes(&leases).unwrap();
        assert!(leases.is_dir());
    }
}
