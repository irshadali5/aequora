use serde::{Deserialize, Serialize};

use crate::{AdmissionRejection, PolicyError};

const TOKEN_SCALE: u128 = 1_000_000;

/// Sustained rate and bounded burst for connection, device, principal, or tenant admission.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RateLimit {
    pub tokens_per_second: u32,
    pub burst: u32,
}

impl Default for RateLimit {
    fn default() -> Self {
        Self {
            tokens_per_second: 100,
            burst: 200,
        }
    }
}

impl RateLimit {
    /// Validates non-zero sustained and burst limits.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::InvalidRateLimit`] when either limit is zero.
    pub const fn validate(self) -> Result<(), PolicyError> {
        if self.tokens_per_second == 0 || self.burst == 0 {
            return Err(PolicyError::InvalidRateLimit);
        }
        Ok(())
    }
}

/// Runtime-neutral integer token bucket. The host supplies monotonic milliseconds.
#[derive(Clone, Copy, Debug)]
pub struct TokenBucket {
    policy: RateLimit,
    available: u128,
    updated_at_ms: u64,
}

impl TokenBucket {
    /// Creates a full bucket at the supplied monotonic instant.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::InvalidRateLimit`] for a zero rate or burst.
    pub fn new(policy: RateLimit, now_ms: u64) -> Result<Self, PolicyError> {
        policy.validate()?;
        Ok(Self {
            policy,
            available: u128::from(policy.burst).saturating_mul(TOKEN_SCALE),
            updated_at_ms: now_ms,
        })
    }

    /// Consumes a bounded token count or reports deterministic retry guidance.
    ///
    /// # Errors
    ///
    /// Returns [`AdmissionRejection::RateLimited`] without consuming tokens when the bucket lacks
    /// capacity. Clients must still apply stable jitter around the returned delay.
    pub fn try_take(&mut self, now_ms: u64, tokens: u32) -> Result<(), AdmissionRejection> {
        self.refill(now_ms);
        let required = u128::from(tokens.max(1)).saturating_mul(TOKEN_SCALE);
        if self.available >= required {
            self.available -= required;
            return Ok(());
        }
        let missing = required.saturating_sub(self.available);
        let refill_per_ms = u128::from(self.policy.tokens_per_second).saturating_mul(1_000);
        let retry_after_ms = missing.div_ceil(refill_per_ms).min(u128::from(u64::MAX));
        Err(AdmissionRejection::RateLimited {
            retry_after_ms: u64::try_from(retry_after_ms).unwrap_or(u64::MAX).max(1),
        })
    }

    fn refill(&mut self, now_ms: u64) {
        let elapsed = now_ms.saturating_sub(self.updated_at_ms);
        if now_ms > self.updated_at_ms {
            self.updated_at_ms = now_ms;
        }
        let added = u128::from(elapsed)
            .saturating_mul(u128::from(self.policy.tokens_per_second))
            .saturating_mul(1_000);
        let maximum = u128::from(self.policy.burst).saturating_mul(TOKEN_SCALE);
        self.available = self.available.saturating_add(added).min(maximum);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_storm_is_burst_bounded_and_refills_monotonically() {
        let mut limiter = TokenBucket::new(
            RateLimit {
                tokens_per_second: 10,
                burst: 20,
            },
            0,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        for _ in 0..20 {
            assert_eq!(limiter.try_take(0, 1), Ok(()));
        }
        assert!(matches!(
            limiter.try_take(0, 1),
            Err(AdmissionRejection::RateLimited {
                retry_after_ms: 100
            })
        ));
        assert_eq!(limiter.try_take(100, 1), Ok(()));
        assert!(limiter.try_take(99, 1).is_err());
    }
}
