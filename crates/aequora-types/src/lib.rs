//! Database-independent primitives used throughout Aequora.

use core::{fmt, str::FromStr};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// HTTP header carrying a stable [`OperationalErrorCode`] without exposing domain payloads.
pub const OPERATIONAL_ERROR_CODE_HEADER: &str = "x-aequora-error-code";

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
uuid_id!(/// Stable identity shared by every operation, event, and job from one root action.
    CorrelationId);
uuid_id!(/// Stable identity of one authoritative journal event.
    EventId);
uuid_id!(/// Stable identity of one durable background job.
    JobId);
uuid_id!(/// Stable identity of one replica repair attempt.
    RepairId);
uuid_id!(/// Stable identity of one bounded anti-entropy exchange.
    IntegritySessionId);

/// Direct causal predecessor of an operation, authoritative event, or durable job.
///
/// Causation is intentionally separate from operation dependencies and journal ordering.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum LineageRef {
    /// A client or server domain operation directly caused this work.
    Operation(OperationId),
    /// An authoritative journal event directly caused this work.
    Event(EventId),
    /// A durable background job directly caused this work.
    Job(JobId),
}

/// Retry-stable correlation and direct-causation metadata.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct LineageContext {
    /// Root user/system action shared by all descendants.
    pub correlation_id: CorrelationId,
    /// Immediate semantic cause, independent from dependency ordering.
    pub caused_by: Option<LineageRef>,
}

impl LineageContext {
    /// Creates lineage for a new root action.
    #[must_use]
    pub fn root() -> Self {
        Self {
            correlation_id: CorrelationId::new(),
            caused_by: None,
        }
    }

    /// Creates a descendant while preserving the root correlation.
    #[must_use]
    pub const fn derived(self, caused_by: LineageRef) -> Self {
        Self {
            correlation_id: self.correlation_id,
            caused_by: Some(caused_by),
        }
    }

    /// Sentinel used only while decoding a pre-lineage operation envelope.
    #[doc(hidden)]
    #[must_use]
    pub const fn legacy_missing() -> Self {
        Self {
            correlation_id: CorrelationId::from_uuid(Uuid::nil()),
            caused_by: None,
        }
    }

    /// Deterministically upgrades a pre-lineage envelope using its stable operation identity.
    #[must_use]
    pub fn resolved_for_operation(self, operation_id: OperationId) -> Self {
        if self.correlation_id.as_uuid().is_nil() {
            Self {
                correlation_id: CorrelationId::from_uuid(operation_id.as_uuid()),
                caused_by: self.caused_by,
            }
        } else {
            self
        }
    }
}

impl Default for LineageContext {
    fn default() -> Self {
        Self::root()
    }
}

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

/// Stable, payload-free operational failure category shared across transports and diagnostics.
///
/// The textual values are part of the operator-facing compatibility surface. Human-readable error
/// messages may change without changing these codes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum OperationalErrorCode {
    /// Global or tenant admission capacity was exhausted.
    Overloaded,
    /// An operator-selected maintenance mode rejected synchronization work.
    Maintenance,
    /// The client is outside the supported upgrade window.
    UpgradeRequired,
    /// Authentication or authenticated identity validation failed.
    Authentication,
    /// Protocol framing, compatibility, or structural validation failed.
    Protocol,
    /// Authoritative persistence was unavailable or failed.
    Storage,
    /// A domain conflict requires explicit resolution.
    Conflict,
    /// Request data failed a non-protocol validation rule.
    Validation,
    /// A bounded receive, execution, or dependency deadline elapsed.
    Deadline,
    /// The server is draining and no longer admits new work.
    Draining,
    /// A wire or decompressed payload exceeded a configured bound.
    PayloadLimit,
}

impl OperationalErrorCode {
    /// Stable machine-readable code sent through operational boundaries.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Overloaded => "AEQ-OVERLOAD-001",
            Self::Maintenance => "AEQ-MAINT-001",
            Self::UpgradeRequired => "AEQ-UPGRADE-001",
            Self::Authentication => "AEQ-AUTH-001",
            Self::Protocol => "AEQ-PROTO-001",
            Self::Storage => "AEQ-STORAGE-001",
            Self::Conflict => "AEQ-CONFLICT-001",
            Self::Validation => "AEQ-VALIDATION-001",
            Self::Deadline => "AEQ-DEADLINE-001",
            Self::Draining => "AEQ-DRAIN-001",
            Self::PayloadLimit => "AEQ-LIMIT-001",
        }
    }

    /// Parses a known stable code and rejects unrecognized values.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "AEQ-OVERLOAD-001" => Some(Self::Overloaded),
            "AEQ-MAINT-001" => Some(Self::Maintenance),
            "AEQ-UPGRADE-001" => Some(Self::UpgradeRequired),
            "AEQ-AUTH-001" => Some(Self::Authentication),
            "AEQ-PROTO-001" => Some(Self::Protocol),
            "AEQ-STORAGE-001" => Some(Self::Storage),
            "AEQ-CONFLICT-001" => Some(Self::Conflict),
            "AEQ-VALIDATION-001" => Some(Self::Validation),
            "AEQ-DEADLINE-001" => Some(Self::Deadline),
            "AEQ-DRAIN-001" => Some(Self::Draining),
            "AEQ-LIMIT-001" => Some(Self::PayloadLimit),
            _ => None,
        }
    }
}

impl std::fmt::Display for OperationalErrorCode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
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
    fn derived_lineage_preserves_root_correlation() {
        let root = LineageContext::root();
        let operation = OperationId::new();
        let derived = root.derived(LineageRef::Operation(operation));
        assert_eq!(derived.correlation_id, root.correlation_id);
        assert_eq!(derived.caused_by, Some(LineageRef::Operation(operation)));
    }

    #[test]
    fn legacy_lineage_fallback_is_deterministic_per_operation() {
        let operation = OperationId::new();
        let expected = CorrelationId::from_uuid(operation.as_uuid());
        assert_eq!(
            LineageContext::legacy_missing()
                .resolved_for_operation(operation)
                .correlation_id,
            expected
        );
        assert_eq!(
            LineageContext::legacy_missing().resolved_for_operation(operation),
            LineageContext::legacy_missing().resolved_for_operation(operation)
        );
    }

    #[test]
    fn versions_never_wrap() {
        let version = match EntityVersion::new(u64::MAX) {
            Ok(version) => version,
            Err(error) => panic!("{error}"),
        };
        assert_eq!(version.checked_next(), None);
    }

    #[test]
    fn operational_error_codes_are_stable_and_round_trip() {
        let codes = [
            OperationalErrorCode::Overloaded,
            OperationalErrorCode::Maintenance,
            OperationalErrorCode::UpgradeRequired,
            OperationalErrorCode::Authentication,
            OperationalErrorCode::Protocol,
            OperationalErrorCode::Storage,
            OperationalErrorCode::Conflict,
            OperationalErrorCode::Validation,
            OperationalErrorCode::Deadline,
            OperationalErrorCode::Draining,
            OperationalErrorCode::PayloadLimit,
        ];
        for code in codes {
            assert_eq!(OperationalErrorCode::parse(code.as_str()), Some(code));
        }
        assert_eq!(OperationalErrorCode::parse("AEQ-UNKNOWN-001"), None);
    }
}
