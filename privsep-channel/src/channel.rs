use bincode::{deserialize, serialize};
use byteorder::{BigEndian, WriteBytesExt};
use nix::libc;
use sendfd::{RecvWithFd, SendWithFd};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::VecDeque;
use std::io::{self, ErrorKind};
use std::marker::PhantomData;
use std::os::fd::FromRawFd;
use std::os::unix::io::RawFd;
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;

use crate::error::ChannelError;
use crate::serializefd::SerializeFd;

pub type Result<T> = std::result::Result<T, ChannelError>;

pub struct ChannelOld<M>
where
    M: SerializeFd,
    M: Serialize,
    M: DeserializeOwned,
{
    stream: UnixStream,
    received_fds: VecDeque<RawFd>,
    tx_buffer: Box<[u8]>,
    rx_buffer: Box<[u8]>,
    rx_buffer_offset: usize,

    phantom: PhantomData<M>,
}

// Define fixed buffer sizes. TX needs space for prefix + data.
const TX_BUFFER_SIZE: usize = 4096;
const RX_BUFFER_SIZE: usize = 4096;
const PREFIX_BYTES: usize = 4;
// Max payload size must fit within buffer minus prefix length
const MAX_PAYLOAD_SIZE: usize = TX_BUFFER_SIZE - PREFIX_BYTES;

impl<M> ChannelOld<M>
where
    M: SerializeFd,
    M: Serialize,
    M: DeserializeOwned,
{
    pub fn new(stream: UnixStream) -> Self {
        // Set non-blocking for recv_fd, although we handle blocking reads overall
        // stream.set_nonblocking(true).expect("Failed to set non-blocking");
        ChannelOld {
            stream,
            received_fds: VecDeque::new(),
            tx_buffer: vec![0u8; TX_BUFFER_SIZE].into_boxed_slice(),
            rx_buffer: vec![0u8; RX_BUFFER_SIZE].into_boxed_slice(),
            rx_buffer_offset: 0,
            phantom: PhantomData,
        }
    }

    pub fn new_from_fd(fd: RawFd) -> io::Result<Self> {
        let stream = make_stream(fd)?;

        Ok(ChannelOld::new(stream))
    }

    pub async fn send(&mut self, msg: &M) -> Result<()> {
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

    pub async fn recv(&mut self) -> Result<M> {
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
    async fn send_msg(&mut self, msg: &M) -> Result<()>
    where
        M: SerializeFd,
        M: Serialize,
    {
        let fd: Option<RawFd> = msg.extract_fd();

        let serialized_msg = serialize(msg)?;
        let serialized_len = serialized_msg.len();

        if serialized_len > MAX_PAYLOAD_SIZE {
            return Err(ChannelError::MessageTooLargeForTxBuffer(
                serialized_len,
                MAX_PAYLOAD_SIZE,
            ));
        }
        let total_msg_len = PREFIX_BYTES + serialized_len;

        (&mut self.tx_buffer[0..PREFIX_BYTES]).write_u32::<BigEndian>(serialized_len as u32)?;
        self.tx_buffer[PREFIX_BYTES..total_msg_len].copy_from_slice(&serialized_msg);

        let mut total_bytes_sent = 0;
        while total_bytes_sent < total_msg_len {
            let buf = &self.tx_buffer[total_bytes_sent..total_msg_len];

            let fd_storage: [RawFd; 1];
            let fds: &[RawFd];

            if total_bytes_sent == 0 {
                if let Some(fd_val) = fd {
                    fd_storage = [fd_val];

                    fds = &fd_storage;
                } else {
                    fds = &[];
                }
            } else {
                // Not the first iteration, don't send FD again.
                fds = &[];
            }

            match self.stream.send_with_fd(buf, fds) {
                Ok(n) => {
                    if n == 0 {
                        return Err(ChannelError::Io(io::Error::new(
                            io::ErrorKind::WriteZero,
                            "send_fd returned 0 bytes sent",
                        )));
                    }

                    total_bytes_sent += n;
                }
                Err(e) => {
                    if e.kind() == io::ErrorKind::Interrupted {
                        continue;
                    }

                    return Err(ChannelError::Io(e));
                }
            }
        }

        Ok(())
    }

    /// Receives a message from the Unix socket.
    /// Handles receiving file descriptors via ancillary data and associates
    /// them with `Msg::FileDescriptor` messages.
    async fn recv_msg(&mut self) -> Result<M> {
        // total_bytes_read now tracks ALL valid bytes from the start of the buffer,
        // including any leftover data from the previous call.
        let mut total_bytes_read = self.rx_buffer_offset;
        let mut message_length: Option<usize> = None; // Full message length (prefix + payload)
        let mut fd_buf = [0 as RawFd; 8];

        // --- Try checking for message length using potentially leftover data first ---
        if message_length.is_none() && total_bytes_read >= PREFIX_BYTES {
            let payload_len = (&self.rx_buffer[0..PREFIX_BYTES]).read_u32().await? as usize;
            let expected_total_len = PREFIX_BYTES + payload_len;
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
                    (0..fds_received_this_iter).for_each(|i| {
                        if fd_buf[i] >= 0 {
                            self.received_fds.push_back(fd_buf[i]);
                        } else {
                            eprintln!("Warning: Received invalid FD {}", fd_buf[i]);
                        }
                    });

                    // Try parsing length again if we just got enough bytes
                    if message_length.is_none() && total_bytes_read >= PREFIX_BYTES {
                        let payload_len =
                            (&self.rx_buffer[0..PREFIX_BYTES]).read_u32().await? as usize;
                        let expected_total_len = PREFIX_BYTES + payload_len;
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
        let payload_slice = &self.rx_buffer[PREFIX_BYTES..final_message_len];
        let msg: M = deserialize(payload_slice)?;
        let msg = msg.compose_fd(&mut self.received_fds)?;

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
            println!("Closing orphaned buffered FD: {fd}");
            unsafe { libc::close(fd) };
        }
    }
}

impl<M> Drop for ChannelOld<M>
where
    M: SerializeFd,
    M: Serialize,
    M: DeserializeOwned,
{
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
