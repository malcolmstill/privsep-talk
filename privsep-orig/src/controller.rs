use nix::unistd::getpid;
use privsep_channel::channel::ChannelOld;
use privsep_channel::error::ChannelError;
use std::fs::File;
use std::io::{self, Write};
use std::os::unix::io::AsRawFd;
use std::time::Duration;
use thiserror::Error;
use tokio::net::UnixStream;

#[cfg(target_os = "openbsd")]
use pledge::pledge_promises;

use crate::msg::Msg;
use crate::proc;

static NAME: &str = "controller";

pub async fn controller() -> Result<(), ControllerError> {
    #[cfg(target_os = "openbsd")]
    pledge_promises![Stdio Rpath Wpath Cpath Sendfd Proc Exec Ps].unwrap();

    let pid = getpid();
    println!("{NAME}[{pid}]: Starting...");

    // Start parser
    let (mut parser_ch, mut parser) = {
        let (parent_sock, child_sock) = UnixStream::pair()?;

        let child = proc::start("parser", parent_sock.as_raw_fd(), child_sock)?;

        let channel = ChannelOld::new(parent_sock);

        (channel, child)
    };

    // Start engine
    let (mut engine_ch, mut engine) = {
        let (parent_sock, child_sock) = UnixStream::pair()?;

        let child = proc::start("engine", parent_sock.as_raw_fd(), child_sock)?;

        let channel = ChannelOld::new(parent_sock);

        (channel, child)
    };

    #[cfg(target_os = "openbsd")]
    pledge_promises![Stdio Rpath Wpath Cpath Sendfd].unwrap();

    // Child-to-child socket
    {
        let (left, right) = UnixStream::pair()?;
        let lfd = left.as_raw_fd();
        let rfd = right.as_raw_fd();

        parser_ch.send(&Msg::FileDescriptor(lfd)).await?;
        engine_ch.send(&Msg::FileDescriptor(rfd)).await?;
    }

    // Send other fd to parser
    {
        // Create a temporary file to send.
        let file_to_send = create_temp_file("Hello from the parent via sendfd!")?;
        let fd = file_to_send.as_raw_fd(); // Get the raw FD to send

        println!("{NAME}[{pid}]: Attempting to send file descriptor: {fd}");

        // Send file descriptor
        parser_ch.send(&Msg::FileDescriptor(fd)).await?;

        println!("{NAME}[{pid}]: File descriptor {fd} sent using sendfd");
    }

    println!("{NAME}[{pid}]: Waiting...");

    let delay = tokio::time::sleep(Duration::from_millis(5_000));
    tokio::pin!(delay);

    let mut flag = false;

    #[cfg(target_os = "openbsd")]
    pledge_promises![Stdio].unwrap();

    loop {
        tokio::select! {
            _ = &mut delay => {
                if !flag {
                    parser_ch.send(&Msg::IntegerMessage(24)).await?;
                    flag = true;
                }
            }
            msg = parser_ch.recv() => {
                println!("{NAME}[{pid}]: Received from parser {msg:?}");
                msg?;
            }
            msg = engine_ch.recv() => {
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

// Helper function to create a file for demonstration purposes.
fn create_temp_file(content: &str) -> Result<File, ControllerError> {
    let mut temp_file = tempfile::tempfile()?;

    temp_file.write_all(content.as_bytes())?;
    temp_file.flush()?;

    // Seek back to the beginning so the receiver can read immediately
    io::Seek::seek(&mut temp_file, io::SeekFrom::Start(0))?;

    Ok(temp_file)
}

#[derive(Debug, Error)]
pub enum ControllerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Channel error: {0}")]
    Channel(#[from] ChannelError),
}
