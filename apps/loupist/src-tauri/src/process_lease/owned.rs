//! In-process ownership covers the gap before a durable lease is written and
//! short-lived workers that do not need crash recovery.

use super::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock, Mutex, MutexGuard},
};

pub(crate) static HOST_PROCESS_GROUPS: LazyLock<Arc<ProcessGroups>> = LazyLock::new(Arc::default);

#[derive(Default)]
pub(crate) struct ProcessGroups(Mutex<Groups>);

#[derive(Default)]
struct Groups {
    closed: bool,
    next_id: u64,
    entries: BTreeMap<u64, Entry>,
}

struct Entry {
    pid: i32,
    state: GroupOwnership,
    lease: Option<ChildProcessLease>,
}

#[derive(PartialEq, Eq)]
enum GroupOwnership {
    // An armed owner still reserves the leader PID.
    Armed,
    // SIGKILL was sent before reaping. Only observe the group now.
    Killed,
    Exited,
}

pub(crate) struct OwnedProcessGroup {
    groups: Arc<ProcessGroups>,
    id: u64,
    pid: i32,
}

impl ProcessGroups {
    fn lock(&self) -> MutexGuard<'_, Groups> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }

    // Holding the same lock as shutdown makes the first group signal safe
    // even if exit races the exact interval between spawn and registration.
    pub(crate) fn spawn<T>(
        self: &Arc<Self>,
        spawn: impl FnOnce() -> io::Result<(T, u32)>,
    ) -> io::Result<(T, OwnedProcessGroup)> {
        let mut groups = self.lock();
        if groups.closed {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Loupist is closing",
            ));
        }
        let (child, pid) = spawn()?;
        let pid = pid as i32;
        let id = groups.next_id;
        groups.next_id += 1;
        groups.entries.insert(
            id,
            Entry {
                pid,
                state: GroupOwnership::Armed,
                lease: None,
            },
        );
        Ok((
            child,
            OwnedProcessGroup {
                groups: self.clone(),
                id,
                pid,
            },
        ))
    }

    /// Close spawn and lease admission, signal every remaining owned group
    /// before waiting for any of them, and release their leases by one deadline.
    pub(crate) fn finish_shutdown(&self, deadline: Instant, allow_polite_stop: bool) {
        let mut groups = self.lock();
        groups.closed = true;
        if allow_polite_stop {
            groups
                .entries
                .retain(|_, entry| !entry.complete_if_exited());
            for entry in groups
                .entries
                .values()
                .filter(|entry| entry.state == GroupOwnership::Armed)
            {
                let _ = signal_process_group(entry.pid, libc::SIGTERM);
            }
            let polite_deadline = deadline.min(Instant::now() + RECOVERY_TERM_GRACE);
            if groups.wait_for_exit(polite_deadline) {
                return;
            }
        }
        let mut signaled = 0;
        for entry in groups.entries.values_mut() {
            if entry.state != GroupOwnership::Armed {
                continue;
            }
            if matches!(process_group_has_exited(entry.pid), Ok(true)) {
                entry.state = GroupOwnership::Exited;
            } else if signal_process_group(entry.pid, libc::SIGKILL).is_ok() {
                entry.state = GroupOwnership::Killed;
                signaled += 1;
            }
        }
        if signaled != 0 {
            eprintln!("Loupist shutdown sent SIGKILL to {signaled} process groups");
        }
        if !groups.wait_for_exit(deadline) {
            eprintln!(
                "Loupist shutdown could not confirm cleanup of {} process groups",
                groups.entries.len()
            );
        }
    }
}

impl Groups {
    fn wait_for_exit(&mut self, deadline: Instant) -> bool {
        loop {
            self.entries.retain(|_, entry| !entry.complete_if_exited());
            if self.entries.is_empty() {
                return true;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            std::thread::sleep(remaining.min(POLL_INTERVAL));
        }
    }
}

impl Entry {
    fn complete_if_exited(&self) -> bool {
        (self.state == GroupOwnership::Exited
            || matches!(process_group_has_exited(self.pid), Ok(true)))
            && self.remove_lease().is_ok()
    }

    fn remove_lease(&self) -> Result<(), ProcessLeaseError> {
        if let Some(lease) = &self.lease {
            remove_record(&lease.directory, &lease.file_name)?;
        }
        Ok(())
    }
}

impl OwnedProcessGroup {
    pub(crate) fn spawn(
        command: &mut tokio::process::Command,
    ) -> io::Result<(tokio::process::Child, Self)> {
        HOST_PROCESS_GROUPS.spawn(|| {
            let child = command.spawn()?;
            let pid = child.id().expect("a new child has not been reaped");
            Ok((child, pid))
        })
    }

    pub(crate) fn spawn_std(
        command: &mut std::process::Command,
    ) -> io::Result<(std::process::Child, Self)> {
        HOST_PROCESS_GROUPS.spawn(|| {
            let child = command.spawn()?;
            let pid = child.id();
            Ok((child, pid))
        })
    }

    pub(crate) fn register_lease(
        &self,
        dir: &Path,
        kind: ProcessKind,
        executable: &Path,
    ) -> Result<(), ProcessLeaseError> {
        let mut groups = self.groups.lock();
        if groups.closed {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Loupist is closing").into());
        }
        if let Some(entry) = groups.entries.get_mut(&self.id) {
            entry.lease = Some(ChildProcessLease::register(
                dir, kind, self.pid, self.pid, executable,
            )?);
        }
        Ok(())
    }

    pub(crate) fn kill(&self) -> Result<(), ProcessLeaseError> {
        let mut groups = self.groups.lock();
        if let Some(entry) = groups.entries.get_mut(&self.id) {
            if entry.state == GroupOwnership::Armed {
                signal_process_group(self.pid, libc::SIGKILL)?;
                entry.state = GroupOwnership::Killed;
            }
        }
        Ok(())
    }

    /// The owner already confirmed the entire group exited. Disarm before
    /// reaping the leader, even if removing its durable lease later fails.
    pub(crate) fn disarm(&self) {
        let mut groups = self.groups.lock();
        if let Some(entry) = groups.entries.get_mut(&self.id) {
            entry.state = GroupOwnership::Exited;
        }
    }

    pub(crate) fn complete(&self) -> Result<(), ProcessLeaseError> {
        let mut groups = self.groups.lock();
        if let Some(entry) = groups.entries.get(&self.id) {
            if entry.state != GroupOwnership::Exited {
                return Err(ProcessLeaseError::StopTimeout);
            }
            entry.remove_lease()?;
            groups.entries.remove(&self.id);
        }
        Ok(())
    }
}

impl Drop for OwnedProcessGroup {
    fn drop(&mut self) {
        {
            let mut groups = self.groups.lock();
            let Some(entry) = groups.entries.get_mut(&self.id) else {
                return;
            };
            if entry.state == GroupOwnership::Armed {
                let _ = signal_process_group(self.pid, libc::SIGKILL);
                entry.state = GroupOwnership::Killed;
            }
            if entry.complete_if_exited() {
                groups.entries.remove(&self.id);
                return;
            }
        }
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let (groups, id, pid) = (self.groups.clone(), self.id, self.pid);
            runtime.spawn(async move {
                if confirm_process_group_exit(pid, RECOVERY_KILL_GRACE)
                    .await
                    .is_confirmed()
                {
                    let mut groups = groups.lock();
                    if groups
                        .entries
                        .get(&id)
                        .is_some_and(|entry| entry.remove_lease().is_ok())
                    {
                        groups.entries.remove(&id);
                    }
                }
            });
        }
    }
}
