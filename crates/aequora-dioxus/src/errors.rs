use aequora_client::{AequoraError, AequoraErrorCode};
use std::{fmt, sync::Arc};

/// Stable UI-facing error category. Raw database and transport errors are not retained.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum UiErrorKind {
    Storage,
    Validation,
    Authentication,
    Permission,
    Network,
    Overload,
    Conflict,
    UpgradeRequired,
    RebootstrapRequired,
    Closed,
    Internal,
}

/// Redacted presentation error suitable for localization by stable code and category.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiError {
    kind: UiErrorKind,
    code: Arc<str>,
}

impl UiError {
    #[must_use]
    pub fn new(kind: UiErrorKind, code: impl Into<Arc<str>>) -> Self {
        Self {
            kind,
            code: code.into(),
        }
    }

    #[must_use]
    pub const fn kind(&self) -> UiErrorKind {
        self.kind
    }

    #[must_use]
    pub fn code(&self) -> &str {
        &self.code
    }
}

impl fmt::Display for UiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.code)
    }
}

impl std::error::Error for UiError {}

impl From<AequoraError> for UiError {
    fn from(error: AequoraError) -> Self {
        let code = error.code();
        let kind = match code {
            AequoraErrorCode::StorageUnavailable => UiErrorKind::Storage,
            AequoraErrorCode::TransportUnavailable => UiErrorKind::Network,
            AequoraErrorCode::AuthenticationRequired => UiErrorKind::Authentication,
            AequoraErrorCode::AuthorizationDenied => UiErrorKind::Permission,
            AequoraErrorCode::Conflict => UiErrorKind::Conflict,
            AequoraErrorCode::Validation => UiErrorKind::Validation,
            AequoraErrorCode::Backpressure => UiErrorKind::Overload,
            AequoraErrorCode::UnsupportedCapability | AequoraErrorCode::Internal => {
                UiErrorKind::Internal
            }
            AequoraErrorCode::Closed => UiErrorKind::Closed,
            _ => UiErrorKind::Internal,
        };
        Self::new(kind, code.as_str())
    }
}

/// Error returned by an application-owned local query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryError(UiError);

impl QueryError {
    #[must_use]
    pub fn new(kind: UiErrorKind, code: impl Into<Arc<str>>) -> Self {
        Self(UiError::new(kind, code))
    }

    #[must_use]
    pub const fn kind(&self) -> UiErrorKind {
        self.0.kind()
    }

    #[must_use]
    pub fn code(&self) -> &str {
        self.0.code()
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for QueryError {}

impl From<UiError> for QueryError {
    fn from(error: UiError) -> Self {
        Self(error)
    }
}

impl From<AequoraError> for QueryError {
    fn from(error: AequoraError) -> Self {
        Self(error.into())
    }
}
