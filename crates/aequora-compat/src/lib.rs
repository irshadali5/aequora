//! Protocol negotiation, compatibility governance, and safe evolution contracts.
//!
//! The crate is database-, transport-, and async-runtime-neutral. Hosts may reload immutable
//! policy snapshots atomically, route the resulting session profile through any transport, and
//! keep durable local intent untouched when compatibility blocks synchronization.

mod capability;
mod feature;
mod framing;
mod negotiation;
mod operation;
mod policy;
mod registry;
mod version;

pub use capability::*;
pub use feature::*;
pub use framing::*;
pub use negotiation::*;
pub use operation::*;
pub use policy::*;
pub use registry::*;
pub use version::*;

use thiserror::Error;

/// Stable failures produced while validating or applying compatibility contracts.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
#[non_exhaustive]
pub enum CompatibilityError {
    #[error("protocol advertisement is empty, oversized, or contains version zero")]
    InvalidProtocolAdvertisement,
    #[error("capability advertisement is oversized or contains reserved ID zero")]
    CapabilityAdvertisementTooLarge,
    #[error("snapshot advertisement is empty, oversized, or contains version zero")]
    InvalidSnapshotAdvertisement,
    #[error("preferred protocol is not an allowed supported protocol")]
    InvalidPreferredProtocol,
    #[error("protocol policy contains duplicate, forbidden, or zero versions")]
    InvalidProtocolPolicy,
    #[error("required capability is not enabled by this policy or serving fleet")]
    RequiredCapabilityUnavailable,
    #[error("compatibility policy generation must be non-zero")]
    InvalidPolicyGeneration,
    #[error("build policy is internally inconsistent")]
    InvalidBuildPolicy,
    #[error("registry contains a duplicate stable ID")]
    DuplicateRegistryId,
    #[error("registry reuses a reserved or removed stable ID")]
    ReservedRegistryIdReused,
    #[error("registry metadata or version range is invalid")]
    InvalidRegistryEntry,
    #[error("registry source could not be decoded: {0}")]
    RegistryDecode(String),
    #[error("unknown required extension {0}")]
    UnknownRequiredExtension(u16),
    #[error("extension collection exceeds negotiated bounds")]
    ExtensionBoundsExceeded,
    #[error("operation payload is immutable after possible delivery")]
    PossiblySentOperationImmutable,
    #[error("operation schema is unsupported or lacks a complete upcast path")]
    OperationSchemaUnsupported,
    #[error("upcaster did not advance exactly one registered semantic version")]
    InvalidUpcastStep,
    #[error("feature cannot become required before every serving node supports it")]
    FleetCapabilityIncomplete,
}
