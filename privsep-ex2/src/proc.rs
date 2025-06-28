use nix::libc;
use std::io::Result;
use std::os::fd::{AsRawFd, RawFd};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};

pub static SOCKFD: RawFd = 56;

pub fn start(subsystem: &str, parent_sock_fd: i32, child_sock: UnixStream) -> Result<Child> {
    let child_sock_fd = child_sock.as_raw_fd();

    let exe = std::env::current_exe().unwrap();
    let mut cmd = Command::new(exe);
    let proc = cmd.arg(subsystem);

    unsafe {
        proc.pre_exec(move || {
            libc::close(parent_sock_fd);

            if libc::dup2(child_sock_fd, SOCKFD) == -1 {
                return Err(std::io::Error::last_os_error());
            }

            if child_sock_fd != SOCKFD {
                libc::close(child_sock_fd);
            }

            Ok(())
        });
    }

    proc.spawn()
}
