use bincode::{deserialize, serialize};
use byteorder::{BigEndian, WriteBytesExt};
use nix::libc;
use sendfd::{RecvWithFd, SendWithFd};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{self, ErrorKind};
use std::os::fd::FromRawFd;
use std::os::unix::io::RawFd;
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum Msg {
    TextMessage(String),
    IntegerMessage(i64),
    // Do not serialise the file descriptor...this is will be sent via ancilliary data
    FileDescriptor(#[serde(skip)] RawFd),
}

#[derive(Error, Debug)]
pub enum ChannelError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Bincode serialization/deserialization error: {0}")]
    Bincode(#[from] bincode::Error),
    // #[error("SendFd error: {0}")]
    // SendFd(#[from] io::Error),
    // #[error("RecvFd error: {0}")]
    // RecvFd(#[from] io::Error),
    #[error("Serialized message size ({0} bytes) exceeds fixed buffer capacity ({1} bytes)")]
    MessageTooLargeForTxBuffer(usize, usize),
    #[error("Received message length ({0} bytes) exceeds fixed buffer capacity ({1} bytes)")]
    MessageTooLargeForRxBuffer(usize, usize),
    #[error("Connection closed prematurely (EOF) while reading")]
    ConnectionClosedPrematurely,
    #[error(
        "Received FileDescriptor message but no FD was available in the ancillary data buffer"
    )]
    MissingFdForMessage,
}

type Result<T> = std::result::Result<T, ChannelError>;

pub struct Channel {
    stream: UnixStream,
    // Buffer for file descriptors received via ancillary data but not yet
    // matched to a FileDescriptor message. Using VecDeque for FIFO behavior.
    received_fds: VecDeque<RawFd>,
    tx_buffer: Box<[u8]>,
    rx_buffer: Box<[u8]>,
    // Stores the number of valid bytes already present at the start of rx_buffer
    // from a previous read operation.
    rx_buffer_offset: usize,
}

// Define fixed buffer sizes. TX needs space for prefix + data.
const TX_BUFFER_SIZE: usize = 4096;
const RX_BUFFER_SIZE: usize = 4096;
// Max payload size must fit within buffer minus prefix length
const MAX_PAYLOAD_SIZE: usize = TX_BUFFER_SIZE - 4;

impl Channel {
    pub fn new(stream: UnixStream) -> Self {
        // Set non-blocking for recv_fd, although we handle blocking reads overall
        // stream.set_nonblocking(true).expect("Failed to set non-blocking");
        Channel {
            stream,
            received_fds: VecDeque::new(),
            tx_buffer: vec![0u8; TX_BUFFER_SIZE].into_boxed_slice(),
            rx_buffer: vec![0u8; RX_BUFFER_SIZE].into_boxed_slice(),
            rx_buffer_offset: 0,
        }
    }

    pub fn new_from_fd(fd: RawFd) -> io::Result<Self> {
        let stream = make_stream(fd)?;

        Ok(Channel::new(stream))
    }

    pub async fn send(&mut self, msg: &Msg) -> Result<()> {
        loop {
            self.stream.writable().await?;

            match self.send_msg(msg).await {
                Err(ChannelError::Io(ref e)) if e.kind() == ErrorKind::WouldBlock => {
                    continue;
                }
                r => return r,
            }
        }
    }

    pub async fn recv(&mut self) -> Result<Msg> {
        loop {
            self.stream.readable().await?;

            match self.recv_msg().await {
                Err(ChannelError::Io(ref e)) if e.kind() == ErrorKind::WouldBlock => {
                    continue;
                }
                r => return r,
            }
        }
    }

    /// Sends a message over the Unix socket.
    /// If the message is `Msg::FileDescriptor`, the contained `RawFd` is sent
    /// via ancillary data.
    async fn send_msg(&mut self, msg: &Msg) -> Result<()> {
        let fd_to_send: Option<RawFd> = match msg {
            Msg::FileDescriptor(fd) => Some(*fd),
            _ => None,
        };

        // 1. Serialize to a temporary Vec ONLY to find the exact size.
        //    This is necessary because we must check size BEFORE writing to fixed buffer.
        let serialized_msg = serialize(msg)?;
        let serialized_len = serialized_msg.len();

        // 2. Check if it fits in the fixed buffer (prefix + data)
        if serialized_len > MAX_PAYLOAD_SIZE {
            return Err(ChannelError::MessageTooLargeForTxBuffer(
                serialized_len,
                MAX_PAYLOAD_SIZE,
            ));
        }
        let total_size_to_send = 4 + serialized_len; // Include 4-byte prefix

        // 3. Prepare the fixed tx_buffer
        //    Write length prefix
        (&mut self.tx_buffer[0..4]).write_u32::<BigEndian>(serialized_len as u32)?;
        //    Copy the serialized message data
        self.tx_buffer[4..total_size_to_send].copy_from_slice(&serialized_msg);

        // 4. Loop to send the data and FD
        let mut total_bytes_sent = 0;
        while total_bytes_sent < total_size_to_send {
            let remaining_slice = &self.tx_buffer[total_bytes_sent..total_size_to_send];

            let fd_storage: [RawFd; 1];
            let fds_to_send_this_iter: &[RawFd]; // The slice to pass to send_fd

            if total_bytes_sent == 0 {
                // Only try to send FD on the first iteration
                if let Some(fd_val) = fd_to_send {
                    // Copy the RawFd into our stack storage.
                    fd_storage = [fd_val];
                    // Create a slice referencing the stack storage.
                    // This slice is valid for this loop iteration.
                    fds_to_send_this_iter = &fd_storage;
                } else {
                    // No FD to send, use the static empty slice.
                    fds_to_send_this_iter = &[];
                }
            } else {
                // Not the first iteration, don't send FD again.
                fds_to_send_this_iter = &[];
            }

            match self
                .stream
                .send_with_fd(remaining_slice, fds_to_send_this_iter)
            {
                Ok(bytes_sent_this_iter) => {
                    if bytes_sent_this_iter == 0 {
                        // Should not happen on a blocking socket unless connection closed/error
                        return Err(ChannelError::Io(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "send_fd returned 0 bytes sent",
                        )));
                    }

                    // println!("sent bytes = {}", bytes_sent_this_iter);

                    total_bytes_sent += bytes_sent_this_iter;
                }
                Err(e) => {
                    // Do we actually get EINTR? How does this affect the file descriptor sending?
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }

                    return Err(ChannelError::Io(e));
                }
            }
        }

        // If loop completes, all bytes were sent
        Ok(())
    }

    /// Receives a message from the Unix socket.
    /// Handles receiving file descriptors via ancillary data and associates
    /// them with `Msg::FileDescriptor` messages.
    async fn recv_msg(&mut self) -> Result<Msg> {
        // total_bytes_read now tracks ALL valid bytes from the start of the buffer,
        // including any leftover data from the previous call.
        let mut total_bytes_read = self.rx_buffer_offset;
        let mut message_length: Option<usize> = None; // Full message length (prefix + payload)
        let mut fd_buf = [0 as RawFd; 8];

        // --- Try checking for message length using potentially leftover data first ---
        if message_length.is_none() && total_bytes_read >= 4 {
            let payload_len = (&self.rx_buffer[0..4]).read_u32().await? as usize;
            let expected_total_len = 4 + payload_len;
            if expected_total_len > RX_BUFFER_SIZE {
                // Reset offset? Maybe not, let the error propagate. This state is likely unrecoverable.
                // self.rx_buffer_offset = 0;
                return Err(ChannelError::MessageTooLargeForRxBuffer(
                    expected_total_len,
                    RX_BUFFER_SIZE,
                ));
            }
            message_length = Some(expected_total_len);

            // Check if leftover data already contains the full message
            if let Some(expected_len) = message_length {
                if total_bytes_read >= expected_len {
                    // Full message was already in the buffer from last time!
                    // No need to read more for THIS message. Proceed to deserialize.
                    // The loop below will be skipped.
                }
            }
        }

        // --- Loop reading chunks if message not fully present in leftover data ---
        // Condition: Continue if we don't know the length yet, OR if we know the length
        //            but haven't read enough total bytes yet.
        while message_length.is_none() || total_bytes_read < message_length.unwrap() {
            // Determine slice to read into (space after existing valid data)
            let current_read_slice = &mut self.rx_buffer[total_bytes_read..];

            if current_read_slice.is_empty() {
                // Buffer is full, but we still haven't completed the message.
                return Err(ChannelError::MessageTooLargeForRxBuffer(
                    message_length.unwrap_or(RX_BUFFER_SIZE + 1), // Best guess
                    RX_BUFFER_SIZE,
                ));
            }

            // Perform the read
            match self.stream.recv_with_fd(current_read_slice, &mut fd_buf) {
                Ok((bytes_read_this_iter, fds_received_this_iter)) => {
                    if bytes_read_this_iter == 0 {
                        // EOF
                        return Err(ChannelError::ConnectionClosedPrematurely);
                    }

                    total_bytes_read += bytes_read_this_iter; // Update total valid bytes count

                    // Buffer FDs
                    for i in 0..fds_received_this_iter {
                        if fd_buf[i] >= 0 {
                            self.received_fds.push_back(fd_buf[i]);
                        } else {
                            eprintln!("Warning: Received invalid FD {}", fd_buf[i]);
                        }
                    }

                    // Try parsing length again if we just got enough bytes
                    if message_length.is_none() && total_bytes_read >= 4 {
                        let payload_len = (&self.rx_buffer[0..4]).read_u32().await? as usize;
                        let expected_total_len = 4 + payload_len;
                        if expected_total_len > RX_BUFFER_SIZE {
                            // Reset offset before error? Probably not needed.
                            // self.rx_buffer_offset = 0;
                            return Err(ChannelError::MessageTooLargeForRxBuffer(
                                expected_total_len,
                                RX_BUFFER_SIZE,
                            ));
                        }
                        message_length = Some(expected_total_len);
                    }

                    // Check if we've completed reading the message *after* this read
                    if let Some(expected_len) = message_length {
                        if total_bytes_read >= expected_len {
                            break; // Exit loop, message complete
                        }
                        // else: continue loop, need more data
                    }
                    // else: continue loop, still need prefix data
                } // end Ok match arm
                // Err(sendfd::RecvFdError::Io(ref e)) if e.kind() == io::ErrorKind::Interrupted => {
                //     continue; // Retry on signal
                // }
                Err(e) => {
                    // Reset offset before error? Probably not. Error state.
                    // self.rx_buffer_offset = 0;
                    return Err(ChannelError::Io(e));
                }
            } // end match recv_fd
        } // end loop

        // --- Post-loop processing: A full message is now in the buffer ---
        let final_message_len = message_length.expect("Loop exited without msg length");

        // Deserialize payload (index 4 up to message end)
        let payload_slice = &self.rx_buffer[4..final_message_len];
        let mut msg: Msg = deserialize(payload_slice)?;

        // --- FD Association (remains the same logic) ---
        match msg {
            Msg::FileDescriptor(_) => {
                if let Some(received_fd) = self.received_fds.pop_front() {
                    msg = Msg::FileDescriptor(received_fd);
                } else {
                    // Reset offset before error? Maybe. This is a protocol error.
                    // self.rx_buffer_offset = 0;
                    return Err(ChannelError::MissingFdForMessage);
                }
            }
            _ => {
                if !self.received_fds.is_empty() {
                    eprintln!("Warning: FD received and buffered but current message {:?} does not expect one.", msg);
                    // Handle stricter if needed
                }
            }
        }

        // --- Handle Leftover Data ---
        let leftover_len = total_bytes_read - final_message_len;
        if leftover_len > 0 {
            // Move the leftover data to the beginning of the buffer
            self.rx_buffer
                .copy_within(final_message_len..total_bytes_read, 0);
            // Update the offset for the next read
            self.rx_buffer_offset = leftover_len;
        } else {
            // No leftover data
            self.rx_buffer_offset = 0;
        }

        Ok(msg)
    }

    /// Explicitly closes any buffered FDs that were received but not consumed
    /// by a Msg::FileDescriptor message. This is important to prevent leaks
    /// if the connection closes unexpectedly or the protocol has errors.
    fn close_buffered_fds(&mut self) {
        while let Some(fd) = self.received_fds.pop_front() {
            println!("Closing orphaned buffered FD: {}", fd);
            unsafe { libc::close(fd) };
        }
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        self.close_buffered_fds();
        // The UnixStream will be closed automatically when dropped.
    }
}

fn make_stream(fd: RawFd) -> io::Result<UnixStream> {
    let sock = unsafe { std::os::unix::net::UnixStream::from_raw_fd(fd) };
    sock.set_nonblocking(true)?;

    UnixStream::from_std(sock)
}
