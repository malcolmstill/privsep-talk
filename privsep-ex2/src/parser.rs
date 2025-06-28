use crate::{
    msg::{CtrlParseMsg, EngineParseMsg, ParseCtrlMsg, ParseEngineMsg},
    proc::SOCKFD,
};
use nix::unistd::{getpid, Pid};
use privsep_channel::{
    channel_redux::{Channel, ChannelRx, ChannelTx},
    error::ChannelError,
};
use privsep_rpn::rpn::{eval_rpn, RpnError};
use std::os::fd::FromRawFd;
use thiserror::Error;
use tokio::net::UnixStream;

#[cfg(target_os = "openbsd")]
use pledge::pledge_promises;

static NAME: &str = "parser";

pub async fn parser() -> Result<(), ParserError> {
    #[cfg(target_os = "openbsd")]
    pledge_promises![Stdio Recvfd].unwrap();

    let pid = getpid();
    println!("{NAME}[{pid}]: Starting...");

    let (mut _tx_ctrl, mut rx_ctrl) = Channel::new_from_fd::<ParseCtrlMsg, CtrlParseMsg>(SOCKFD)?;
    let (mut tx_engine, mut rx_engine) = expect_peer_channel(pid, &mut rx_ctrl).await?;

    println!("{NAME}[{pid}]: Looping.");

    loop {
        tokio::select! {
            msg = rx_engine.recv() => {
                let msg = msg?;
                println!("{NAME}[{pid}]: <- [engine]: Got message {msg:?}.");

            }
            msg = rx_ctrl.recv() => {
                match msg? {
                    CtrlParseMsg::Data(data) => {
                        match parse_evaluate_rpn(&data)  {
                            Ok(value) => tx_engine.send(&ParseEngineMsg::NewValue(value)).await?,
                            Err(e) => println!("{NAME}[{pid}]: Bad input: {e:?}"),
                        }
                    },
                    _ => println!("{NAME}[{pid}]: unexpected message"),
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
    #[error("Time elapsed")]
    Time(#[from] tokio::time::error::Elapsed),
    #[error("RPN")]
    Rpn(#[from] RpnError),
}

async fn expect_peer_channel(
    pid: Pid,
    rx: &mut ChannelRx<CtrlParseMsg>,
) -> Result<(ChannelTx<ParseEngineMsg>, ChannelRx<EngineParseMsg>), ParserError> {
    println!("{NAME}[{pid}]: Waiting on peer channel...");

    let CtrlParseMsg::PeerSocket(ch_fd) = rx.recv().await? else {
        panic!("expected peer socket");
    };

    println!("{NAME}[{pid}]: received peer channel fd = {ch_fd}");

    let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(ch_fd) };
    stream.set_nonblocking(true)?;
    let stream = UnixStream::from_std(stream)?;

    let ch = Channel::from_stream(stream);

    println!("{NAME}[{pid}]: Peer channel received");

    Ok(ch)
}

fn parse_evaluate_rpn(data: &str) -> Result<f64, ParserError> {
    let tokens: Vec<&str> = data.split_whitespace().collect();

    let value = eval_rpn(&tokens)?;

    Ok(value)
}
