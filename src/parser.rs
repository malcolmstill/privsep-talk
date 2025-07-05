use crate::{
    msg::{Channel, Msg},
    proc::CHILD_TARGET_FD,
};
use nix::unistd::getpid;
use std::{
    fs::File,
    io::{Read, Result},
    os::fd::FromRawFd,
};
use tokio::net::UnixStream;

#[cfg(target_os = "openbsd")]
use pledge::pledge_promises;
#[cfg(target_os = "openbsd")]
use unveil::unveil;

static NAME: &str = "parser";

pub async fn parser() -> Result<()> {
    let pid = getpid();
    println!("{NAME}[{}]: Starting...", pid);

    let mut ctrl_ch = Channel::new_from_fd(CHILD_TARGET_FD)?;

    println!("{NAME}[{}]: Waiting on peer channel...", pid);

    let msg = ctrl_ch.recv().await.expect("Expected fd");

    println!("{NAME}[{}]: Peer channel received", pid);

    let mut engine_ch = match msg {
        Msg::FileDescriptor(ch_fd) => {
            println!("{NAME}[{}]: received peer channel fd = {}", pid, ch_fd);

            let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(ch_fd) };
            stream.set_nonblocking(true)?;
            let stream = UnixStream::from_std(stream)?;

            Channel::new(stream)
        }
        _ => todo!(),
    };

    // Receive the file descriptor from the parent using sendfd::recv_fd
    println!(
        "{NAME}[{}]: Waiting to receive file descriptor from parent...",
        pid
    );

    let msg = ctrl_ch.recv().await.expect("Expected fd");

    match msg {
        Msg::TextMessage(_) | Msg::IntegerMessage(_) => todo!(),
        Msg::FileDescriptor(temp_fd) => {
            println!("{NAME}[{}]: received fd = {}", pid, temp_fd);

            let mut tmp_file_fd = unsafe { File::from_raw_fd(temp_fd) };

            let mut out = String::new();
            tmp_file_fd.read_to_string(&mut out)?;

            println!("{NAME}[{}]: read temp file: {}", pid, out);
        }
    }

    println!("{NAME}[{}]: Looping.", pid);

    #[cfg(target_os = "openbsd")]
    pledge_promises![Stdio].unwrap();

    loop {
        tokio::select! {
            msg = engine_ch.recv() => {
                let msg = msg.unwrap();
                println!("{NAME}[{}]: <- [engine]: Got message {:?}.", pid, msg);

            }
            msg = ctrl_ch.recv() => {
                let msg = msg.unwrap();
                println!("{NAME}[{}]: <- [controller]: Got message {:?}.", pid, msg);

                match msg {
                    Msg::TextMessage(_) => {},
                    Msg::IntegerMessage(int) => {
                        println!("{NAME}[{}]: GOT INTEGER {:?}.", pid, int);

                        #[cfg(target_os = "openbsd")]
                        unveil("", "").unwrap();
                    },
                    Msg::FileDescriptor(_) => {},
                }
            }
        }
    }
}
