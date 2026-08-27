//! Transport abstraction. The synchronization engine is not coupled to HTTP.

use aequora_protocol::{BootstrapRequest, BootstrapResponse, PushHint, SyncRequest, SyncResponse};
use aequora_types::OperationalErrorCode;
use async_trait::async_trait;
use std::time::Duration;
use thiserror::Error;

/// Exchange failure and retry semantics.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("transport {kind:?}: {message}")]
pub struct TransportError {
    /// Whether an unchanged exchange is worth retrying.
    pub kind: TransportErrorKind,
    /// Stable operational category, when the transport supplied one.
    pub code: Option<OperationalErrorCode>,
    /// Advisory server delay. Client policy must cap it and retain jitter/backoff.
    pub retry_after: Option<Duration>,
    /// Non-sensitive implementation explanation.
    pub message: String,
}

impl TransportError {
    /// Constructs a transient transport failure.
    #[must_use]
    pub fn transient(message: impl Into<String>) -> Self {
        Self {
            kind: TransportErrorKind::Transient,
            code: None,
            retry_after: None,
            message: message.into(),
        }
    }

    /// Constructs a permanent transport failure.
    #[must_use]
    pub fn permanent(message: impl Into<String>) -> Self {
        Self {
            kind: TransportErrorKind::Permanent,
            code: None,
            retry_after: None,
            message: message.into(),
        }
    }

    /// Attaches a stable payload-free operational category.
    #[must_use]
    pub const fn with_code(mut self, code: OperationalErrorCode) -> Self {
        self.code = Some(code);
        self
    }

    /// Attaches advisory server retry timing without changing retry classification.
    #[must_use]
    pub const fn with_retry_after(mut self, retry_after: Duration) -> Self {
        self.retry_after = Some(retry_after);
        self
    }
}

/// Retry significance of a transport error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransportErrorKind {
    /// Connection, timeout, or server availability failure.
    Transient,
    /// Authentication, compatibility, or malformed response failure.
    Permanent,
}

/// Bidirectional request-response exchange.
#[async_trait]
pub trait SyncTransport: Send + Sync {
    /// Sends pending operations and a cursor, then receives acknowledgements and changes.
    async fn exchange(&self, request: SyncRequest) -> Result<SyncResponse, TransportError>;

    /// Begins or resumes a consistent bootstrap snapshot.
    async fn bootstrap(
        &self,
        _request: BootstrapRequest,
    ) -> Result<BootstrapResponse, TransportError> {
        Err(TransportError::permanent(
            "bootstrap is not supported by this transport",
        ))
    }

    /// Waits for an advisory notification that a normal synchronization pull may be useful.
    /// Implementations must not place authoritative state or sensitive payloads in a hint.
    async fn next_push_hint(&self) -> Result<PushHint, TransportError> {
        Err(TransportError::permanent(
            "server push hints are not supported by this transport",
        ))
    }
}

/// One ordered, bounded snapshot-page stream.
#[async_trait]
pub trait SnapshotPageStream: Send {
    /// Returns the next page, or `None` after the final page was delivered.
    async fn next_page(&mut self) -> Result<Option<BootstrapResponse>, TransportError>;
}

/// Optional transport capability for carrying many bounded bootstrap pages on one stream.
#[async_trait]
pub trait StreamingSyncTransport: SyncTransport {
    /// Opens a stream beginning or resuming at the supplied snapshot offset.
    async fn bootstrap_stream(
        &self,
        request: BootstrapRequest,
    ) -> Result<Box<dyn SnapshotPageStream>, TransportError>;
}
