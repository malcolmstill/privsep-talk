use nix::libc;
use std::io::Result;
use std::os::fd::{AsRawFd, RawFd};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};

pub static CHILD_TARGET_FD: RawFd = 3;

pub fn start_proc(child_arg: &str, child_sock: UnixStream) -> Result<Child> {
    let child_sock_fd = child_sock.as_raw_fd();

    let exe = std::env::current_exe().unwrap();

    let mut cmd = Command::new(exe);

    let proc = cmd.arg(child_arg);

    unsafe {
        proc.pre_exec(move || {
            // This code runs in the child process *before* exec

            // 1. Duplicate the inherited FD to the target FD in the child
            if libc::dup2(child_sock_fd, CHILD_TARGET_FD) == -1 {
                // Use perror or write to stderr; direct println might be problematic here
                // Use a raw syscall for write if necessary, or just return Err.
                return Err(std::io::Error::last_os_error());
            }

            // 2. Optionally close the original inherited FD if different from target
            if child_sock_fd != CHILD_TARGET_FD {
                libc::close(child_sock_fd);
            }

            // 3. Remove FD_CLOEXEC from the *target* FD (CHILD_TARGET_FD)
            //    (dup2 does *not* copy the CLOEXEC flag, so the new FD (CHILD_TARGET_FD)
            //     won't have it set initially - this step might be redundant depending
            //     on exact dup2 semantics, but safer to ensure).
            let flags = libc::fcntl(CHILD_TARGET_FD, libc::F_GETFD);
            if flags == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::fcntl(CHILD_TARGET_FD, libc::F_SETFD, flags & !libc::FD_CLOEXEC) == -1 {
                return Err(std::io::Error::last_os_error());
            }

            // The child executable will now find the required data on FD 3
            Ok(())
        });
    }

    proc.spawn()
}
