#!/usr/bin/env python3
"""A per-user native GUI lease for POSIX launchers; no background service.

flock survives exec and is released by the kernel after the last inherited
descriptor closes, including cancelled launchers. The persistent empty/receipt
file is not itself the lock, so crashed runs never need stale-file deletion.
"""

from __future__ import annotations

import ctypes
import fcntl
import json
import os
from pathlib import Path
import secrets
import select
import shutil
import signal
import stat
import subprocess
import sys

# Wide enough for the layout suite's 2560px window.
HEADLESS_SCREEN = ("-screen", "0", "2560x1600x24")
PR_SET_PDEATHSIG = 1


def needs_headless_display() -> bool:
    return sys.platform.startswith("linux") and not (
        os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY")
    )


def _exit_with_parent() -> None:
    ctypes.CDLL(None, use_errno=True).prctl(PR_SET_PDEATHSIG, signal.SIGTERM)


def start_headless_display(state_dir: Path) -> None:
    """Start a private Xvfb that exits with the launcher and export its DISPLAY.

    Xvfb is a child, not a wrapper, so the launcher keeps this pid across exec
    and session receipts and process-tree stops still name the real launcher.
    """
    xvfb, xauth = shutil.which("Xvfb"), shutil.which("xauth")
    if not xvfb or not xauth:
        raise RuntimeError(
            "No display is set and Xvfb or xauth is missing. Install the xvfb "
            "package (docs/dev/checks.md#linux-cloud-development) or set DISPLAY."
        )
    cookie = secrets.token_hex(16)
    for number in range(99, 199):
        if Path(f"/tmp/.X{number}-lock").exists():
            continue
        display = f":{number}"
        authority = state_dir / f"xvfb-{number}.auth"
        authority.unlink(missing_ok=True)
        subprocess.run(
            [xauth, "-q", "-f", str(authority), "add", display, ".", cookie],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        ready_read, ready_write = os.pipe()
        server = subprocess.Popen(
            [
                xvfb,
                display,
                *HEADLESS_SCREEN,
                "-nolisten",
                "tcp",
                "-auth",
                str(authority),
                "-displayfd",
                str(ready_write),
            ],
            pass_fds=(ready_write,),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            preexec_fn=_exit_with_parent,
        )
        os.close(ready_write)
        readable, _, _ = select.select([ready_read], [], [], 15)
        announced = os.read(ready_read, 16).strip() if readable else b""
        os.close(ready_read)
        if announced == str(number).encode():
            os.environ["DISPLAY"] = display
            os.environ["XAUTHORITY"] = str(authority)
            return
        server.kill()
        server.wait()
    raise RuntimeError("Xvfb did not start on displays :99 to :198")


def run(command: list[str], lock_path: Path | None = None) -> None:
    if not command:
        raise ValueError("usage: native_ui_lease.py <command> [arguments...]")
    lock_path = lock_path or Path.home() / ".cache/loupist/native-ui.lock"
    lock_path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    parent = lock_path.parent.lstat()
    if not stat.S_ISDIR(parent.st_mode) or parent.st_uid != os.getuid():
        raise ValueError(
            "Native GUI lease directory must be a real user-owned directory"
        )

    fd = None
    # A suite passes the same open file description to each app restart. This
    # reuses its lock rather than deadlocking against itself.
    try:
        inherited = int(os.environ.get("LOUPIST_GUI_LEASE_FD", ""))
        opened = os.fstat(inherited)
        current = lock_path.lstat()
        if (opened.st_dev, opened.st_ino) == (current.st_dev, current.st_ino):
            fd = inherited
    except (ValueError, OSError):
        pass
    inherited_lease = fd is not None
    if fd is None:
        fd = os.open(lock_path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    opened = os.fstat(fd)
    if not stat.S_ISREG(opened.st_mode) or opened.st_uid != os.getuid():
        raise ValueError("Native GUI lease must be a regular user-owned file")
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        owner = os.pread(fd, 512, 0).decode("utf-8", errors="replace").strip()
        raise RuntimeError(
            f"Another Loupist native GUI run owns the display ({owner or 'starting'}). "
            "Arrange a handoff; no app was launched or stopped."
        ) from None

    os.fchmod(fd, 0o600)
    if not inherited_lease:
        os.ftruncate(fd, 0)
        os.pwrite(
            fd,
            json.dumps({"pid": os.getpid(), "command": Path(command[0]).name}).encode(),
            0,
        )
    os.set_inheritable(fd, True)
    os.environ["LOUPIST_GUI_LEASE_FD"] = str(fd)
    os.environ["LOUPIST_GUI_LEASE_ENTRY"] = f"{os.getpid()}:{command[0]}"
    if needs_headless_display():
        start_headless_display(lock_path.parent)
    os.execvpe(command[0], command, os.environ)


if __name__ == "__main__":
    try:
        run(sys.argv[1:])
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
