use crate::{
    msg::{Channel, Msg},
    proc::CHILD_TARGET_FD,
};
use nix::unistd::getpid;
use std::{io::Result, os::fd::FromRawFd};
use tokio::net::UnixStream;

static NAME: &str = "engine";

pub async fn engine() -> Result<()> {
    let pid = getpid();
    println!("{NAME}[{}]: Starting...", pid);

    let mut ctrl_ch = Channel::new_from_fd(CHILD_TARGET_FD)?;

    let msg = ctrl_ch.recv().await.expect("Expected fd");

    let mut parser_ch = match msg {
        Msg::FileDescriptor(ch_fd) => {
            println!("{NAME}[{}]: received peer channel fd = {}", pid, ch_fd);

            let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(ch_fd) };
            stream.set_nonblocking(true)?;
            let stream = UnixStream::from_std(stream)?;

            Channel::new(stream)
        }
        _ => todo!(),
    };

    println!("{NAME}[{}]: Looping.", pid);

    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!("{NAME}[{}]: Sending message to parser", pid);

        parser_ch
            .send(&Msg::TextMessage("hello parser from engine".to_owned()))
            .await
            .unwrap();

        ctrl_ch
            .send(&Msg::TextMessage("hello control from engine".to_owned()))
            .await
            .unwrap();
    }
}
