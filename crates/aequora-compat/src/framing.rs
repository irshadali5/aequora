//! Stable envelope metadata and bounded extension handling.

use crate::{CompatibilityError, MessageKind, PayloadVersion};
use aequora_types::ProtocolVersion;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Maximum extension sections in one message.
pub const MAX_EXTENSION_SECTIONS: usize = 32;
/// Maximum aggregate extension bytes in one core protocol message.
pub const MAX_EXTENSION_BYTES: usize = 64 * 1024;

/// Decode context required before a payload is interpreted.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolHeader {
    pub protocol: ProtocolVersion,
    pub message_kind: MessageKind,
    pub payload_version: PayloadVersion,
    pub payload_len: u32,
}

impl ProtocolHeader {
    /// Validates non-zero explicit version context.
    ///
    /// # Errors
    ///
    /// Returns an invalid-advertisement error when any version discriminator is zero.
    pub fn validate(self) -> Result<(), CompatibilityError> {
        if self.protocol.0 == 0 || self.message_kind.0 == 0 || self.payload_version.0 == 0 {
            return Err(CompatibilityError::InvalidProtocolAdvertisement);
        }
        Ok(())
    }
}

/// One length-delimited, explicitly optional or required extension section.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExtensionSection {
    pub id: u16,
    pub required: bool,
    pub payload: Vec<u8>,
}

/// Validates bounds and rejects unknown required semantics while allowing known-safe optional data.
///
/// # Errors
///
/// Returns a bounds error for oversized input or an unknown-required-extension error when a
/// required extension is not registered.
pub fn validate_extensions(
    extensions: &[ExtensionSection],
    known: &BTreeSet<u16>,
) -> Result<(), CompatibilityError> {
    let total = extensions.iter().try_fold(0_usize, |total, extension| {
        total.checked_add(extension.payload.len())
    });
    if extensions.len() > MAX_EXTENSION_SECTIONS
        || total.is_none_or(|total| total > MAX_EXTENSION_BYTES)
        || extensions.iter().any(|extension| extension.id == 0)
    {
        return Err(CompatibilityError::ExtensionBoundsExceeded);
    }
    if let Some(extension) = extensions
        .iter()
        .find(|extension| extension.required && !known.contains(&extension.id))
    {
        return Err(CompatibilityError::UnknownRequiredExtension(extension.id));
    }
    Ok(())
}
