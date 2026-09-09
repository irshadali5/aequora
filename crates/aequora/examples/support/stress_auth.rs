// This source is shared by two independent example binaries; each uses one half of the codec.
#![allow(dead_code)]

use aequora_executor::AuthContext;
use aequora_types::{ActorId, DeviceId, TenantId};
use std::{env, error::Error, fmt, str::FromStr, sync::Arc};
use zeroize::Zeroizing;

pub const AUTH_KEY_ENV: &str = "AEQUORA_STRESS_AUTH_KEY_HEX";
const TOKEN_VERSION: &str = "aeq-stress-v1";
const KEY_BYTES: usize = 32;

#[derive(Clone)]
pub struct StressAuthKey(Arc<Zeroizing<[u8; KEY_BYTES]>>);

impl StressAuthKey {
    pub fn from_environment() -> Result<Self, StressAuthError> {
        let encoded = Zeroizing::new(
            env::var(AUTH_KEY_ENV).map_err(|_| StressAuthError::MissingAuthenticationKey)?,
        );
        if encoded.len() != KEY_BYTES * 2 {
            return Err(StressAuthError::InvalidAuthenticationKey);
        }
        let mut key = Zeroizing::new([0_u8; KEY_BYTES]);
        hex::decode_to_slice(encoded.as_bytes(), key.as_mut())
            .map_err(|_| StressAuthError::InvalidAuthenticationKey)?;
        Ok(Self(Arc::new(key)))
    }

    pub fn issue(&self, tenant_id: TenantId, actor_id: ActorId, device_id: DeviceId) -> String {
        let identity = format!("{tenant_id}:{actor_id}:{device_id}");
        let mac = blake3::keyed_hash(self.0.as_ref(), identity.as_bytes());
        format!("{TOKEN_VERSION}:{identity}:{mac}")
    }

    pub fn authenticate(&self, token: &str) -> Option<AuthContext> {
        let mut fields = token.split(':');
        if fields.next()? != TOKEN_VERSION {
            return None;
        }
        let tenant = fields.next()?;
        let actor = fields.next()?;
        let device = fields.next()?;
        let encoded_mac = fields.next()?;
        if fields.next().is_some() || encoded_mac.len() != KEY_BYTES * 2 {
            return None;
        }

        let tenant_id = TenantId::from_str(tenant).ok()?;
        let actor_id = ActorId::from_str(actor).ok()?;
        let device_id = DeviceId::from_str(device).ok()?;
        let identity = format!("{tenant}:{actor}:{device}");
        let expected = blake3::keyed_hash(self.0.as_ref(), identity.as_bytes());
        let mut offered = [0_u8; KEY_BYTES];
        hex::decode_to_slice(encoded_mac.as_bytes(), &mut offered).ok()?;
        constant_time_eq(expected.as_bytes(), &offered).then_some(AuthContext {
            tenant_id,
            actor_id,
            device_id,
        })
    }
}

impl fmt::Debug for StressAuthKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("StressAuthKey([REDACTED])")
    }
}

fn constant_time_eq(left: &[u8; KEY_BYTES], right: &[u8; KEY_BYTES]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StressAuthError {
    MissingAuthenticationKey,
    InvalidAuthenticationKey,
}

impl fmt::Display for StressAuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingAuthenticationKey => write!(
                formatter,
                "{AUTH_KEY_ENV} is required for the local stress harness"
            ),
            Self::InvalidAuthenticationKey => write!(
                formatter,
                "{AUTH_KEY_ENV} must contain exactly 32 hexadecimal bytes"
            ),
        }
    }
}

impl Error for StressAuthError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> StressAuthKey {
        StressAuthKey(Arc::new(Zeroizing::new([byte; KEY_BYTES])))
    }

    #[test]
    fn identity_token_is_bound_and_tamper_evident() {
        let signing_key = key(7);
        let tenant_id = TenantId::new();
        let actor_id = ActorId::new();
        let device_id = DeviceId::new();
        let token = signing_key.issue(tenant_id, actor_id, device_id);

        assert_eq!(
            signing_key.authenticate(&token),
            Some(AuthContext {
                tenant_id,
                actor_id,
                device_id,
            })
        );
        assert!(key(8).authenticate(&token).is_none());

        let tampered = token.replacen(&tenant_id.to_string(), &TenantId::new().to_string(), 1);
        assert!(signing_key.authenticate(&tampered).is_none());
        assert_eq!(format!("{signing_key:?}"), "StressAuthKey([REDACTED])");
    }
}
