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
use std::{fs::File, io::Read, os::fd::FromRawFd, time::Duration};
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    net::{TcpListener, TcpStream, UnixStream},
    time::timeout,
};

#[cfg(target_os = "openbsd")]
use pledge::pledge_promises;

static NAME: &str = "parser";

pub async fn parser() -> Result<(), ParserError> {
    #[cfg(target_os = "openbsd")]
    pledge_promises![Stdio Recvfd Inet].unwrap();

    let pid = getpid();
    println!("{NAME}[{pid}]: Starting...");

    // New TCP connection stuff //
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    println!("Server listening on 127.0.0.1:8080");

    let mut connection: Option<BufReader<TcpStream>> = None;
    // Finish TCP connection stuff

    let (mut _tx_ctrl, mut rx_ctrl) = Channel::new_from_fd::<ParseCtrlMsg, CtrlParseMsg>(SOCKFD)?;

    let (mut tx_engine, mut rx_engine) = expect_peer_channel(pid, &mut rx_ctrl).await?;

    expect_fd(pid, &mut rx_ctrl).await?;

    println!("{NAME}[{pid}]: Looping.");

    // #[cfg(target_os = "openbsd")]
    // pledge_promises![Stdio].unwrap();

    loop {
        tokio::select! {
            // Start TCP connection stuff
            accept = listener.accept(), if connection.is_none() => {
                let (socket, addr) = accept?;
                println!("Accepted {addr}");
                connection = Some(BufReader::new(socket));
            }

            result = async {
                if let Some(ref mut rdr) = connection {
                    process_socket(rdr).await
                } else {
                    Ok((false, None))  // shouldn't happen
                }
            }, if connection.is_some() => {
                match result {
                    Ok((keep_open, maybe_f)) => {
                        if let Some(f) = maybe_f {
                            tx_engine.send(&ParseEngineMsg::NewValue(f)).await?;
                        }
                        if !keep_open {
                            println!("Closing connection");
                            connection = None;
                        }
                    }
                    Err(e) => {
                        eprintln!("Socket error: {e}");
                        connection = None;
                    }
                }
            }
            // Finish TCP connection stuff

            msg = rx_engine.recv() => {
                let msg = msg?;
                println!("{NAME}[{pid}]: <- [engine]: Got message {msg:?}.");

            }
            msg = rx_ctrl.recv() => {
                let msg = msg?;
                println!("{NAME}[{pid}]: <- [controller]: Got message {msg:?}.");

                match msg {
                    CtrlParseMsg::PeerSocket(_) => {},
                    CtrlParseMsg::Connection(_) => {},
                    CtrlParseMsg::Stop => {},
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
    #[error("Connection closed")]
    ConnectionClosed,
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

async fn expect_fd(pid: Pid, rx: &mut ChannelRx<CtrlParseMsg>) -> Result<(), ParserError> {
    // Receive the file descriptor from the parent using sendfd::recv_fd
    println!("{NAME}[{pid}]: Waiting to receive file descriptor from parent...",);

    let CtrlParseMsg::Connection(temp_fd) = rx.recv().await? else {
        panic!("expected peer socket");
    };

    println!("{NAME}[{pid}]: received fd = {temp_fd}");

    let mut tmp_file_fd = unsafe { File::from_raw_fd(temp_fd) };

    let mut out = String::new();
    tmp_file_fd.read_to_string(&mut out)?;

    println!("{NAME}[{pid}]: read temp file: {out}");

    Ok(())
}

// TCP stuff
async fn process_socket(
    reader: &mut BufReader<TcpStream>,
) -> Result<(bool, Option<f64>), ParserError> {
    let mut line = String::new();

    let n = timeout(Duration::from_secs(5), reader.read_line(&mut line)).await??;

    if n == 0 {
        return Err(ParserError::ConnectionClosed);
    }

    let trimmed = line.trim_end();
    if trimmed.is_empty() {
        return Ok((true, None));
    }

    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let v = eval_rpn(&tokens)?;

    Ok((true, Some(v)))
}
// End TCP stuff
