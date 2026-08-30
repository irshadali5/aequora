//! Runtime-, transport-, database-, and provider-neutral incident diagnostics.
//!
//! This crate models bounded evidence collection and sandboxed reproduction. It never treats
//! diagnostic artifacts as authoritative business state and deliberately contains no filesystem,
//! archive, networking, database, cryptographic-key, or production side-effect implementation.

#![allow(clippy::missing_errors_doc)]

use aequora_types::{
    AuthorityEpoch, CorrelationId, DeviceId, EntityRef, JobId, OperationId, Sequence, SyncScopeId,
    TenantId,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;
use uuid::Uuid;

pub const INCIDENT_BUNDLE_SCHEMA_VERSION: IncidentBundleSchemaVersion =
    IncidentBundleSchemaVersion::new(1);
pub const DIAGNOSTIC_SECTION_SCHEMA_VERSION: u16 = 1;
pub const MAX_DETAIL_FIELDS: usize = 32;
pub const MAX_DETAIL_TEXT_BYTES: usize = 4_096;
pub const MAX_SELECTORS: usize = 32;
pub const MAX_SECTIONS: usize = 64;
pub const MAX_RELATIVE_PATH_BYTES: usize = 512;

/// Build provenance for the canonical durable registry, safe for authenticated diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct DurableRegistryProvenance {
    pub generation: u64,
    pub digest: &'static str,
    pub entry_count: usize,
}

/// Returns the immutable registry identity embedded into this binary.
#[must_use]
pub const fn durable_registry_provenance() -> DurableRegistryProvenance {
    DurableRegistryProvenance {
        generation: aequora_registry_generated::REGISTRY_GENERATION,
        digest: aequora_registry_generated::REGISTRY_DIGEST,
        entry_count: aequora_registry_generated::ENTRIES.len(),
    }
}

macro_rules! uuid_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

uuid_id!(IncidentId, "Stable `UUIDv7` identity of one incident.");
uuid_id!(
    IncidentBundleId,
    "Stable `UUIDv7` identity of one incident bundle."
);
uuid_id!(
    DiagnosticEventId,
    "Stable `UUIDv7` identity of one diagnostic event."
);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum IncidentClass {
    SyncFailure,
    Divergence,
    ConflictAnomaly,
    BootstrapFailure,
    AuthorityTransition,
    JobFailure,
    DataIntegrity,
    Performance,
    Security,
    Governance,
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TimeRange {
    pub start_unix_ms: u64,
    pub end_unix_ms: u64,
}

impl TimeRange {
    pub fn validate(self, max_window_ms: u64) -> Result<(), DiagnosticError> {
        let duration = self
            .end_unix_ms
            .checked_sub(self.start_unix_ms)
            .ok_or(DiagnosticError::InvalidTimeRange)?;
        if duration > max_window_ms {
            return Err(DiagnosticError::LimitExceeded("time window"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DiagnosticSelector {
    Operation(OperationId),
    Entity(EntityRef),
    Scope(SyncScopeId),
    Device(DeviceId),
    Job(JobId),
    TimeWindow(TimeRange),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum DataClassification {
    Public,
    Operational,
    Personal,
    Restricted,
    Secret,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DiagnosticValue {
    Text(String),
    Number(i64),
    Id(Uuid),
    Digest([u8; 32]),
    Redacted,
}

impl DiagnosticValue {
    #[must_use]
    pub fn sanitized(self, class: DataClassification, allow_sensitive: bool) -> Self {
        match class {
            DataClassification::Secret => Self::Redacted,
            DataClassification::Personal | DataClassification::Restricted if !allow_sensitive => {
                match self {
                    Self::Text(value) => Self::Digest(*blake3::hash(value.as_bytes()).as_bytes()),
                    Self::Id(value) => Self::Digest(*blake3::hash(value.as_bytes()).as_bytes()),
                    Self::Number(value) => {
                        Self::Digest(*blake3::hash(&value.to_le_bytes()).as_bytes())
                    }
                    value @ (Self::Digest(_) | Self::Redacted) => value,
                }
            }
            _ => self,
        }
    }

    fn validate(&self) -> Result<(), DiagnosticError> {
        if let Self::Text(value) = self {
            if value.len() > MAX_DETAIL_TEXT_BYTES {
                return Err(DiagnosticError::LimitExceeded("detail text"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ClassifiedValue {
    pub classification: DataClassification,
    pub value: DiagnosticValue,
}

pub type DiagnosticDetails = BTreeMap<String, ClassifiedValue>;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DiagnosticEventKind {
    SyncStarted,
    SyncBatchSent,
    SyncResponseReceived,
    CursorAdvanced,
    RetryScheduled,
    AuthorityChanged,
    BootstrapStarted,
    BootstrapCheckpoint,
    ConflictRecorded,
    ResourceDeferred,
    Error,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiagnosticEvent {
    pub event_id: DiagnosticEventId,
    pub kind: DiagnosticEventKind,
    pub occurred_at_unix_ms: u64,
    pub correlation_id: Option<CorrelationId>,
    pub operation_id: Option<OperationId>,
    pub scope_id: Option<SyncScopeId>,
    pub details: DiagnosticDetails,
}

impl DiagnosticEvent {
    pub fn validate(&self) -> Result<(), DiagnosticError> {
        if self.details.len() > MAX_DETAIL_FIELDS {
            return Err(DiagnosticError::LimitExceeded("detail fields"));
        }
        for (key, value) in &self.details {
            if key.is_empty() || key.len() > 128 {
                return Err(DiagnosticError::InvalidDetailKey);
            }
            value.value.validate()?;
        }
        Ok(())
    }

    #[must_use]
    pub fn estimated_bytes(&self) -> usize {
        128 + self
            .details
            .iter()
            .map(|(key, value)| {
                key.len()
                    + match &value.value {
                        DiagnosticValue::Text(text) => text.len(),
                        _ => 32,
                    }
            })
            .sum::<usize>()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiagnosticRingLimits {
    pub max_events: usize,
    pub max_bytes: usize,
    pub max_age_ms: u64,
}

impl DiagnosticRingLimits {
    pub fn validate(self) -> Result<(), DiagnosticError> {
        if self.max_events == 0 || self.max_bytes == 0 || self.max_age_ms == 0 {
            return Err(DiagnosticError::InvalidLimits);
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct DiagnosticRing {
    limits: DiagnosticRingLimits,
    events: VecDeque<DiagnosticEvent>,
    bytes: usize,
}

impl DiagnosticRing {
    pub fn new(limits: DiagnosticRingLimits) -> Result<Self, DiagnosticError> {
        limits.validate()?;
        Ok(Self {
            limits,
            events: VecDeque::new(),
            bytes: 0,
        })
    }

    pub fn push(&mut self, event: DiagnosticEvent) -> Result<(), DiagnosticError> {
        event.validate()?;
        let event_bytes = event.estimated_bytes();
        if event_bytes > self.limits.max_bytes {
            return Err(DiagnosticError::LimitExceeded("single event"));
        }
        let cutoff = event
            .occurred_at_unix_ms
            .saturating_sub(self.limits.max_age_ms);
        while self.events.front().is_some_and(|item| {
            item.occurred_at_unix_ms < cutoff
                || self.events.len() >= self.limits.max_events
                || self.bytes.saturating_add(event_bytes) > self.limits.max_bytes
        }) {
            if let Some(removed) = self.events.pop_front() {
                self.bytes = self.bytes.saturating_sub(removed.estimated_bytes());
            }
        }
        self.bytes = self.bytes.saturating_add(event_bytes);
        self.events.push_back(event);
        Ok(())
    }

    #[must_use]
    pub fn snapshot(&self) -> Vec<DiagnosticEvent> {
        self.events.iter().cloned().collect()
    }

    #[must_use]
    pub const fn byte_len(&self) -> usize {
        self.bytes
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DiagnosticMode {
    SupportMinimal,
    SupportStandard,
    Forensic,
    Developer,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DataInclusion {
    Excluded,
    Included,
}

impl DataInclusion {
    #[must_use]
    pub const fn is_included(self) -> bool {
        matches!(self, Self::Included)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ProtectionRequirement {
    Optional,
    Required,
}

impl ProtectionRequirement {
    #[must_use]
    pub const fn is_required(self) -> bool {
        matches!(self, Self::Required)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiagnosticPolicy {
    pub mode: DiagnosticMode,
    pub max_bytes: u64,
    pub max_records_per_section: u32,
    pub max_time_window_ms: u64,
    pub max_files: u32,
    pub max_archive_depth: u16,
    pub include_payloads: DataInclusion,
    pub include_audit_values: DataInclusion,
    pub include_logs: DataInclusion,
    pub require_encryption: ProtectionRequirement,
    pub require_signature: ProtectionRequirement,
}

impl DiagnosticPolicy {
    #[must_use]
    pub const fn support_minimal() -> Self {
        Self {
            mode: DiagnosticMode::SupportMinimal,
            max_bytes: 10 * 1024 * 1024,
            max_records_per_section: 1_000,
            max_time_window_ms: 15 * 60 * 1_000,
            max_files: 128,
            max_archive_depth: 4,
            include_payloads: DataInclusion::Excluded,
            include_audit_values: DataInclusion::Excluded,
            include_logs: DataInclusion::Excluded,
            require_encryption: ProtectionRequirement::Required,
            require_signature: ProtectionRequirement::Optional,
        }
    }

    #[must_use]
    pub const fn support_standard() -> Self {
        Self {
            include_logs: DataInclusion::Included,
            ..Self::support_minimal()
        }
    }

    #[must_use]
    pub const fn forensic(max_bytes: u64) -> Self {
        Self {
            mode: DiagnosticMode::Forensic,
            max_bytes,
            max_records_per_section: 10_000,
            max_time_window_ms: 24 * 60 * 60 * 1_000,
            max_files: 1_024,
            max_archive_depth: 8,
            include_payloads: DataInclusion::Excluded,
            include_audit_values: DataInclusion::Included,
            include_logs: DataInclusion::Included,
            require_encryption: ProtectionRequirement::Required,
            require_signature: ProtectionRequirement::Required,
        }
    }

    pub fn validate(&self, production: bool) -> Result<(), DiagnosticError> {
        if self.max_bytes == 0
            || self.max_records_per_section == 0
            || self.max_time_window_ms == 0
            || self.max_files == 0
            || self.max_archive_depth == 0
        {
            return Err(DiagnosticError::InvalidLimits);
        }
        if production && self.mode == DiagnosticMode::Developer {
            return Err(DiagnosticError::DeveloperModeInProduction);
        }
        if production && !self.require_encryption.is_required() {
            return Err(DiagnosticError::EncryptionRequired);
        }
        if self.mode == DiagnosticMode::Forensic && !self.require_signature.is_required() {
            return Err(DiagnosticError::SignatureRequired);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum DiagnosticSectionId {
    Runtime,
    Outbox,
    Journal,
    Ledger,
    Scope,
    Authority,
    Job,
    Audit,
    Integrity,
    Crypto,
    Compatibility,
    Trace,
    Logs,
    Replay,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum EvidenceConfidence {
    Authoritative,
    DurableLocal,
    Derived,
    BestEffort,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MissingSectionReason {
    Unavailable,
    NotAuthorized,
    NotApplicable,
    CollectionFailed,
    Truncated,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiagnosticRecord {
    pub kind: String,
    pub confidence: EvidenceConfidence,
    pub fields: DiagnosticDetails,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiagnosticSection {
    pub id: DiagnosticSectionId,
    pub schema_version: u16,
    pub confidence: EvidenceConfidence,
    pub records: Vec<DiagnosticRecord>,
    pub truncated_records: u64,
}

impl DiagnosticSection {
    pub fn validate(&self, policy: &DiagnosticPolicy) -> Result<(), DiagnosticError> {
        if self.schema_version == 0 {
            return Err(DiagnosticError::InvalidSchemaVersion);
        }
        if self.records.len() > policy.max_records_per_section as usize {
            return Err(DiagnosticError::LimitExceeded("section records"));
        }
        for record in &self.records {
            if record.kind.is_empty() || record.kind.len() > 128 {
                return Err(DiagnosticError::InvalidRecordKind);
            }
            if record.fields.len() > MAX_DETAIL_FIELDS {
                return Err(DiagnosticError::LimitExceeded("record fields"));
            }
            for value in record.fields.values() {
                value.value.validate()?;
            }
        }
        Ok(())
    }
}

pub trait DiagnosticSanitizer: Send + Sync {
    fn sanitize(
        &self,
        section: DiagnosticSection,
        policy: &DiagnosticPolicy,
    ) -> Result<DiagnosticSection, DiagnosticError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultDiagnosticSanitizer;

impl DiagnosticSanitizer for DefaultDiagnosticSanitizer {
    fn sanitize(
        &self,
        mut section: DiagnosticSection,
        policy: &DiagnosticPolicy,
    ) -> Result<DiagnosticSection, DiagnosticError> {
        section.validate(policy)?;
        let allow_sensitive = matches!(
            policy.mode,
            DiagnosticMode::Forensic | DiagnosticMode::Developer
        ) && policy.include_payloads.is_included();
        for record in &mut section.records {
            for value in record.fields.values_mut() {
                value.value = value.value.clone().sanitized(
                    value.classification,
                    allow_sensitive && value.classification != DataClassification::Secret,
                );
            }
        }
        Ok(section)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiagnosticRequest {
    pub incident_id: IncidentId,
    pub tenant_id: TenantId,
    pub selectors: Vec<DiagnosticSelector>,
    pub requested_sections: BTreeSet<DiagnosticSectionId>,
    pub boundary: Option<BundleBoundary>,
    pub policy: DiagnosticPolicy,
}

impl DiagnosticRequest {
    pub fn validate(&self, production: bool) -> Result<(), DiagnosticError> {
        self.policy.validate(production)?;
        if self.selectors.is_empty() || self.selectors.len() > MAX_SELECTORS {
            return Err(DiagnosticError::InvalidSelectorCount);
        }
        if self.requested_sections.is_empty() || self.requested_sections.len() > MAX_SECTIONS {
            return Err(DiagnosticError::InvalidSectionCount);
        }
        for selector in &self.selectors {
            if let DiagnosticSelector::TimeWindow(range) = selector {
                range.validate(self.policy.max_time_window_ms)?;
            }
        }
        Ok(())
    }
}

#[async_trait]
pub trait DiagnosticProvider: Send + Sync {
    async fn diagnostics(
        &self,
        request: &DiagnosticRequest,
        section: DiagnosticSectionId,
    ) -> Result<DiagnosticSection, DiagnosticError>;
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IncidentBundleSchemaVersion(u16);

impl IncidentBundleSchemaVersion {
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BundleProducer {
    Client,
    Server,
    Combined,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BundleBoundary {
    pub authority_epoch: AuthorityEpoch,
    pub journal_sequence: Sequence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BuildIdentity {
    pub version: String,
    pub git_commit: Option<String>,
    pub build_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RuntimeInventory {
    pub producer_build: BuildIdentity,
    pub operating_system: String,
    pub architecture: String,
    pub adapter_kind: String,
    pub adapter_version: String,
    pub protocol_version: u16,
    pub schema_versions: BTreeMap<String, u32>,
    pub capability_ids: BTreeSet<u32>,
    pub authority_epoch: Option<AuthorityEpoch>,
    pub config_generation: Option<u64>,
    pub cargo_lock_digest: Option<[u8; 32]>,
    pub enabled_features: BTreeSet<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationForensicView {
    pub operation_id: OperationId,
    pub correlation_id: Option<CorrelationId>,
    pub client_outbox_state: Option<String>,
    pub ledger_status: Option<String>,
    pub committed_sequence: Option<Sequence>,
    pub payload_digest: Option<[u8; 32]>,
    pub journal_event_ids: Vec<Uuid>,
    pub audit_event_ids: Vec<Uuid>,
    pub related_job_ids: Vec<JobId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EntityForensicView {
    pub entity: EntityRef,
    pub authoritative_version: Option<u64>,
    pub client_version: Option<u64>,
    pub tombstoned: bool,
    pub scope_ids: Vec<SyncScopeId>,
    pub provenance_digests: Vec<[u8; 32]>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ScopeForensicView {
    pub scope_id: SyncScopeId,
    pub scope_version: u64,
    pub scope_generation: u64,
    pub projection_schema_version: u32,
    pub client_cursor: Option<Sequence>,
    pub journal_floor: Sequence,
    pub snapshot_boundary: Option<BundleBoundary>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeviceForensicView {
    pub device_id: DeviceId,
    pub status: String,
    pub client_build_id: String,
    pub last_seen_unix_ms: u64,
    pub last_authority_epoch: Option<AuthorityEpoch>,
    pub local_store_generation: u64,
    pub scope_watermarks: BTreeMap<SyncScopeId, Sequence>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthorityForensicView {
    pub authority_id: String,
    pub authority_epoch: AuthorityEpoch,
    pub current_sequence: Sequence,
    pub promotion_history_digests: Vec<[u8; 32]>,
    pub checkpoint_digests: Vec<[u8; 32]>,
    pub fork_detected: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobForensicView {
    pub job_id: JobId,
    pub payload_schema_version: u16,
    pub state: String,
    pub attempts: u32,
    pub latest_fence: Option<u64>,
    pub provider_outcome_digest: Option<[u8; 32]>,
    pub side_effect_intent_digest: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ForensicView {
    Operation(Box<OperationForensicView>),
    Entity(EntityForensicView),
    Scope(ScopeForensicView),
    Device(DeviceForensicView),
    Authority(AuthorityForensicView),
    Job(JobForensicView),
}

#[async_trait]
pub trait ForensicQueryProvider: Send + Sync {
    async fn query(
        &self,
        tenant_id: TenantId,
        selector: &DiagnosticSelector,
        max_records: u32,
    ) -> Result<ForensicView, DiagnosticError>;
}

impl RuntimeInventory {
    pub fn validate(&self) -> Result<(), DiagnosticError> {
        for value in [
            &self.producer_build.version,
            &self.producer_build.build_id,
            &self.operating_system,
            &self.architecture,
            &self.adapter_kind,
            &self.adapter_version,
        ] {
            if value.trim().is_empty() || value.len() > 512 {
                return Err(DiagnosticError::InvalidInventory);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ClockSource {
    ClientWallClock,
    ServerAuthorityTime,
    MonotonicRelative,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TimelineSource {
    Client,
    Authority,
    Journal,
    Audit,
    Job,
    Adapter,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TimelineRefs {
    pub operation_id: Option<OperationId>,
    pub correlation_id: Option<CorrelationId>,
    pub job_id: Option<JobId>,
    pub scope_id: Option<SyncScopeId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TimelineEvent {
    pub source: TimelineSource,
    pub confidence: EvidenceConfidence,
    pub logical_order: Option<u64>,
    pub timestamp: u64,
    pub clock_source: ClockSource,
    pub kind: String,
    pub references: TimelineRefs,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SectionCompleteness {
    pub requested: BTreeSet<DiagnosticSectionId>,
    pub collected: BTreeSet<DiagnosticSectionId>,
    pub missing: BTreeMap<DiagnosticSectionId, MissingSectionReason>,
}

impl SectionCompleteness {
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.missing.is_empty() && self.requested == self.collected
    }

    pub fn validate(&self) -> Result<(), DiagnosticError> {
        if self.requested.is_empty() || !self.collected.is_subset(&self.requested) {
            return Err(DiagnosticError::InvalidCompleteness);
        }
        let accounted = self
            .collected
            .union(&self.missing.keys().copied().collect())
            .copied()
            .collect::<BTreeSet<_>>();
        if accounted != self.requested {
            return Err(DiagnosticError::InvalidCompleteness);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum BundleState {
    Planning,
    Collecting,
    Sanitizing,
    Verifying,
    Ready,
    ReadyWithWarnings,
    Failed,
    Expired,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum IncidentState {
    Open,
    Investigating,
    Mitigated,
    Resolved,
    Closed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IncidentRecord {
    pub incident_id: IncidentId,
    pub tenant_id: TenantId,
    pub incident_class: IncidentClass,
    pub state: IncidentState,
    pub opened_at_unix_ms: u64,
    pub summary_digest: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IncidentBundleRecord {
    pub bundle_id: IncidentBundleId,
    pub incident_id: IncidentId,
    pub tenant_id: TenantId,
    pub mode: DiagnosticMode,
    pub state: BundleState,
    pub artifact_ref: Option<String>,
    pub digest: Option<[u8; 32]>,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub created_by: String,
    pub durable_job_id: Option<JobId>,
}

pub trait IncidentRegistry: Send + Sync {
    fn put_incident(&self, incident: IncidentRecord) -> Result<(), DiagnosticError>;
    fn put_bundle(&self, bundle: IncidentBundleRecord) -> Result<(), DiagnosticError>;
    fn incident(&self, id: IncidentId) -> Result<Option<IncidentRecord>, DiagnosticError>;
    fn bundle(&self, id: IncidentBundleId)
    -> Result<Option<IncidentBundleRecord>, DiagnosticError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BundleRetentionDecision {
    RetainForLegalHold,
    RetainUntilExpiry,
    ExpireAndDelete,
}

#[must_use]
pub const fn evaluate_bundle_retention(
    expires_at_unix_ms: u64,
    now_unix_ms: u64,
    legal_hold_active: bool,
) -> BundleRetentionDecision {
    if legal_hold_active {
        BundleRetentionDecision::RetainForLegalHold
    } else if now_unix_ms < expires_at_unix_ms {
        BundleRetentionDecision::RetainUntilExpiry
    } else {
        BundleRetentionDecision::ExpireAndDelete
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IncidentBundleManifest {
    pub bundle_id: IncidentBundleId,
    pub incident_id: IncidentId,
    pub tenant_id: TenantId,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub producer: BundleProducer,
    pub incident_class: IncidentClass,
    pub selectors: Vec<DiagnosticSelector>,
    pub schema_version: IncidentBundleSchemaVersion,
    pub mode: DiagnosticMode,
    pub boundary: Option<BundleBoundary>,
    pub completeness: SectionCompleteness,
    pub content_digest: [u8; 32],
    pub encryption_key_id: Option<String>,
    pub signature_key_id: Option<String>,
    pub state: BundleState,
}

impl IncidentBundleManifest {
    pub fn validate(&self, policy: &DiagnosticPolicy) -> Result<(), DiagnosticError> {
        // Deployment code applies the production-mode guard while authorizing the request. A
        // manifest can also describe an explicitly non-production Developer bundle.
        policy.validate(false)?;
        if self.schema_version.get() == 0
            || self.selectors.is_empty()
            || self.selectors.len() > MAX_SELECTORS
            || self.created_at_unix_ms >= self.expires_at_unix_ms
        {
            return Err(DiagnosticError::InvalidManifest);
        }
        self.completeness.validate()?;
        if policy.require_encryption.is_required()
            && self.encryption_key_id.as_deref().is_none_or(str::is_empty)
        {
            return Err(DiagnosticError::EncryptionRequired);
        }
        if policy.require_signature.is_required()
            && self.signature_key_id.as_deref().is_none_or(str::is_empty)
        {
            return Err(DiagnosticError::SignatureRequired);
        }
        if self.state == BundleState::Ready && !self.completeness.is_complete() {
            return Err(DiagnosticError::InvalidCompleteness);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BundlePlan {
    pub request: DiagnosticRequest,
    pub estimated_bytes: u64,
    pub sensitivity: BTreeSet<DataClassification>,
    pub required_approval: bool,
    pub plan_digest: [u8; 32],
}

impl BundlePlan {
    pub fn new(
        request: DiagnosticRequest,
        estimated_bytes: u64,
        sensitivity: BTreeSet<DataClassification>,
        production: bool,
    ) -> Result<Self, DiagnosticError> {
        request.validate(production)?;
        if estimated_bytes > request.policy.max_bytes {
            return Err(DiagnosticError::LimitExceeded("estimated bundle bytes"));
        }
        let required_approval = request.policy.mode == DiagnosticMode::Forensic
            || request.policy.include_payloads.is_included()
            || sensitivity.contains(&DataClassification::Restricted);
        let plan_digest = digest_plan(&request, estimated_bytes, &sensitivity);
        Ok(Self {
            request,
            estimated_bytes,
            sensitivity,
            required_approval,
            plan_digest,
        })
    }

    pub fn verify_binding(&self) -> Result<(), DiagnosticError> {
        if self.plan_digest != digest_plan(&self.request, self.estimated_bytes, &self.sensitivity) {
            return Err(DiagnosticError::PlanDigestMismatch);
        }
        Ok(())
    }
}

fn digest_plan(
    request: &DiagnosticRequest,
    estimated_bytes: u64,
    sensitivity: &BTreeSet<DataClassification>,
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aequora:incident-plan:v1");
    hasher.update(request.incident_id.as_uuid().as_bytes());
    hasher.update(request.tenant_id.as_uuid().as_bytes());
    hasher.update(&estimated_bytes.to_le_bytes());
    hasher.update(&(request.selectors.len() as u64).to_le_bytes());
    hasher.update(&(request.requested_sections.len() as u64).to_le_bytes());
    for class in sensitivity {
        hasher.update(&[*class as u8]);
    }
    *hasher.finalize().as_bytes()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HashEntry {
    pub relative_path: String,
    pub digest: [u8; 32],
    pub size: u64,
}

impl HashEntry {
    pub fn validate_path(&self) -> Result<(), DiagnosticError> {
        validate_relative_path(&self.relative_path)
    }

    #[must_use]
    pub fn from_bytes(relative_path: String, bytes: &[u8]) -> Self {
        Self {
            relative_path,
            digest: *blake3::hash(bytes).as_bytes(),
            size: bytes.len() as u64,
        }
    }

    pub fn verify(&self, bytes: &[u8]) -> Result<(), DiagnosticError> {
        self.validate_path()?;
        if self.size != bytes.len() as u64 || self.digest != *blake3::hash(bytes).as_bytes() {
            return Err(DiagnosticError::HashMismatch(self.relative_path.clone()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HashInventory {
    pub entries: Vec<HashEntry>,
}

impl HashInventory {
    pub fn validate(&self, policy: &DiagnosticPolicy) -> Result<(), DiagnosticError> {
        if self.entries.len() > policy.max_files as usize {
            return Err(DiagnosticError::LimitExceeded("file count"));
        }
        let mut paths = BTreeSet::new();
        let mut total = 0_u64;
        for entry in &self.entries {
            entry.validate_path()?;
            if !paths.insert(entry.relative_path.as_str()) {
                return Err(DiagnosticError::DuplicatePath(entry.relative_path.clone()));
            }
            total = total
                .checked_add(entry.size)
                .ok_or(DiagnosticError::LimitExceeded("uncompressed bytes"))?;
        }
        if total > policy.max_bytes {
            return Err(DiagnosticError::LimitExceeded("uncompressed bytes"));
        }
        Ok(())
    }

    #[must_use]
    pub fn canonical_digest(&self) -> [u8; 32] {
        let mut entries = self.entries.iter().collect::<Vec<_>>();
        entries.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"aequora:incident-bundle:v1");
        for entry in entries {
            hasher.update(&(entry.relative_path.len() as u64).to_le_bytes());
            hasher.update(entry.relative_path.as_bytes());
            hasher.update(&entry.size.to_le_bytes());
            hasher.update(&entry.digest);
        }
        *hasher.finalize().as_bytes()
    }
}

pub fn validate_relative_path(path: &str) -> Result<(), DiagnosticError> {
    if path.is_empty()
        || path.len() > MAX_RELATIVE_PATH_BYTES
        || path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\0')
        || path
            .split(['/', '\\'])
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        || path.as_bytes().get(1) == Some(&b':')
    {
        return Err(DiagnosticError::UnsafePath(path.to_owned()));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArchiveObservation {
    pub compressed_bytes: u64,
    pub uncompressed_bytes: u64,
    pub file_count: u32,
    pub maximum_depth: u16,
    pub contains_link: bool,
}

impl ArchiveObservation {
    pub fn validate(self, policy: &DiagnosticPolicy) -> Result<(), DiagnosticError> {
        if self.compressed_bytes > policy.max_bytes
            || self.uncompressed_bytes > policy.max_bytes
            || self.file_count > policy.max_files
            || self.maximum_depth > policy.max_archive_depth
            || self.contains_link
        {
            return Err(DiagnosticError::UnsafeArchive);
        }
        Ok(())
    }
}

pub trait BundleSignatureVerifier: Send + Sync {
    fn verify_signature(
        &self,
        key_id: &str,
        digest: &[u8; 32],
        signature: &[u8],
    ) -> Result<(), DiagnosticError>;
}

pub trait BundleEncryptor: Send + Sync {
    fn encrypt(&self, recipient_key_id: &str, plaintext: &[u8])
    -> Result<Vec<u8>, DiagnosticError>;
}

pub trait BundleSigner: Send + Sync {
    fn sign(&self, key_id: &str, digest: &[u8; 32]) -> Result<Vec<u8>, DiagnosticError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtectedBundle {
    pub ciphertext: Vec<u8>,
    pub plaintext_digest: [u8; 32],
    pub encryption_key_id: String,
    pub signature: Option<Vec<u8>>,
    pub signature_key_id: Option<String>,
}

pub fn protect_bundle(
    plaintext: &[u8],
    policy: &DiagnosticPolicy,
    recipient_key_id: &str,
    encryptor: &dyn BundleEncryptor,
    signature_key_id: Option<&str>,
    signer: Option<&dyn BundleSigner>,
) -> Result<ProtectedBundle, DiagnosticError> {
    policy.validate(true)?;
    if plaintext.len() as u64 > policy.max_bytes || recipient_key_id.trim().is_empty() {
        return Err(DiagnosticError::LimitExceeded("bundle plaintext"));
    }
    let plaintext_digest = *blake3::hash(plaintext).as_bytes();
    let ciphertext = encryptor.encrypt(recipient_key_id, plaintext)?;
    if ciphertext.len() as u64 > policy.max_bytes {
        return Err(DiagnosticError::LimitExceeded("bundle ciphertext"));
    }
    let signature = if policy.require_signature.is_required() {
        let key_id = signature_key_id.ok_or(DiagnosticError::SignatureRequired)?;
        Some(
            signer
                .ok_or(DiagnosticError::SignatureVerifierUnavailable)?
                .sign(key_id, &plaintext_digest)?,
        )
    } else {
        None
    };
    Ok(ProtectedBundle {
        ciphertext,
        plaintext_digest,
        encryption_key_id: recipient_key_id.to_owned(),
        signature,
        signature_key_id: signature_key_id.map(str::to_owned),
    })
}

#[derive(Clone, Debug)]
pub struct BundleVerification<'a> {
    pub manifest: &'a IncidentBundleManifest,
    pub inventory: &'a HashInventory,
    pub signature: Option<&'a [u8]>,
}

pub fn verify_bundle(
    verification: &BundleVerification<'_>,
    policy: &DiagnosticPolicy,
    signature_verifier: Option<&dyn BundleSignatureVerifier>,
) -> Result<(), DiagnosticError> {
    verification.manifest.validate(policy)?;
    verification.inventory.validate(policy)?;
    let digest = verification.inventory.canonical_digest();
    if verification.manifest.content_digest != digest {
        return Err(DiagnosticError::ContentDigestMismatch);
    }
    if policy.require_signature.is_required() {
        let key_id = verification
            .manifest
            .signature_key_id
            .as_deref()
            .ok_or(DiagnosticError::SignatureRequired)?;
        let signature = verification
            .signature
            .ok_or(DiagnosticError::SignatureRequired)?;
        signature_verifier
            .ok_or(DiagnosticError::SignatureVerifierUnavailable)?
            .verify_signature(key_id, &digest, signature)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ReproductionLevel {
    MetadataOnly,
    ProtocolReplay,
    DomainReplay,
    FullLocalSimulation,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum FaultAction {
    DropResponse,
    DelayResponse { millis: u64 },
    DuplicateRequest,
    CrashAfterCommit,
    ReorderMessage,
    RestartClient,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReplayManifest {
    pub level: ReproductionLevel,
    pub seed: u64,
    pub handler_version: String,
    pub expected_plan_digest: [u8; 32],
    pub expected_outcome_digest: Option<[u8; 32]>,
    pub faults: Vec<FaultAction>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SimulatedSideEffectIntent {
    pub kind: String,
    pub payload_digest: [u8; 32],
}

pub trait ReplaySandbox {
    fn replay(&mut self, manifest: &ReplayManifest) -> Result<ReplayOutcome, DiagnosticError>;

    fn is_production_attached(&self) -> bool;
}

pub trait FailureTraceMinimizer {
    fn still_reproduces(&mut self, faults: &[FaultAction]) -> Result<bool, DiagnosticError>;
}

pub fn minimize_fault_script(
    faults: &[FaultAction],
    verifier: &mut dyn FailureTraceMinimizer,
) -> Result<Vec<FaultAction>, DiagnosticError> {
    let mut minimized = faults.to_vec();
    let mut index = 0;
    while index < minimized.len() {
        let mut candidate = minimized.clone();
        candidate.remove(index);
        if verifier.still_reproduces(&candidate)? {
            minimized = candidate;
        } else {
            index += 1;
        }
    }
    Ok(minimized)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReplayOutcome {
    pub plan_digest: [u8; 32],
    pub outcome_digest: [u8; 32],
    pub simulated_side_effects: Vec<SimulatedSideEffectIntent>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReplayReport {
    pub exact_handler: bool,
    pub plan_matches: bool,
    pub outcome_matches: Option<bool>,
    pub simulated_side_effect_count: usize,
}

pub fn replay_in_sandbox(
    sandbox: &mut dyn ReplaySandbox,
    manifest: &ReplayManifest,
    available_handler_version: &str,
) -> Result<ReplayReport, DiagnosticError> {
    if sandbox.is_production_attached() {
        return Err(DiagnosticError::ProductionReplayForbidden);
    }
    let exact_handler = manifest.handler_version == available_handler_version;
    if !exact_handler {
        return Ok(ReplayReport {
            exact_handler: false,
            plan_matches: false,
            outcome_matches: None,
            simulated_side_effect_count: 0,
        });
    }
    let outcome = sandbox.replay(manifest)?;
    Ok(ReplayReport {
        exact_handler,
        plan_matches: outcome.plan_digest == manifest.expected_plan_digest,
        outcome_matches: manifest
            .expected_outcome_digest
            .map(|expected| outcome.outcome_digest == expected),
        simulated_side_effect_count: outcome.simulated_side_effects.len(),
    })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RootCauseClassification {
    TransportLossAfterCommit,
    ClientReconcileFailure,
    CursorExpired,
    ProtocolIncompatible,
    AuthorityChanged,
    ProviderAmbiguity,
    InsufficientEvidence,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ConclusionConfidence {
    Confirmed,
    Likely,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EvidenceRef {
    pub section: DiagnosticSectionId,
    pub record_kind: String,
    pub confidence: EvidenceConfidence,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IncidentSummary {
    pub classification: RootCauseClassification,
    pub confidence: ConclusionConfidence,
    pub supporting_refs: Vec<EvidenceRef>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ClientSendState {
    NeverSent,
    Sent,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ClientReconcileState {
    Pending,
    Reconciled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OperationExplanationInput {
    pub client_send_state: ClientSendState,
    pub client_reconcile_state: ClientReconcileState,
    pub authority_committed_sequence: Option<Sequence>,
    pub client_cursor: Option<Sequence>,
}

#[must_use]
pub fn explain_operation(input: OperationExplanationInput) -> IncidentSummary {
    let mut refs = Vec::new();
    if input.client_send_state == ClientSendState::Sent {
        refs.push(EvidenceRef {
            section: DiagnosticSectionId::Outbox,
            record_kind: "outbox-state".to_owned(),
            confidence: EvidenceConfidence::DurableLocal,
        });
    }
    if let Some(committed) = input.authority_committed_sequence {
        refs.push(EvidenceRef {
            section: DiagnosticSectionId::Ledger,
            record_kind: "committed-operation".to_owned(),
            confidence: EvidenceConfidence::Authoritative,
        });
        let cursor_is_behind = input.client_cursor.is_none_or(|cursor| cursor < committed);
        if input.client_send_state == ClientSendState::Sent
            && input.client_reconcile_state == ClientReconcileState::Pending
            && cursor_is_behind
        {
            return IncidentSummary {
                classification: RootCauseClassification::TransportLossAfterCommit,
                confidence: ConclusionConfidence::Confirmed,
                supporting_refs: refs,
            };
        }
    }
    IncidentSummary {
        classification: RootCauseClassification::InsufficientEvidence,
        confidence: ConclusionConfidence::Unknown,
        supporting_refs: refs,
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum DiagnosticError {
    #[error("diagnostic limits must be non-zero")]
    InvalidLimits,
    #[error("diagnostic time range is invalid")]
    InvalidTimeRange,
    #[error("diagnostic selector count is invalid")]
    InvalidSelectorCount,
    #[error("diagnostic section count is invalid")]
    InvalidSectionCount,
    #[error("diagnostic detail key is invalid")]
    InvalidDetailKey,
    #[error("diagnostic record kind is invalid")]
    InvalidRecordKind,
    #[error("diagnostic schema version is invalid")]
    InvalidSchemaVersion,
    #[error("runtime inventory is invalid")]
    InvalidInventory,
    #[error("bundle manifest is invalid")]
    InvalidManifest,
    #[error("bundle completeness declaration is invalid")]
    InvalidCompleteness,
    #[error("developer diagnostic mode is disabled in production")]
    DeveloperModeInProduction,
    #[error("bundle encryption is required")]
    EncryptionRequired,
    #[error("bundle signature is required")]
    SignatureRequired,
    #[error("bundle signature verifier is unavailable")]
    SignatureVerifierUnavailable,
    #[error("bundle plan digest does not match the reviewed plan")]
    PlanDigestMismatch,
    #[error("bundle content digest does not match its inventory")]
    ContentDigestMismatch,
    #[error("bundle path is unsafe: {0}")]
    UnsafePath(String),
    #[error("bundle contains a duplicate path: {0}")]
    DuplicatePath(String),
    #[error("bundle file hash mismatch: {0}")]
    HashMismatch(String),
    #[error("archive violates extraction bounds")]
    UnsafeArchive,
    #[error("diagnostic limit exceeded: {0}")]
    LimitExceeded(&'static str),
    #[error("production-connected replay is forbidden")]
    ProductionReplayForbidden,
    #[error("diagnostic provider failed: {0}")]
    Provider(String),
    #[error("signature verification failed")]
    SignatureInvalid,
    #[error("bundle encryption failed")]
    EncryptionFailed,
    #[error("incident registry is unavailable")]
    RegistryUnavailable,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(at: u64, text: &str) -> DiagnosticEvent {
        DiagnosticEvent {
            event_id: DiagnosticEventId::new(),
            kind: DiagnosticEventKind::Error,
            occurred_at_unix_ms: at,
            correlation_id: None,
            operation_id: None,
            scope_id: None,
            details: BTreeMap::from([(
                "message".to_owned(),
                ClassifiedValue {
                    classification: DataClassification::Operational,
                    value: DiagnosticValue::Text(text.to_owned()),
                },
            )]),
        }
    }

    #[test]
    fn ring_enforces_event_byte_and_age_bounds() {
        let mut ring = DiagnosticRing::new(DiagnosticRingLimits {
            max_events: 2,
            max_bytes: 1_024,
            max_age_ms: 10,
        })
        .unwrap_or_else(|error| panic!("valid ring: {error}"));
        ring.push(event(1, "first"))
            .unwrap_or_else(|error| panic!("first: {error}"));
        ring.push(event(20, "second"))
            .unwrap_or_else(|error| panic!("second: {error}"));
        assert_eq!(ring.snapshot().len(), 1);
    }

    #[test]
    fn sanitizer_never_exports_secrets_and_digests_restricted_values() {
        let section = DiagnosticSection {
            id: DiagnosticSectionId::Runtime,
            schema_version: 1,
            confidence: EvidenceConfidence::BestEffort,
            records: vec![DiagnosticRecord {
                kind: "config".to_owned(),
                confidence: EvidenceConfidence::BestEffort,
                fields: BTreeMap::from([
                    (
                        "token".to_owned(),
                        ClassifiedValue {
                            classification: DataClassification::Secret,
                            value: DiagnosticValue::Text("access-token-value".to_owned()),
                        },
                    ),
                    (
                        "email".to_owned(),
                        ClassifiedValue {
                            classification: DataClassification::Restricted,
                            value: DiagnosticValue::Text("person@example.test".to_owned()),
                        },
                    ),
                ]),
            }],
            truncated_records: 0,
        };
        let sanitized = DefaultDiagnosticSanitizer
            .sanitize(section, &DiagnosticPolicy::support_minimal())
            .unwrap_or_else(|error| panic!("sanitize: {error}"));
        let fields = &sanitized.records[0].fields;
        assert_eq!(fields["token"].value, DiagnosticValue::Redacted);
        assert!(matches!(fields["email"].value, DiagnosticValue::Digest(_)));
    }

    #[test]
    fn path_and_archive_bombs_are_rejected() {
        assert!(validate_relative_path("../secret").is_err());
        assert!(validate_relative_path("/absolute").is_err());
        assert!(validate_relative_path("metadata/safe.ron").is_ok());
        assert!(
            ArchiveObservation {
                compressed_bytes: 1,
                uncompressed_bytes: 20 * 1024 * 1024,
                file_count: 1,
                maximum_depth: 1,
                contains_link: false,
            }
            .validate(&DiagnosticPolicy::support_minimal())
            .is_err()
        );
    }

    #[test]
    fn changed_file_fails_hash_verification() {
        let entry = HashEntry::from_bytes("timeline.postcard".to_owned(), b"before");
        assert!(entry.verify(b"after").is_err());
    }

    #[test]
    fn partial_collection_is_explicit() {
        let requested = BTreeSet::from([DiagnosticSectionId::Ledger, DiagnosticSectionId::Logs]);
        let completeness = SectionCompleteness {
            requested,
            collected: BTreeSet::from([DiagnosticSectionId::Ledger]),
            missing: BTreeMap::from([(
                DiagnosticSectionId::Logs,
                MissingSectionReason::CollectionFailed,
            )]),
        };
        assert!(completeness.validate().is_ok());
        assert!(!completeness.is_complete());
    }

    #[test]
    fn committed_operation_with_lost_response_is_explained() {
        let summary = explain_operation(OperationExplanationInput {
            client_send_state: ClientSendState::Sent,
            client_reconcile_state: ClientReconcileState::Pending,
            authority_committed_sequence: Some(Sequence(8_821)),
            client_cursor: Some(Sequence(8_819)),
        });
        assert_eq!(
            summary.classification,
            RootCauseClassification::TransportLossAfterCommit
        );
        assert_eq!(summary.confidence, ConclusionConfidence::Confirmed);
    }

    struct ProductionSandbox;

    impl ReplaySandbox for ProductionSandbox {
        fn replay(&mut self, _manifest: &ReplayManifest) -> Result<ReplayOutcome, DiagnosticError> {
            Err(DiagnosticError::ProductionReplayForbidden)
        }

        fn is_production_attached(&self) -> bool {
            true
        }
    }

    #[test]
    fn replay_cannot_attach_to_production() {
        let mut sandbox = ProductionSandbox;
        let manifest = ReplayManifest {
            level: ReproductionLevel::DomainReplay,
            seed: 1,
            handler_version: "v1".to_owned(),
            expected_plan_digest: [0; 32],
            expected_outcome_digest: None,
            faults: vec![FaultAction::DropResponse],
        };
        assert_eq!(
            replay_in_sandbox(&mut sandbox, &manifest, "v1"),
            Err(DiagnosticError::ProductionReplayForbidden)
        );
    }

    struct TestEncryptor;

    impl BundleEncryptor for TestEncryptor {
        fn encrypt(
            &self,
            recipient_key_id: &str,
            plaintext: &[u8],
        ) -> Result<Vec<u8>, DiagnosticError> {
            let mut protected = recipient_key_id.as_bytes().to_vec();
            protected.extend(plaintext.iter().map(|byte| byte ^ 0xA5));
            Ok(protected)
        }
    }

    struct TestSigner;

    impl BundleSigner for TestSigner {
        fn sign(&self, key_id: &str, digest: &[u8; 32]) -> Result<Vec<u8>, DiagnosticError> {
            let mut signature = key_id.as_bytes().to_vec();
            signature.extend_from_slice(digest);
            Ok(signature)
        }
    }

    #[test]
    fn forensic_bundle_requires_encryption_and_signature_boundaries() {
        let bundle = protect_bundle(
            b"sanitized evidence",
            &DiagnosticPolicy::forensic(1_024),
            "recipient-key",
            &TestEncryptor,
            Some("signing-key"),
            Some(&TestSigner),
        )
        .unwrap_or_else(|error| panic!("protect forensic bundle: {error}"));
        assert_ne!(bundle.ciphertext, b"sanitized evidence");
        assert!(bundle.signature.is_some());
        assert_eq!(bundle.signature_key_id.as_deref(), Some("signing-key"));
    }

    #[test]
    fn retention_honors_legal_hold_before_expiry() {
        assert_eq!(
            evaluate_bundle_retention(10, 20, true),
            BundleRetentionDecision::RetainForLegalHold
        );
        assert_eq!(
            evaluate_bundle_retention(10, 20, false),
            BundleRetentionDecision::ExpireAndDelete
        );
    }

    struct DropResponsesAreSufficient;

    impl FailureTraceMinimizer for DropResponsesAreSufficient {
        fn still_reproduces(&mut self, faults: &[FaultAction]) -> Result<bool, DiagnosticError> {
            Ok(faults.contains(&FaultAction::DropResponse))
        }
    }

    #[test]
    fn trace_minimization_removes_irrelevant_faults() {
        let minimized = minimize_fault_script(
            &[
                FaultAction::DelayResponse { millis: 20 },
                FaultAction::DropResponse,
                FaultAction::RestartClient,
            ],
            &mut DropResponsesAreSufficient,
        )
        .unwrap_or_else(|error| panic!("minimize trace: {error}"));
        assert_eq!(minimized, vec![FaultAction::DropResponse]);
    }
}
