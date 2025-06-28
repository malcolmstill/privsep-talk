use privsep_channel::channel::Result;
use privsep_channel::error::ChannelError;
use privsep_channel::serializefd::SerializeFd;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::os::unix::io::RawFd;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum Msg {
    TextMessage(String),
    IntegerMessage(i64),
    FileDescriptor(#[serde(skip)] RawFd),
}

impl SerializeFd for Msg {
    fn extract_fd(&self) -> Option<RawFd> {
        match self {
            Msg::TextMessage(_) => None,
            Msg::IntegerMessage(_) => None,
            Msg::FileDescriptor(fd) => Some(*fd),
        }
    }

    fn compose_fd(self, received_fds: &mut VecDeque<RawFd>) -> Result<Msg> {
        match self {
            Msg::FileDescriptor(_) => {
                if let Some(received_fd) = received_fds.pop_front() {
                    Ok(Msg::FileDescriptor(received_fd))
                } else {
                    // Reset offset before error? Maybe. This is a protocol error.
                    // self.rx_buffer_offset = 0;
                    Err(ChannelError::MissingFdForMessage)
                }
            }
            msg => {
                if !received_fds.is_empty() {
                    eprintln!("Warning: FD received and buffered but current message {msg:?} does not expect one.");
                    // Handle stricter if needed
                }

                Ok(msg)
            }
        }
    }
}

impl std::fmt::Display for Msg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Msg::TextMessage(txt) => write!(f, "Msg::TextMessage({txt})"),
            Msg::IntegerMessage(n) => write!(f, "Msg::IntegerMessage({n})"),
            Msg::FileDescriptor(fd) => write!(f, "Msg::FileDescriptor({fd})"),
        }
    }
}
