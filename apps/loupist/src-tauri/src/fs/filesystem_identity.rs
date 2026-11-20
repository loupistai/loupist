//! Capture identity and creation time from the same retained file descriptor.
use crate::domain::{FilesystemBirthTime, RootFilesystemIdentity};
use std::{io, os::fd::AsFd};

pub(crate) fn of_file(file: &impl AsFd) -> io::Result<RootFilesystemIdentity> {
    let metadata = rustix::fs::fstat(file)?;
    #[cfg(target_os = "macos")]
    let birth_time = Some(FilesystemBirthTime {
        seconds: metadata.st_birthtime,
        nanoseconds: metadata.st_birthtime_nsec as u32,
    });
    #[cfg(target_os = "linux")]
    let birth_time = {
        use rustix::fs::{statx, AtFlags, StatxFlags};
        statx(file, "", AtFlags::EMPTY_PATH, StatxFlags::BTIME)
            .ok()
            .and_then(|stat| {
                (stat.stx_mask & StatxFlags::BTIME.bits() != 0).then_some(FilesystemBirthTime {
                    seconds: stat.stx_btime.tv_sec,
                    nanoseconds: stat.stx_btime.tv_nsec,
                })
            })
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let birth_time = None;
    #[cfg(test)]
    let birth_time = BIRTH_OVERRIDE.with(|value| {
        value
            .borrow()
            .iter()
            .rev()
            .find(|(inode, _)| *inode == metadata.st_ino as u64)
            .map_or(birth_time, |(_, birth)| *birth)
    });
    // Some filesystems set BTIME but return epoch or a negative sentinel.
    // Normalize those responses identically on every platform before binding.
    let birth_time = birth_time.filter(|birth| birth.seconds > 0);
    Ok(RootFilesystemIdentity {
        device: metadata.st_dev as u64,
        inode: metadata.st_ino as u64,
        birth_time,
    })
}

#[cfg(test)]
thread_local! {
    static BIRTH_OVERRIDE: std::cell::RefCell<Vec<(u64, Option<FilesystemBirthTime>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Scope a missing or changed filesystem birth-time response to one test inode.
#[cfg(test)]
pub(crate) fn with_birth_time<T>(
    inode: u64,
    birth: Option<FilesystemBirthTime>,
    body: impl FnOnce() -> T,
) -> T {
    struct Reset(usize);
    impl Drop for Reset {
        fn drop(&mut self) {
            BIRTH_OVERRIDE.with(|value| value.borrow_mut().truncate(self.0));
        }
    }
    let _reset = Reset(BIRTH_OVERRIDE.with(|value| {
        let mut values = value.borrow_mut();
        let length = values.len();
        values.push((inode, birth));
        length
    }));
    body()
}
