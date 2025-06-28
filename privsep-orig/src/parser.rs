use crate::{msg::Msg, proc::SOCKFD};
use nix::unistd::getpid;
use privsep_channel::{channel::ChannelOld, error::ChannelError};
use std::{fs::File, io::Read, os::fd::FromRawFd};
use thiserror::Error;
use tokio::net::UnixStream;

#[cfg(target_os = "openbsd")]
use pledge::pledge_promises;
#[cfg(target_os = "openbsd")]
use unveil::unveil;

static NAME: &str = "parser";

pub async fn parser() -> Result<(), ParserError> {
    let pid = getpid();
    println!("{NAME}[{pid}]: Starting...");

    let mut ctrl_ch = ChannelOld::<Msg>::new_from_fd(SOCKFD)?;

    println!("{NAME}[{pid}]: Waiting on peer channel...");

    let msg = ctrl_ch.recv().await?;

    println!("{NAME}[{pid}]: Peer channel received");

    let mut engine_ch: ChannelOld<Msg> = match msg {
        Msg::FileDescriptor(ch_fd) => {
            println!("{NAME}[{pid}]: received peer channel fd = {ch_fd}");

            let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(ch_fd) };
            stream.set_nonblocking(true)?;
            let stream = UnixStream::from_std(stream)?;

            ChannelOld::new(stream)
        }
        _ => return Err(ParserError::UnexpectedMessage(msg)),
    };

    // Receive the file descriptor from the parent using sendfd::recv_fd
    println!("{NAME}[{pid}]: Waiting to receive file descriptor from parent...",);

    let msg = ctrl_ch.recv().await.expect("Expected fd");

    match msg {
        Msg::TextMessage(_) | Msg::IntegerMessage(_) => todo!(),
        Msg::FileDescriptor(temp_fd) => {
            println!("{NAME}[{pid}]: received fd = {temp_fd}");

            let mut tmp_file_fd = unsafe { File::from_raw_fd(temp_fd) };

            let mut out = String::new();
            tmp_file_fd.read_to_string(&mut out)?;

            println!("{NAME}[{pid}]: read temp file: {out}");
        }
    }

    println!("{NAME}[{pid}]: Looping.");

    #[cfg(target_os = "openbsd")]
    pledge_promises![Stdio].unwrap();

    loop {
        tokio::select! {
            msg = engine_ch.recv() => {
                let msg = msg?;
                println!("{NAME}[{pid}]: <- [engine]: Got message {msg:?}.");

            }
            msg = ctrl_ch.recv() => {
                let msg = msg?;
                println!("{NAME}[{pid}]: <- [controller]: Got message {msg:?}.");

                match msg {
                    Msg::TextMessage(_) => {},
                    Msg::IntegerMessage(int) => {
                        println!("{NAME}[{pid}]: GOT INTEGER {int:?}.");

                        #[cfg(target_os = "openbsd")]
                        unveil("", "").unwrap();
                    },
                    Msg::FileDescriptor(_) => {},
                }
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum ParserError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Channel error: {0}")]
    Channel(#[from] ChannelError),
    #[error("Unexpected message: {0}")]
    UnexpectedMessage(Msg),
}
