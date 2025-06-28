use privsep_channel::error::ChannelError;
use privsep_channel::serializefd::{pop_fd, SerializeFd};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::os::unix::io::RawFd;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum ParseEngineMsg {
    NewValue(f64),
}

impl SerializeFd for ParseEngineMsg {
    fn extract_fd(&self) -> Option<RawFd> {
        match self {
            Self::NewValue(_) => None,
        }
    }

    fn compose_fd(self, _fds: &mut VecDeque<RawFd>) -> Result<Self, ChannelError> {
        let msg = match self {
            Self::NewValue(_) => self,
        };

        Ok(msg)
    }
}

// impl std::fmt::Display for ParseEngineMsg {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         match self {
//             Self::Up => write!(f, "Msg::Up"),
//             Self::Down => write!(f, "Msg::Down"),
//             Self::Left => write!(f, "Msg::Left"),
//             Self::Right => write!(f, "Msg::Right"),
//         }
//     }
// }

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum EngineParseMsg {
    Failed,
}

impl SerializeFd for EngineParseMsg {
    fn extract_fd(&self) -> Option<RawFd> {
        match self {
            Self::Failed => None,
        }
    }

    fn compose_fd(self, _fds: &mut VecDeque<RawFd>) -> Result<Self, ChannelError> {
        let msg = match self {
            Self::Failed => self,
        };

        Ok(msg)
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum CtrlParseMsg {
    PeerSocket(#[serde(skip)] RawFd),
    Connection(#[serde(skip)] RawFd),
    Data(String),
    Stop,
}

impl SerializeFd for CtrlParseMsg {
    fn extract_fd(&self) -> Option<RawFd> {
        match self {
            Self::PeerSocket(fd) => Some(*fd),
            Self::Connection(fd) => Some(*fd),
            Self::Data(_) => None,
            Self::Stop => None,
        }
    }

    fn compose_fd(self, fds: &mut VecDeque<RawFd>) -> Result<Self, ChannelError> {
        let msg = match self {
            Self::PeerSocket(_) => Self::PeerSocket(pop_fd(fds)?),
            Self::Connection(_) => Self::Connection(pop_fd(fds)?),
            Self::Data(_) => self,
            Self::Stop => self,
        };

        Ok(msg)
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum ParseCtrlMsg {
    Foo,
}

impl SerializeFd for ParseCtrlMsg {
    fn extract_fd(&self) -> Option<RawFd> {
        match self {
            Self::Foo => None,
        }
    }

    fn compose_fd(self, _fds: &mut VecDeque<RawFd>) -> Result<Self, ChannelError> {
        let msg = match self {
            Self::Foo => self,
        };

        Ok(msg)
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum CtrlEngineMsg {
    PeerSocket(#[serde(skip)] RawFd),
    Stop,
}

impl SerializeFd for CtrlEngineMsg {
    fn extract_fd(&self) -> Option<RawFd> {
        match self {
            Self::PeerSocket(fd) => Some(*fd),
            Self::Stop => None,
        }
    }

    fn compose_fd(self, fds: &mut VecDeque<RawFd>) -> Result<Self, ChannelError> {
        let msg = match self {
            Self::PeerSocket(_) => Self::PeerSocket(pop_fd(fds)?),
            Self::Stop => self,
        };

        Ok(msg)
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum EngineCtrlMsg {
    Bar,
}

impl SerializeFd for EngineCtrlMsg {
    fn extract_fd(&self) -> Option<RawFd> {
        match self {
            Self::Bar => None,
        }
    }

    fn compose_fd(self, _fds: &mut VecDeque<RawFd>) -> Result<Self, ChannelError> {
        let msg = match self {
            Self::Bar => self,
        };

        Ok(msg)
    }
}
