use std::{collections::VecDeque, os::fd::RawFd};

use crate::error::ChannelError;

pub trait SerializeFd {
    fn extract_fd(&self) -> Option<RawFd>;
    fn compose_fd(self, received_fds: &mut VecDeque<RawFd>) -> Result<Self, ChannelError>
    where
        Self: std::marker::Sized;
}

pub fn pop_fd(fds: &mut VecDeque<RawFd>) -> Result<RawFd, ChannelError> {
    let fd = fds.pop_front().ok_or(ChannelError::MissingFdForMessage)?;

    Ok(fd)
}
