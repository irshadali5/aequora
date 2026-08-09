//! Stable wire data-transfer objects for synchronization exchanges.

use aequora_types::{
    ActorId, Cursor, DeviceId, EntityRef, EntityVersion, HybridTimestamp, OperationId,
    ProtocolVersion, RegionId, RequestId, SchemaVersion, Sequence, SessionId, SnapshotId,
    SyncScopeId, TenantId,
};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

/// Absolute allocation ceilings enforced while deserializing untrusted transport DTOs.
/// Deployments normally configure lower runtime limits in `aequora-validator` and transports.
pub mod wire_limits {
    use aequora_types::OperationId;
    use serde::{
        Deserialize,
        de::{self, Deserializer, SeqAccess, Visitor},
    };
    use smallvec::SmallVec;
    use std::{fmt, marker::PhantomData};

    /// Absolute operations accepted by the wire DTO decoder.
    pub const OPERATIONS: usize = 4_096;
    /// Absolute dependencies accepted on one wire operation.
    pub const DEPENDENCIES: usize = 1_024;
    /// Absolute partial-scope selectors accepted by the decoder.
    pub const PARTITIONS: usize = 512;
    /// Absolute feature capabilities accepted by the decoder.
    pub const CAPABILITIES: usize = 64;
    /// Absolute bytes accepted in any individual domain payload or partition value.
    pub const PAYLOAD_BYTES: usize = 16 * 1_024 * 1_024;
    /// Absolute entries accepted in an individual server result collection.
    pub const RESULTS: usize = 8_192;
    /// Absolute entities accepted in one decoded bootstrap page.
    pub const SNAPSHOT_ENTITIES: usize = 8_192;

    struct BoundedSequence<T, const MAX: usize>(PhantomData<T>);

    impl<'de, T, const MAX: usize> Visitor<'de> for BoundedSequence<T, MAX>
    where
        T: Deserialize<'de>,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "a sequence containing at most {MAX} elements")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            if sequence.size_hint().is_some_and(|length| length > MAX) {
                return Err(de::Error::invalid_length(MAX.saturating_add(1), &self));
            }
            let mut items = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX));
            while let Some(item) = sequence.next_element()? {
                if items.len() == MAX {
                    return Err(de::Error::invalid_length(MAX.saturating_add(1), &self));
                }
                items.push(item);
            }
            Ok(items)
        }
    }

    fn bounded_vec<'de, D, T, const MAX: usize>(deserializer: D) -> Result<Vec<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        deserializer.deserialize_seq(BoundedSequence::<T, MAX>(PhantomData))
    }

    pub(crate) fn operations<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        bounded_vec::<D, T, OPERATIONS>(deserializer)
    }

    pub(crate) fn partitions<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        bounded_vec::<D, T, PARTITIONS>(deserializer)
    }

    pub(crate) fn capabilities<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        bounded_vec::<D, T, CAPABILITIES>(deserializer)
    }

    pub(crate) fn payload<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        bounded_vec::<D, u8, PAYLOAD_BYTES>(deserializer)
    }

    pub(crate) fn results<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        bounded_vec::<D, T, RESULTS>(deserializer)
    }

    pub(crate) fn snapshot_entities<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        bounded_vec::<D, T, SNAPSHOT_ENTITIES>(deserializer)
    }

    struct BoundedDependencies;

    impl<'de> Visitor<'de> for BoundedDependencies {
        type Value = SmallVec<[OperationId; 4]>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "a dependency sequence containing at most {DEPENDENCIES} elements"
            )
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            if sequence
                .size_hint()
                .is_some_and(|length| length > DEPENDENCIES)
            {
                return Err(de::Error::invalid_length(
                    DEPENDENCIES.saturating_add(1),
                    &self,
                ));
            }
            let mut items = SmallVec::new();
            while let Some(item) = sequence.next_element()? {
                if items.len() == DEPENDENCIES {
                    return Err(de::Error::invalid_length(
                        DEPENDENCIES.saturating_add(1),
                        &self,
                    ));
                }
                items.push(item);
            }
            Ok(items)
        }
    }

    pub(crate) fn dependencies<'de, D>(
        deserializer: D,
    ) -> Result<SmallVec<[OperationId; 4]>, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_seq(BoundedDependencies)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use serde::{Deserialize, Serialize};

        #[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
        struct TinySequence(#[serde(deserialize_with = "tiny")] Vec<u8>);

        fn tiny<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
        where
            D: Deserializer<'de>,
        {
            bounded_vec::<D, u8, 2>(deserializer)
        }

        #[test]
        fn declared_collection_length_is_rejected_by_the_deserializer() {
            let encoded = postcard::to_stdvec(&TinySequence(vec![1, 2, 3]))
                .unwrap_or_else(|error| panic!("{error}"));
            let decoded = postcard::from_bytes::<TinySequence>(&encoded);
            assert!(decoded.is_err());
        }
    }
}

/// Numeric identifier registered by an application for an operation payload.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OperationKind(pub u16);

/// Metadata that is useful to the application but independent of its payload.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationMetadata {
    /// Optional opaque trace identifier. It must not contain business payload data.
    pub trace_id: Option<String>,
    /// Operation IDs that must execute before this operation.
    #[serde(deserialize_with = "wire_limits::dependencies")]
    pub dependencies: SmallVec<[OperationId; 4]>,
}

/// A domain operation and the synchronization metadata needed to process it safely.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationEnvelope {
    /// Transport protocol spoken by the producer.
    pub protocol_version: ProtocolVersion,
    /// Permanent key used to make retries idempotent.
    pub operation_id: OperationId,
    /// Claimed tenant; the server must compare it with authenticated context.
    pub tenant_id: TenantId,
    /// Actor that originated the operation.
    pub actor_id: ActorId,
    /// Device that originated the operation.
    pub device_id: DeviceId,
    /// Aggregate root or entity targeted by the operation.
    pub entity: EntityRef,
    /// Authoritative version on which the local edit was based.
    pub base_version: Option<EntityVersion>,
    /// Causal timestamp metadata, never used as a cursor or entity version.
    pub created_at: HybridTimestamp,
    /// Application payload schema version.
    pub schema_version: SchemaVersion,
    /// Application-registered operation decoder/handler identifier.
    pub operation_kind: OperationKind,
    /// Postcard-encoded application command. Raw SQL is never valid here.
    #[serde(deserialize_with = "wire_limits::payload")]
    pub payload: Vec<u8>,
    /// Dependencies and non-sensitive diagnostic metadata.
    pub metadata: OperationMetadata,
}

/// Authenticated session metadata sent on every exchange.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionMetadata {
    /// Client session identity.
    pub session_id: SessionId,
    /// Client device identity.
    pub device_id: DeviceId,
    /// Authenticated actor identity as understood by the client.
    pub actor_id: ActorId,
    /// Tenant requested by the client.
    pub tenant_id: TenantId,
    /// Scope of the requested journal cursor.
    pub scope_id: SyncScopeId,
    /// Opaque application-defined filters that make up this partial synchronization scope.
    #[serde(deserialize_with = "wire_limits::partitions")]
    pub partitions: Vec<Partition>,
}

/// One opaque partial-synchronization partition selector.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Partition {
    /// Compact application-defined partition kind.
    pub kind: u16,
    /// Opaque bounded value interpreted only by the application/server adapter.
    #[serde(deserialize_with = "wire_limits::payload")]
    pub value: Vec<u8>,
}

/// A feature that can be negotiated additively.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[non_exhaustive]
pub enum Capability {
    /// Postcard payloads using protocol version one.
    PostcardV1,
    /// Zstandard compression is supported.
    Zstd,
    /// Snapshot bootstrap version one is supported.
    SnapshotV1,
    /// The peer understands tombstones.
    Tombstones,
    /// The transport can deliver multiple snapshot pages on one bounded stream.
    StreamingSnapshots,
    /// The transport can deliver payload-free journal-advance hints.
    PushHints,
    /// The peer supports the Aequora QUIC framing profile.
    Quic,
    /// The peer understands region-routing metadata.
    MultiRegion,
}

/// Client-enforced response limits advertised to the server.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClientLimits {
    /// Maximum number of remote changes accepted in this response.
    pub max_changes: u32,
    /// Maximum uncompressed response size the client is prepared to accept.
    pub max_response_bytes: u32,
}

impl Default for ClientLimits {
    fn default() -> Self {
        Self {
            max_changes: 1_024,
            max_response_bytes: 4 * 1_024 * 1_024,
        }
    }
}

/// One bidirectional push/pull synchronization request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SyncRequest {
    /// Wire protocol version.
    pub protocol: ProtocolVersion,
    /// Unique request identity used only for correlation and diagnostics.
    pub request_id: RequestId,
    /// Client session and identity claims.
    pub session: SessionMetadata,
    /// Last authoritative sequence durably reconciled by the client.
    pub cursor: Option<Cursor>,
    /// Pending domain operations.
    #[serde(deserialize_with = "wire_limits::operations")]
    pub operations: Vec<OperationEnvelope>,
    /// Limits the server must honor.
    pub limits: ClientLimits,
    /// Features supported by the client.
    #[serde(deserialize_with = "wire_limits::capabilities")]
    pub capabilities: Vec<Capability>,
}

/// Result retained by the server operation ledger and returned for retries.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationAck {
    /// Operation that produced this acknowledgement.
    pub operation_id: OperationId,
    /// Resulting authoritative entity version.
    pub entity_version: EntityVersion,
    /// Journal sequence produced by the operation.
    pub sequence: Sequence,
    /// True when this response was replayed from the idempotency ledger.
    pub duplicate: bool,
}

/// Stable machine-readable rejection categories.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub enum RejectionCode {
    /// Request identity did not match the authenticated context.
    IdentityMismatch,
    /// The actor is not allowed to perform the operation.
    Unauthorized,
    /// Wire shape or bounded-field validation failed.
    InvalidOperation,
    /// The application rejected the operation's business semantics.
    BusinessRule,
    /// The operation depends on an unavailable or rejected operation.
    Dependency,
    /// Operation schema is outside the application's compatibility window.
    SchemaIncompatible,
}

/// A permanent operation rejection that should not be retried unchanged.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationRejection {
    /// Rejected operation.
    pub operation_id: OperationId,
    /// Machine-readable category.
    pub code: RejectionCode,
    /// Bounded, non-sensitive explanation suitable for a conflict inbox.
    pub message: String,
}

/// Conflict behavior selected by application policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub enum ConflictPolicy {
    /// Reject stale writes.
    Reject,
    /// Keep authoritative state.
    ServerWins,
    /// Application explicitly permits client replacement.
    ClientWins,
    /// Application-specific merger is required.
    CustomMerge,
    /// A human must resolve the conflict.
    ManualResolution,
    /// Merge independently timestamped application fields.
    FieldMerge,
    /// Apply an application-defined commutative mutation.
    CommutativeOperation,
    /// Merge an application-defined convergent replicated data type.
    Crdt,
}

/// A stale-base conflict surfaced to the client.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Conflict {
    /// Operation that encountered the conflict.
    pub operation_id: OperationId,
    /// Entity whose version diverged.
    pub entity: EntityRef,
    /// Version from which the client edited.
    pub client_base: Option<EntityVersion>,
    /// Current authoritative version, if the entity exists.
    pub server_version: Option<EntityVersion>,
    /// Policy applied by the server.
    pub policy: ConflictPolicy,
    /// Non-sensitive application explanation.
    pub message: String,
}

/// Kind of authoritative state transition.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ChangeKind {
    /// Entity was created or replaced with active state.
    Upsert,
    /// Entity was deleted but remains represented for synchronization.
    Tombstone,
}

/// An authoritative journal entry pulled by a client.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteChange {
    /// Tenant owning the change.
    pub tenant_id: TenantId,
    /// Scope in which `sequence` is monotonic.
    pub scope_id: SyncScopeId,
    /// Authoritative journal position.
    pub sequence: Sequence,
    /// Operation that produced the change.
    pub operation_id: OperationId,
    /// Changed entity.
    pub entity: EntityRef,
    /// Resulting entity version.
    pub version: EntityVersion,
    /// Upsert or tombstone.
    pub change_kind: ChangeKind,
    /// Application-defined authoritative snapshot/event payload.
    #[serde(deserialize_with = "wire_limits::payload")]
    pub payload: Vec<u8>,
    /// Authoritative event timestamp.
    pub timestamp: HybridTimestamp,
}

/// Reason incremental synchronization must restart from a consistent snapshot.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub enum ResyncReason {
    /// The client's cursor predates retained journal history.
    CursorExpired,
    /// The requested partial synchronization scope changed incompatibly.
    ScopeChanged,
    /// Domain schema cannot be migrated incrementally.
    SchemaIncompatible,
    /// The device exceeded the deployment's inactivity window.
    DeviceInactive,
    /// Local or authoritative consistency checks detected corruption.
    CorruptionDetected,
}

/// Typed server instruction accompanying every synchronization response.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum SyncDirective {
    /// Reconcile this normal incremental response.
    #[default]
    Continue,
    /// This client protocol falls outside the server compatibility window.
    UpgradeRequired {
        /// Oldest protocol accepted by the server.
        minimum: ProtocolVersion,
        /// Current protocol emitted by the server.
        current: ProtocolVersion,
    },
    /// Discard incremental progress only through the normal atomic bootstrap flow.
    ResyncRequired {
        /// Stable reason suitable for application policy and diagnostics.
        reason: ResyncReason,
    },
}

/// One response containing push results and incremental pull changes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SyncResponse {
    /// Server protocol version.
    pub protocol: ProtocolVersion,
    /// Compatibility or recovery instruction evaluated before reconciliation.
    pub directive: SyncDirective,
    /// Accepted operations, including deterministic duplicate replies.
    #[serde(deserialize_with = "wire_limits::results")]
    pub acknowledged: Vec<OperationAck>,
    /// Permanently rejected operations.
    #[serde(deserialize_with = "wire_limits::results")]
    pub rejected: Vec<OperationRejection>,
    /// Operations requiring conflict handling.
    #[serde(deserialize_with = "wire_limits::results")]
    pub conflicts: Vec<Conflict>,
    /// Authoritative journal page after the client's cursor.
    #[serde(deserialize_with = "wire_limits::results")]
    pub changes: Vec<RemoteChange>,
    /// Cursor through which returned changes are complete.
    pub next_cursor: Cursor,
    /// True when another exchange is needed to finish the current pull.
    pub has_more: bool,
    /// Hybrid timestamp emitted by the server.
    pub server_time: HybridTimestamp,
}

/// Client limits for one bootstrap snapshot page.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotLimits {
    /// Maximum entities accepted in one page.
    pub max_entities: u32,
    /// Maximum aggregate application payload bytes accepted in one page.
    pub max_payload_bytes: u32,
}

impl Default for SnapshotLimits {
    fn default() -> Self {
        Self {
            max_entities: 512,
            max_payload_bytes: 4 * 1_024 * 1_024,
        }
    }
}

/// Request to begin or resume a consistent snapshot bootstrap.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BootstrapRequest {
    /// Wire protocol version.
    pub protocol: ProtocolVersion,
    /// Unique request identity used only for correlation and diagnostics.
    pub request_id: RequestId,
    /// Authenticated session and partial scope.
    pub session: SessionMetadata,
    /// Existing consistent snapshot when resuming, or `None` to begin.
    pub snapshot_id: Option<SnapshotId>,
    /// Zero-based entity offset requested from the snapshot.
    pub offset: u64,
    /// Page bounds the server must honor.
    pub limits: SnapshotLimits,
    /// Client features used for negotiation.
    #[serde(deserialize_with = "wire_limits::capabilities")]
    pub capabilities: Vec<Capability>,
}

/// One authoritative entity in a consistent bootstrap snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SnapshotEntity {
    /// Stable entity identity.
    pub entity: EntityRef,
    /// Authoritative version at the snapshot boundary.
    pub version: EntityVersion,
    /// Application-owned authoritative state.
    #[serde(deserialize_with = "wire_limits::payload")]
    pub payload: Vec<u8>,
    /// True when the snapshot preserves a deletion tombstone.
    pub tombstone: bool,
}

/// One resumable page from a consistent bootstrap snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BootstrapResponse {
    /// Server wire protocol version.
    pub protocol: ProtocolVersion,
    /// Stable snapshot identity shared by every page.
    pub snapshot_id: SnapshotId,
    /// Cursor at the logical snapshot boundary.
    pub cursor: Cursor,
    /// Offset represented by the first entity in this page.
    pub offset: u64,
    /// Bounded page of authoritative entity state.
    #[serde(deserialize_with = "wire_limits::snapshot_entities")]
    pub entities: Vec<SnapshotEntity>,
    /// Offset to request next.
    pub next_offset: u64,
    /// True when another page remains.
    pub has_more: bool,
    /// Server timestamp for causal observation.
    pub server_time: HybridTimestamp,
}

/// Reason a server suggests that the client perform a normal synchronization exchange.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[non_exhaustive]
pub enum PushHintReason {
    /// The authoritative journal advanced beyond the hinted cursor.
    JournalAdvanced,
    /// A previously captured bootstrap snapshot should no longer be resumed.
    SnapshotInvalidated,
    /// The preferred serving region changed.
    RegionChanged,
}

/// Payload-free notification that prompts a client to use the normal pull protocol.
/// Hints are advisory and never carry authoritative business state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PushHint {
    /// Wire protocol spoken by the sender.
    pub protocol: ProtocolVersion,
    /// Tenant boundary to validate before acting on the hint.
    pub tenant_id: TenantId,
    /// Scope whose journal may have advanced.
    pub scope_id: SyncScopeId,
    /// Greatest journal position known when the hint was emitted.
    pub sequence: Sequence,
    /// Why the hint was emitted.
    pub reason: PushHintReason,
    /// Serving region that emitted the hint, when region routing is enabled.
    pub region_id: Option<RegionId>,
}
