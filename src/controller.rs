use nix::unistd::getpid;
use std::fs::File;
use std::io::{self, Result, Write};
use std::os::unix::io::AsRawFd;
use std::time::Duration;
use tokio::net::UnixStream;

use crate::msg::{Channel, Msg};
use crate::proc;

static NAME: &str = "controller";

pub async fn controller() -> Result<()> {
    let pid = getpid();
    println!("{NAME}[{}]: Starting...", pid);

    // Child-to-child stream
    let (peer_1_sock, peer_2_sock) = UnixStream::pair()?;

    // 1. Create socket pair for child 1
    // 2. Start child 1
    let (mut parser_ch, mut parser) = {
        let (parent_socket, child_socket) = UnixStream::pair()?;

        let child = proc::start_proc("parser", child_socket)?;

        let mut channel = Channel::new(parent_socket);

        // Send peer-to-peer channel
        channel
            .send(&Msg::FileDescriptor(peer_1_sock.as_raw_fd()))
            .await
            .expect("failed to send");

        // Create a temporary file to send.
        let file_to_send = create_temp_file("Hello from the parent via sendfd!")?;
        let fd_to_send = file_to_send.as_raw_fd(); // Get the raw FD to send

        println!(
            "{NAME}[{}]: Attempting to send file descriptor: {}",
            pid, fd_to_send
        );

        // Send file descriptor
        channel
            .send(&Msg::FileDescriptor(fd_to_send))
            .await
            .expect("failed to send");

        println!(
            "{NAME}[{}]: File descriptor {} sent using sendfd (over our custom channel).",
            pid, fd_to_send
        );

        (channel, child)
    };

    let (mut engine_ch, mut engine) = {
        let (parent_socket, child_socket) = UnixStream::pair()?;

        let child = proc::start_proc("engine", child_socket)?;

        let mut channel = Channel::new(parent_socket);

        // Send peer-to-peer channel
        channel
            .send(&Msg::FileDescriptor(peer_2_sock.as_raw_fd()))
            .await
            .expect("failed to send");

        (channel, child)
    };

    println!("{NAME}[{}]: Waiting...", pid);

    let delay = tokio::time::sleep(Duration::from_millis(5_000));
    tokio::pin!(delay);

    let mut flag = false;

    loop {
        tokio::select! {
            _ = &mut delay => {
                if !flag {
                    parser_ch.send(&Msg::IntegerMessage(24)).await.unwrap();
                    flag = true;
                }
            }
            msg = parser_ch.recv() => {
                println!("{NAME}[{}]: Received from parser {:?}", pid, msg);
            }
            msg = engine_ch.recv() => {
                println!("{NAME}[{}]: Received from engine {:?}", pid, msg);
                // parser_ch.send(&Msg::IntegerMessage(22)).await.unwrap();
            }
            _ = parser.wait() => {
                println!("parser down");
                engine.kill().await.unwrap();
                break;

            }
            _ = engine.wait() => {
                println!("engine down");
                parser.kill().await.unwrap();
                break;
            }
        }
    }

    Ok(())
}

// Helper function to create a file for demonstration purposes.
fn create_temp_file(content: &str) -> Result<File> {
    let mut temp_file = tempfile::tempfile()?;

    temp_file.write_all(content.as_bytes())?;
    temp_file.flush()?;

    // Seek back to the beginning so the receiver can read immediately
    io::Seek::seek(&mut temp_file, io::SeekFrom::Start(0))?;

    Ok(temp_file)
}
