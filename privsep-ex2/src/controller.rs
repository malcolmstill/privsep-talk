use nix::unistd::getpid;
use privsep_channel::channel_redux::Channel;
use privsep_channel::error::ChannelError;
use std::os::unix::io::AsRawFd;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{TcpListener, TcpStream, UnixStream};
use tokio::time::timeout;

#[cfg(target_os = "openbsd")]
use pledge::pledge_promises;

use crate::msg::{CtrlEngineMsg, CtrlParseMsg, EngineCtrlMsg, ParseCtrlMsg};
use crate::proc;

static NAME: &str = "controller";

pub async fn controller() -> Result<(), ControllerError> {
    #[cfg(target_os = "openbsd")]
    pledge_promises![Stdio Ps Rpath Sendfd Inet Proc Exec].unwrap();

    // New TCP connection stuff //
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    println!("Server listening on 127.0.0.1:8080");

    let mut connection: Option<BufReader<TcpStream>> = None;
    // Finish TCP connection stuff

    let pid = getpid();
    println!("{NAME}[{pid}]: Starting...");

    // Start parser
    let (mut tx_parser, mut rx_parser, mut parser) = {
        let (parent_sock, child_sock) = UnixStream::pair()?;

        let child = proc::start("parser", parent_sock.as_raw_fd(), child_sock)?;

        let (tx, rx) = Channel::from_stream::<CtrlParseMsg, ParseCtrlMsg>(parent_sock);

        (tx, rx, child)
    };

    // Start engine
    let (mut tx_engine, mut rx_engine, mut engine) = {
        let (parent_sock, child_sock) = UnixStream::pair()?;

        let child = proc::start("engine", parent_sock.as_raw_fd(), child_sock)?;

        let (tx, rx) = Channel::from_stream::<CtrlEngineMsg, EngineCtrlMsg>(parent_sock);

        (tx, rx, child)
    };

    #[cfg(target_os = "openbsd")]
    pledge_promises![Stdio Sendfd Inet].unwrap();

    // Child-to-child socket
    {
        let (left, right) = UnixStream::pair()?;
        let lfd = left.as_raw_fd();
        let rfd = right.as_raw_fd();

        tx_parser.send(&CtrlParseMsg::PeerSocket(lfd)).await?;
        tx_engine.send(&CtrlEngineMsg::PeerSocket(rfd)).await?;
    }

    println!("{NAME}[{pid}]: Waiting...");

    let delay = tokio::time::sleep(Duration::from_millis(5_000));
    tokio::pin!(delay);

    let mut flag = false;

    #[cfg(target_os = "openbsd")]
    pledge_promises![Stdio Inet].unwrap();

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
                    Ok((keep_open, maybe_data)) => {
                        if let Some(data) = maybe_data {
                            tx_parser.send(&CtrlParseMsg::Data(data)).await?;
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

            _ = &mut delay => {
                if !flag {
                    tx_parser.send(&CtrlParseMsg::Stop).await?;
                    flag = true;
                }
            }
            msg = rx_parser.recv() => {
                println!("{NAME}[{pid}]: Received from parser {msg:?}");
                msg?;
            }
            msg = rx_engine.recv() => {
                println!("{NAME}[{pid}]: Received from engine {msg:?}");
                // parser_ch.send(&Msg::IntegerMessage(22)).await.unwrap();
                msg?;
            }
            _ = parser.wait() => {
                engine.kill().await?;
                break;

            }
            _ = engine.wait() => {
                parser.kill().await?;
                break;
            }
        }
    }

    Ok(())
}

#[derive(Debug, Error)]
pub enum ControllerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Channel error: {0}")]
    Channel(#[from] ChannelError),
    #[error("Connection closed")]
    ConnectionClosed,
    #[error("Time elapsed")]
    Time(#[from] tokio::time::error::Elapsed),
}

// TCP stuff
async fn process_socket(
    reader: &mut BufReader<TcpStream>,
) -> Result<(bool, Option<String>), ControllerError> {
    let mut line = String::new();

    let n = timeout(Duration::from_secs(5), reader.read_line(&mut line)).await??;

    if n == 0 {
        return Err(ControllerError::ConnectionClosed);
    }

    let trimmed = line.trim_end();
    if trimmed.is_empty() {
        return Ok((true, None));
    }

    Ok((true, Some(trimmed.to_owned())))
}
// End TCP stuff
