//! Database-independent primitives used throughout Aequora.

use core::{fmt, str::FromStr};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

macro_rules! uuid_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Creates an approximately time-ordered `UUIDv7` identifier.
            #[must_use]
            pub fn new() -> Self { Self(Uuid::now_v7()) }

            /// Wraps an existing UUID.
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self { Self(value) }

            /// Returns the underlying UUID.
            #[must_use]
            pub const fn as_uuid(self) -> Uuid { self.0 }
        }

        impl Default for $name {
            fn default() -> Self { Self::new() }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(f) }
        }

        impl FromStr for $name {
            type Err = uuid::Error;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value).map(Self)
            }
        }
    };
}

uuid_id!(/// Stable identity of a synchronized entity.
    EntityId);
uuid_id!(/// Permanent idempotency key for one logical operation.
    OperationId);
uuid_id!(/// Identity of a client installation.
    DeviceId);
uuid_id!(/// Identity of an authenticated actor.
    ActorId);
uuid_id!(/// Identity of an isolated tenant.
    TenantId);
uuid_id!(/// Identity of a client sync session.
    SessionId);
uuid_id!(/// Identity of one synchronization request for telemetry correlation.
    RequestId);
uuid_id!(/// Opaque identity of a synchronization scope.
    SyncScopeId);
uuid_id!(/// Identity of the node producing a hybrid timestamp.
    NodeId);
uuid_id!(/// Identity of one consistent bootstrap snapshot.
    SnapshotId);
uuid_id!(/// Stable identity of a deployment region.
    RegionId);

/// Compact, application-defined entity kind. Zero is reserved.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct EntityType(u16);

impl EntityType {
    /// Constructs an entity type, rejecting the reserved zero value.
    ///
    /// # Errors
    ///
    /// Returns [`ValueError::Zero`] when `value` is zero.
    pub const fn new(value: u16) -> Result<Self, ValueError> {
        if value == 0 {
            Err(ValueError::Zero)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns its wire value.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Identifies an entity independently from its database representation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct EntityRef {
    /// Application-defined entity kind.
    pub entity_type: EntityType,
    /// Globally stable entity identity.
    pub entity_id: EntityId,
}

/// A monotonically increasing authoritative entity version.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct EntityVersion(u64);

impl EntityVersion {
    /// The version assigned to a newly created authoritative entity.
    pub const INITIAL: Self = Self(1);

    /// Creates a non-zero version.
    ///
    /// # Errors
    ///
    /// Returns [`ValueError::Zero`] when `value` is zero.
    pub const fn new(value: u64) -> Result<Self, ValueError> {
        if value == 0 {
            Err(ValueError::Zero)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the integer version.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next version, or `None` on overflow.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// A monotonically increasing sequence in one authoritative journal scope.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct Sequence(pub u64);

/// Client progress in one explicitly identified synchronization scope.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Cursor {
    /// Scope in which the sequence is meaningful.
    pub scope: SyncScopeId,
    /// Greatest durably applied authoritative sequence.
    pub sequence: Sequence,
}

/// A transport protocol version, separate from domain schema versions.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ProtocolVersion(pub u16);

impl ProtocolVersion {
    /// First Aequora protocol version.
    pub const V1: Self = Self(1);
}

/// Application operation schema version.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SchemaVersion(pub u16);

/// Causal metadata. It is not a replication cursor or entity version.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct HybridTimestamp {
    /// Wall-clock component in Unix milliseconds.
    pub physical_ms: i64,
    /// Logical counter used when wall time does not advance.
    pub logical: u32,
    /// Node that emitted the timestamp.
    pub node: NodeId,
}

/// Error returned by checked primitive constructors.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ValueError {
    /// Zero is reserved or invalid for this type.
    #[error("zero is not a valid value")]
    Zero,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_as_text() {
        let id = OperationId::new();
        assert_eq!(id.to_string().parse::<OperationId>(), Ok(id));
    }

    #[test]
    fn versions_never_wrap() {
        let version = match EntityVersion::new(u64::MAX) {
            Ok(version) => version,
            Err(error) => panic!("{error}"),
        };
        assert_eq!(version.checked_next(), None);
    }
}
