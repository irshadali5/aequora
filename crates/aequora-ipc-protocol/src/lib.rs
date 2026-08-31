//! Transport-neutral local IPC protocol for desktop application surfaces and an Aequora agent.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fmt;
use thiserror::Error;
use zeroize::Zeroize;

pub const CURRENT_IPC_PROTOCOL_VERSION: IpcProtocolVersion = IpcProtocolVersion(2);
pub const MINIMUM_IPC_PROTOCOL_VERSION: IpcProtocolVersion = IpcProtocolVersion(1);
pub const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct IpcProtocolVersion(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VersionNegotiation {
    Compatible(IpcProtocolVersion),
    AgentUpgradeRequired,
}

#[must_use]
pub const fn negotiate(offered: IpcProtocolVersion) -> VersionNegotiation {
    if offered.0 >= MINIMUM_IPC_PROTOCOL_VERSION.0 && offered.0 <= CURRENT_IPC_PROTOCOL_VERSION.0 {
        VersionNegotiation::Compatible(offered)
    } else {
        VersionNegotiation::AgentUpgradeRequired
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MessageKind {
    Request,
    Response,
    Event,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Handshake {
    pub protocol_version: IpcProtocolVersion,
    pub build_id: String,
    pub registry_generation: u64,
    pub session_token: SessionToken,
}

impl Handshake {
    /// Validates all untrusted handshake fields before authentication.
    ///
    /// # Errors
    ///
    /// Rejects unsupported versions and malformed identities or tokens.
    pub fn validate(&self) -> Result<(), IpcError> {
        if negotiate(self.protocol_version) == VersionNegotiation::AgentUpgradeRequired
            || !valid_text(&self.build_id)
            || !self.session_token.is_valid()
        {
            return Err(IpcError::InvalidHandshake);
        }
        Ok(())
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionToken(Vec<u8>);

impl SessionToken {
    /// Creates a bounded opaque local-session bearer token.
    ///
    /// # Errors
    ///
    /// Rejects tokens shorter than 32 bytes or longer than 256 bytes.
    pub fn new(value: Vec<u8>) -> Result<Self, IpcError> {
        if !(32..=256).contains(&value.len()) {
            return Err(IpcError::InvalidToken);
        }
        Ok(Self(value))
    }
    #[must_use]
    pub fn matches(&self, expected: &Self) -> bool {
        self.0.len() == expected.0.len()
            && self
                .0
                .iter()
                .zip(&expected.0)
                .fold(0_u8, |acc, (a, b)| acc | (a ^ b))
                == 0
    }
    const fn is_valid(&self) -> bool {
        self.0.len() >= 32 && self.0.len() <= 256
    }
}
impl Drop for SessionToken {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}
impl fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionToken([REDACTED])")
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Command {
    OpenStore { profile: String },
    Mutate { operation: Vec<u8> },
    Query { query: Vec<u8> },
    SyncNow,
    GetStatus,
    Subscribe { topic: String },
    ResolveConflict { resolution: Vec<u8> },
    ExportDiagnostics,
    ShutdownAgent,
}

impl Command {
    /// Checks command bounds and ensures all writes are represented as domain operations.
    ///
    /// # Errors
    ///
    /// Rejects empty, oversized, or unsafe fields.
    pub fn validate(&self) -> Result<(), IpcError> {
        let payload = |value: &[u8]| !value.is_empty() && value.len() <= MAX_FRAME_BYTES;
        let valid = match self {
            Self::OpenStore { profile } | Self::Subscribe { topic: profile } => valid_text(profile),
            Self::Mutate { operation } => payload(operation),
            Self::Query { query } => payload(query),
            Self::ResolveConflict { resolution } => payload(resolution),
            Self::SyncNow | Self::GetStatus | Self::ExportDiagnostics | Self::ShutdownAgent => true,
        };
        if valid {
            Ok(())
        } else {
            Err(IpcError::InvalidCommand)
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Event {
    SyncStatusChanged,
    DataChanged,
    ConflictCreated,
    AgentStateChanged,
    UpdateAvailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Frame<T> {
    pub version: IpcProtocolVersion,
    pub kind: MessageKind,
    pub request_id: u64,
    pub payload: T,
}

/// Encodes one length-prefixed Postcard frame.
///
/// # Errors
///
/// Rejects serialization failure or a frame above the hard size limit.
pub fn encode_frame<T: Serialize>(frame: &Frame<T>) -> Result<Vec<u8>, IpcError> {
    let body = postcard::to_stdvec(frame).map_err(|_| IpcError::Encoding)?;
    let length = u32::try_from(body.len()).map_err(|_| IpcError::FrameTooLarge)?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(IpcError::FrameTooLarge);
    }
    let mut framed = Vec::with_capacity(body.len() + 4);
    framed.extend_from_slice(&length.to_le_bytes());
    framed.extend_from_slice(&body);
    Ok(framed)
}

/// Decodes exactly one bounded length-prefixed Postcard frame.
///
/// # Errors
///
/// Rejects truncated, trailing, oversized, malformed, or unsupported frames.
pub fn decode_frame<T: DeserializeOwned>(bytes: &[u8]) -> Result<Frame<T>, IpcError> {
    let prefix: [u8; 4] = bytes
        .get(..4)
        .ok_or(IpcError::Truncated)?
        .try_into()
        .map_err(|_| IpcError::Truncated)?;
    let length =
        usize::try_from(u32::from_le_bytes(prefix)).map_err(|_| IpcError::FrameTooLarge)?;
    if length > MAX_FRAME_BYTES || bytes.len() != length + 4 {
        return Err(IpcError::InvalidLength);
    }
    let frame: Frame<T> = postcard::from_bytes(&bytes[4..]).map_err(|_| IpcError::Encoding)?;
    if negotiate(frame.version) == VersionNegotiation::AgentUpgradeRequired {
        return Err(IpcError::UnsupportedVersion);
    }
    Ok(frame)
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum IpcError {
    #[error("IPC token is invalid")]
    InvalidToken,
    #[error("IPC handshake is invalid or incompatible")]
    InvalidHandshake,
    #[error("IPC command is invalid")]
    InvalidCommand,
    #[error("IPC frame exceeds its bound")]
    FrameTooLarge,
    #[error("IPC frame length is invalid")]
    InvalidLength,
    #[error("IPC frame is truncated")]
    Truncated,
    #[error("IPC protocol version is unsupported")]
    UnsupportedVersion,
    #[error("IPC serialization failed")]
    Encoding,
}

fn valid_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_TEXT_BYTES && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn n_and_n_minus_one_are_supported() {
        assert_eq!(
            negotiate(IpcProtocolVersion(2)),
            VersionNegotiation::Compatible(IpcProtocolVersion(2))
        );
        assert_eq!(
            negotiate(IpcProtocolVersion(1)),
            VersionNegotiation::Compatible(IpcProtocolVersion(1))
        );
        assert_eq!(
            negotiate(IpcProtocolVersion(0)),
            VersionNegotiation::AgentUpgradeRequired
        );
    }

    #[test]
    fn framed_commands_round_trip_and_reject_trailing_bytes() {
        let frame = Frame {
            version: CURRENT_IPC_PROTOCOL_VERSION,
            kind: MessageKind::Request,
            request_id: 7,
            payload: Command::SyncNow,
        };
        let bytes = encode_frame(&frame).unwrap_or_default();
        assert_eq!(decode_frame::<Command>(&bytes), Ok(frame));
        let mut trailing = bytes;
        trailing.push(0);
        assert_eq!(
            decode_frame::<Command>(&trailing),
            Err(IpcError::InvalidLength)
        );
    }
}
