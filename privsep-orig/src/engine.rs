use crate::{msg::Msg, proc::SOCKFD};
use nix::unistd::getpid;
use privsep_channel::{channel::ChannelOld, error::ChannelError};
use std::os::fd::FromRawFd;
use thiserror::Error;
use tokio::net::UnixStream;

static NAME: &str = "engine";

pub async fn engine() -> Result<(), EngineError> {
    let pid = getpid();
    println!("{NAME}[{pid}]: Starting...");

    let mut ctrl_ch = ChannelOld::new_from_fd(SOCKFD)?;

    let msg = ctrl_ch.recv().await.expect("Expected fd");

    let mut parser_ch = match msg {
        Msg::FileDescriptor(ch_fd) => {
            println!("{NAME}[{pid}]: received peer channel fd = {ch_fd}");

            let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(ch_fd) };
            stream.set_nonblocking(true)?;
            let stream = UnixStream::from_std(stream)?;

            ChannelOld::new(stream)
        }
        _ => todo!(),
    };

    println!("{NAME}[{pid}]: Looping.");

    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!("{NAME}[{pid}]: Sending message to parser");

        parser_ch
            .send(&Msg::TextMessage("hello parser from engine".to_owned()))
            .await?;

        ctrl_ch
            .send(&Msg::TextMessage("hello control from engine".to_owned()))
            .await?;
    }
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Channel error: {0}")]
    Channel(#[from] ChannelError),
}
