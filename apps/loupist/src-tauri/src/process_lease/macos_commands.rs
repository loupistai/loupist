//! One-shot macOS utilities are owned until they exit. Applications opened
//! through LaunchServices belong to the system and are outside these groups.

use super::{process_group_has_exited, OwnedProcessGroup};
use std::{
    io::{self, Read},
    os::unix::process::CommandExt,
    process::{Child, Command, ExitStatus, Output, Stdio},
    time::Duration,
};

pub(crate) fn status(command: &mut Command) -> io::Result<ExitStatus> {
    let (child, group) = OwnedProcessGroup::spawn_std(command.process_group(0))?;
    reap(child, group)
}

pub(crate) fn output(command: &mut Command) -> io::Result<Output> {
    let (mut child, group) = OwnedProcessGroup::spawn_std(
        command
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let (stdout, stderr) = std::thread::scope(|scope| {
        let stderr = scope.spawn(move || {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).map(|_| bytes)
        });
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes)?;
        Ok::<_, io::Error>((bytes, stderr.join().expect("stderr reader")?))
    })?;
    Ok(Output {
        status: reap(child, group)?,
        stdout,
        stderr,
    })
}

pub(crate) fn spawn(command: &mut Command) -> io::Result<()> {
    let (child, group) = OwnedProcessGroup::spawn_std(command.process_group(0))?;
    std::thread::Builder::new().spawn(move || {
        let _ = reap(child, group);
    })?;
    Ok(())
}

fn reap(mut child: Child, group: OwnedProcessGroup) -> io::Result<ExitStatus> {
    while !process_group_has_exited(child.id() as i32).map_err(io::Error::other)? {
        std::thread::sleep(Duration::from_millis(5));
    }
    group.disarm();
    child.wait()
}
