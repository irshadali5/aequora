use aequora_scheduler::ServerSchedulingHints;
use thiserror::Error;

/// Stable reason work was rejected before entering authoritative mutation.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum AdmissionRejection {
    #[error("server admission capacity is exhausted")]
    ServerBusy { retry_after_ms: u64 },
    #[error("tenant admission capacity is exhausted")]
    TenantBusy { retry_after_ms: u64 },
    #[error("authenticated tenant request rate is exhausted")]
    RateLimited { retry_after_ms: u64 },
    #[error("bounded admission queue is full")]
    QueueFull { retry_after_ms: u64 },
    #[error("resource budget is exhausted")]
    ResourceBudgetExceeded { retry_after_ms: u64 },
    #[error("request exceeds a configured size or count limit")]
    RequestTooLarge,
    #[error("request dependency graph exceeds a configured limit")]
    TooManyDependencies,
    #[error("work class is unavailable in the current load state")]
    LoadShed { retry_after_ms: u64 },
    #[error("work is disabled by the active brownout policy")]
    Brownout { retry_after_ms: u64 },
}

impl AdmissionRejection {
    #[must_use]
    pub const fn retryable(self) -> bool {
        !matches!(self, Self::RequestTooLarge | Self::TooManyDependencies)
    }

    #[must_use]
    pub const fn retry_after_ms(self) -> Option<u64> {
        match self {
            Self::ServerBusy { retry_after_ms }
            | Self::TenantBusy { retry_after_ms }
            | Self::RateLimited { retry_after_ms }
            | Self::QueueFull { retry_after_ms }
            | Self::ResourceBudgetExceeded { retry_after_ms }
            | Self::LoadShed { retry_after_ms }
            | Self::Brownout { retry_after_ms } => Some(retry_after_ms),
            Self::RequestTooLarge | Self::TooManyDependencies => None,
        }
    }

    /// Converts overload into bounded client guidance. Clients still apply stable jitter/backoff.
    #[must_use]
    pub const fn scheduling_hints(
        self,
        preferred_max_batch_ops: Option<usize>,
        preferred_max_batch_bytes: Option<usize>,
    ) -> ServerSchedulingHints {
        ServerSchedulingHints {
            retry_after_ms: self.retry_after_ms(),
            preferred_max_batch_ops,
            preferred_max_batch_bytes,
            background_allowed: if matches!(self, Self::LoadShed { .. } | Self::Brownout { .. }) {
                Some(false)
            } else {
                None
            },
        }
    }
}

/// Invalid or unsafe overload-policy configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum PolicyError {
    #[error("resource budget must have non-zero execution bounds")]
    InvalidResourceBudget,
    #[error("request limits are zero or inconsistent")]
    InvalidRequestLimits,
    #[error("tenant admission limits are zero or exceed global limits")]
    InvalidTenantPolicy,
    #[error("class limits or reservations are inconsistent")]
    InvalidClassPolicy,
    #[error("a major resource domain has no explicit budget")]
    MissingResourceBudget,
    #[error("load thresholds, capacity percentages, or dwell time are inconsistent")]
    InvalidLoadPolicy,
    #[error("fair queue bounds or weights are inconsistent")]
    InvalidQueuePolicy,
    #[error("rate limit must have a non-zero sustained rate and burst")]
    InvalidRateLimit,
}

/// Stable bounded-queue rejection independent of authoritative execution.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum QueueRejection {
    #[error("global queue capacity is exhausted")]
    GlobalFull,
    #[error("work-class queue capacity is exhausted")]
    ClassFull,
    #[error("tenant queue capacity is exhausted")]
    TenantFull,
}
