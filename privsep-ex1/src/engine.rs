use crate::{
    msg::{CtrlEngineMsg, EngineCtrlMsg, EngineParseMsg, ParseEngineMsg},
    proc::SOCKFD,
};
use nix::unistd::{getpid, Pid};
use privsep_channel::{
    channel_redux::{Channel, ChannelRx, ChannelTx},
    error::ChannelError,
};
use std::os::fd::FromRawFd;
use thiserror::Error;
use tokio::net::UnixStream;

static NAME: &str = "engine";

pub async fn engine() -> Result<(), EngineError> {
    let pid = getpid();
    println!("{NAME}[{pid}]: Starting...");

    let (mut tx_ctrl, mut rx_ctrl) = Channel::new_from_fd(SOCKFD)?;
    let (mut tx_parser, mut rx_parser) = expect_peer_channel(pid, &mut rx_ctrl).await?;

    println!("{NAME}[{pid}]: Looping.");

    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!("{NAME}[{pid}]: Sending message to parser");

        tx_parser.send(&EngineParseMsg::Failed).await?;
        tx_ctrl.send(&EngineCtrlMsg::Bar).await?;

        tokio::select! {
            msg = rx_parser.recv() => {
                let msg = msg?;
                println!("{NAME}[{pid}]: Received message from parser");

                match msg {
                    ParseEngineMsg::NewValue(f) => {
                        tokio::fs::write("latest-value", format!("Latest value = {f}\n")).await?;
                    },
                }
            }
        }
    }
}

async fn expect_peer_channel(
    pid: Pid,
    rx: &mut ChannelRx<CtrlEngineMsg>,
) -> Result<(ChannelTx<EngineParseMsg>, ChannelRx<ParseEngineMsg>), EngineError> {
    let CtrlEngineMsg::PeerSocket(ch_fd) = rx.recv().await? else {
        panic!("expected peer socket");
    };

    println!("{NAME}[{pid}]: received peer channel fd = {ch_fd}");

    let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(ch_fd) };
    stream.set_nonblocking(true)?;
    let stream = UnixStream::from_std(stream)?;

    let ch = Channel::from_stream(stream);

    Ok(ch)
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Channel error: {0}")]
    Channel(#[from] ChannelError),
}
