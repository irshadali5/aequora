//! Independent version domains used by compatibility policy.

use aequora_types::SchemaVersion;
use serde::{Deserialize, Serialize};

macro_rules! version_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(pub u16);

        impl $name {
            /// First supported version in this domain.
            pub const V1: Self = Self(1);
        }
    };
}

/// Version of one operation payload schema.
pub type OperationSchemaVersion = SchemaVersion;

version_type!(
    /// Canonical domain-state representation version.
    DomainSchemaVersion
);
version_type!(
    /// Snapshot manifest and chunk format version.
    SnapshotSchemaVersion
);
version_type!(
    /// Client-visible projection representation version.
    ProjectionSchemaVersion
);
version_type!(
    /// Version of one payload carried by a stable message kind.
    PayloadVersion
);
version_type!(
    /// Runtime adapter API compatibility version.
    AdapterApiVersion
);
version_type!(
    /// Canonical audit representation version.
    AuditSchemaVersion
);
version_type!(
    /// Governance policy semantics version.
    GovernancePolicyVersion
);
version_type!(
    /// Local durable store format version.
    LocalStoreFormatVersion
);

/// Monotonic generation of the deployed compatibility policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CompatibilityPolicyGeneration(pub u64);

/// Stable message-kind identifier in a versioned wire envelope.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MessageKind(pub u16);
