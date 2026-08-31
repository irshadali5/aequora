//! Database-neutral retry and diagnostic classification.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Safe action a caller may take after an adapter failure.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub enum RetryDisposition {
    /// The same request may be retried using the caller's bounded policy.
    Retryable,
    /// Retrying the same request cannot resolve the failure.
    #[default]
    NonRetryable,
    /// The physical connection/store must be reopened before retrying.
    NeedsReopen,
    /// Explicit repair or operator recovery is required.
    NeedsRecovery,
    /// The commit result is ambiguous; query the operation ledger before retrying.
    ResolveCommitOutcome,
}

/// Payload-free physical diagnostic retained without leaking a driver error type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdapterDiagnostic {
    adapter_code: Option<Arc<str>>,
    context: Arc<str>,
}

impl AdapterDiagnostic {
    /// Creates bounded, payload-free diagnostic context.
    ///
    /// # Errors
    ///
    /// Returns an error when the context is empty, contains control characters, or exceeds 512
    /// bytes. Adapter codes are subject to the same checks when present.
    pub fn new(
        adapter_code: Option<impl Into<Arc<str>>>,
        context: impl Into<Arc<str>>,
    ) -> Result<Self, &'static str> {
        let adapter_code = adapter_code.map(Into::into);
        let context = context.into();
        if !is_safe(&context) || adapter_code.as_deref().is_some_and(|value| !is_safe(value)) {
            return Err("adapter diagnostics must be non-empty, bounded, printable text");
        }
        Ok(Self {
            adapter_code,
            context,
        })
    }

    /// Returns the optional physical error code, such as a SQLSTATE.
    #[must_use]
    pub fn adapter_code(&self) -> Option<&str> {
        self.adapter_code.as_deref()
    }

    /// Returns payload-free diagnostic context.
    #[must_use]
    pub fn context(&self) -> &str {
        &self.context
    }
}

fn is_safe(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && value.chars().all(|value| !value.is_control())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_reject_control_characters_and_large_values() {
        assert!(AdapterDiagnostic::new(Some("40001"), "serialization failure").is_ok());
        assert!(AdapterDiagnostic::new(None::<&str>, "bad\nvalue").is_err());
        assert!(AdapterDiagnostic::new(None::<&str>, "x".repeat(513)).is_err());
    }
}
