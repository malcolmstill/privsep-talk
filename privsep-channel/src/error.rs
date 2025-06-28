use std::io;

use thiserror::Error;

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
