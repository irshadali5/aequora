//! Deterministic domain-decision inputs, plans, replay artifacts, and side-effect-safe sandboxing.
//!
//! This crate deliberately has no async runtime, network client, filesystem, database adapter, or
//! framework dependency. Production edges capture real-world inputs before decision execution;
//! handlers consume only the canonical values represented here.

#![allow(clippy::missing_errors_doc)]

use aequora_audit::AuditEvent;
use aequora_protocol::{ChangeKind, OperationEnvelope};
use aequora_types::{
    ActorId, CorrelationId, DeviceId, EntityId, EntityRef, EventId, JobId, OperationId, TenantId,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use uuid::Uuid;

/// Current portable replay-bundle format.
pub const REPLAY_BUNDLE_FORMAT_VERSION: u16 = 1;

const MAX_NAME_BYTES: usize = 128;
const MAX_CAPTURED_RESULTS: usize = 64;
const MAX_ALLOCATED_IDS: usize = 256;
const MAX_PLAN_ITEMS: usize = 1_024;
const MAX_ITEM_BYTES: usize = 4 * 1024 * 1024;
const MAX_PLAN_BYTES: usize = 16 * 1024 * 1024;

/// Server-captured authoritative decision time in Unix milliseconds.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct DomainTimestamp(pub u64);

/// Only boundary allowed to obtain authoritative wall time for a deterministic decision.
pub trait DomainClock {
    /// Captures one timestamp. The caller stores and reuses it for the complete operation.
    fn now(&self) -> Result<DomainTimestamp, ReplayError>;
}

/// Production clock boundary. Domain handlers receive its captured output, never this clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemDomainClock;

impl DomainClock for SystemDomainClock {
    fn now(&self) -> Result<DomainTimestamp, ReplayError> {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ReplayError::ClockBeforeEpoch)?
            .as_millis();
        Ok(DomainTimestamp(
            u64::try_from(millis).map_err(|_| ReplayError::ClockOverflow)?,
        ))
    }
}

/// Fixed test/replay clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedClock(pub DomainTimestamp);

impl DomainClock for FixedClock {
    fn now(&self) -> Result<DomainTimestamp, ReplayError> {
        Ok(self.0)
    }
}

/// Stable semantic implementation version, independent from payload schema and Git revisions.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct HandlerVersion(u32);

impl HandlerVersion {
    /// First semantic handler version.
    pub const V1: Self = Self(1);

    /// Creates a non-zero semantic version.
    pub const fn new(value: u32) -> Result<Self, ReplayError> {
        if value == 0 {
            Err(ReplayError::ZeroVersion)
        } else {
            Ok(Self(value))
        }
    }

    /// Returns the numeric version.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Versioned mutable business inputs that can change a handler decision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PolicySnapshot {
    pub policy_version: u32,
    pub config_version: u32,
    pub canonical_digest: [u8; 32],
}

impl PolicySnapshot {
    /// Validates explicit non-zero policy/config identities and a non-empty digest.
    pub fn validate(&self) -> Result<(), ReplayError> {
        if self.policy_version == 0 || self.config_version == 0 {
            return Err(ReplayError::ZeroVersion);
        }
        if self.canonical_digest == [0; 32] {
            return Err(ReplayError::ZeroDigest);
        }
        Ok(())
    }
}

/// Reproducible seed. It is sensitive input and should follow replay retention policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ExecutionSeed([u8; 32]);

impl ExecutionSeed {
    /// Wraps explicitly captured seed bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Deterministically derives a seed from an operation and domain-separated label.
    #[must_use]
    pub fn derive(operation_id: OperationId, label: &str) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"aequora-execution-seed-v1");
        hasher.update(operation_id.as_uuid().as_bytes());
        hasher.update(label.as_bytes());
        Self(*hasher.finalize().as_bytes())
    }

    #[must_use]
    pub const fn as_bytes(self) -> [u8; 32] {
        self.0
    }
}

/// Stateless deterministic pseudo-random derivation. Labels and ordinals are stable domain input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeterministicRandom {
    seed: ExecutionSeed,
}

impl DeterministicRandom {
    #[must_use]
    pub const fn new(seed: ExecutionSeed) -> Self {
        Self { seed }
    }

    /// Derives reproducible bytes without iteration-order-dependent mutable state.
    #[must_use]
    pub fn bytes(self, label: &str, ordinal: u32) -> [u8; 32] {
        derive_bytes(b"aequora-random-v1", self.seed, label, ordinal)
    }

    /// Derives a reproducible integer.
    #[must_use]
    pub fn u64(self, label: &str, ordinal: u32) -> u64 {
        let bytes = self.bytes(label, ordinal);
        u64::from_le_bytes(bytes[..8].try_into().unwrap_or([0; 8]))
    }
}

/// Kind of stable server-derived identifier reserved for a decision.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum AllocatedIdKind {
    Entity,
    Event,
    Job,
}

/// One captured, domain-labelled server-derived identity.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct AllocatedId {
    pub kind: AllocatedIdKind,
    pub label: String,
    pub ordinal: u32,
    pub value: Uuid,
}

/// Deterministic server-derived ID policy.
#[derive(Clone, Copy, Debug)]
pub struct DeterministicIdAllocator {
    operation_id: OperationId,
    seed: ExecutionSeed,
}

impl DeterministicIdAllocator {
    #[must_use]
    pub const fn new(operation_id: OperationId, seed: ExecutionSeed) -> Self {
        Self { operation_id, seed }
    }

    /// Derives and captures a stable ID for one semantic child key.
    pub fn allocate(
        self,
        kind: AllocatedIdKind,
        label: impl Into<String>,
        ordinal: u32,
    ) -> Result<AllocatedId, ReplayError> {
        let label = label.into();
        validate_name(&label)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"aequora-derived-id-v1");
        hasher.update(self.operation_id.as_uuid().as_bytes());
        hasher.update(&self.seed.0);
        hasher.update(&[kind as u8]);
        hasher.update(label.as_bytes());
        hasher.update(&ordinal.to_le_bytes());
        let mut bytes = [0_u8; 16];
        bytes.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
        Ok(AllocatedId {
            kind,
            label,
            ordinal,
            value: Uuid::from_bytes(bytes),
        })
    }

    pub fn entity_id(self, label: &str, ordinal: u32) -> Result<EntityId, ReplayError> {
        Ok(EntityId::from_uuid(
            self.allocate(AllocatedIdKind::Entity, label, ordinal)?
                .value,
        ))
    }

    pub fn event_id(self, label: &str, ordinal: u32) -> Result<EventId, ReplayError> {
        Ok(EventId::from_uuid(
            self.allocate(AllocatedIdKind::Event, label, ordinal)?.value,
        ))
    }

    pub fn job_id(self, label: &str, ordinal: u32) -> Result<JobId, ReplayError> {
        Ok(JobId::from_uuid(
            self.allocate(AllocatedIdKind::Job, label, ordinal)?.value,
        ))
    }
}

/// Canonical external response captured before it becomes new authoritative input.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapturedExternalResult {
    pub provider: String,
    pub result_kind: String,
    pub canonical_payload: Vec<u8>,
    pub digest: [u8; 32],
}

impl CapturedExternalResult {
    pub fn capture(
        provider: impl Into<String>,
        result_kind: impl Into<String>,
        canonical_payload: Vec<u8>,
    ) -> Result<Self, ReplayError> {
        let provider = provider.into();
        let result_kind = result_kind.into();
        validate_name(&provider)?;
        validate_name(&result_kind)?;
        validate_item(&canonical_payload)?;
        let digest = digest_parts(&[
            b"aequora-external-result-v1",
            provider.as_bytes(),
            result_kind.as_bytes(),
            &canonical_payload,
        ]);
        Ok(Self {
            provider,
            result_kind,
            canonical_payload,
            digest,
        })
    }

    pub fn verify(&self) -> Result<(), ReplayError> {
        validate_name(&self.provider)?;
        validate_name(&self.result_kind)?;
        validate_item(&self.canonical_payload)?;
        let expected = digest_parts(&[
            b"aequora-external-result-v1",
            self.provider.as_bytes(),
            self.result_kind.as_bytes(),
            &self.canonical_payload,
        ]);
        if self.digest != expected {
            return Err(ReplayError::DigestMismatch);
        }
        Ok(())
    }
}

/// Complete captured nondeterministic input set for one operation decision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExecutionInputs {
    pub execution_time: DomainTimestamp,
    pub id_seed: ExecutionSeed,
    pub random_seed: Option<ExecutionSeed>,
    pub allocated_ids: Vec<AllocatedId>,
    pub external_results: Vec<CapturedExternalResult>,
    pub policy: PolicySnapshot,
}

impl ExecutionInputs {
    pub fn verify(&self, operation_id: OperationId) -> Result<(), ReplayError> {
        self.policy.validate()?;
        if self.allocated_ids.len() > MAX_ALLOCATED_IDS
            || self.external_results.len() > MAX_CAPTURED_RESULTS
        {
            return Err(ReplayError::LimitExceeded);
        }
        let allocator = DeterministicIdAllocator::new(operation_id, self.id_seed);
        let mut keys = BTreeSet::new();
        for allocated in &self.allocated_ids {
            validate_name(&allocated.label)?;
            if !keys.insert((allocated.kind, allocated.label.as_str(), allocated.ordinal)) {
                return Err(ReplayError::DuplicateInputKey);
            }
            if allocator
                .allocate(allocated.kind, allocated.label.clone(), allocated.ordinal)?
                .value
                != allocated.value
            {
                return Err(ReplayError::AllocatedIdMismatch);
            }
        }
        for result in &self.external_results {
            result.verify()?;
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<[u8; 32], ReplayError> {
        Ok(*blake3::hash(&postcard::to_stdvec(self)?).as_bytes())
    }
}

/// Authenticated principal captured without trusting client identity claims.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OriginPrincipal {
    pub actor_id: ActorId,
    pub tenant_id: TenantId,
    pub device_id: Option<DeviceId>,
}

/// Only source of time, IDs, randomness, policy/config, and external results visible to a handler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionContext {
    pub operation_id: OperationId,
    pub correlation_id: CorrelationId,
    pub principal: OriginPrincipal,
    pub inputs: ExecutionInputs,
}

impl ExecutionContext {
    pub fn new(
        operation_id: OperationId,
        correlation_id: CorrelationId,
        principal: OriginPrincipal,
        inputs: ExecutionInputs,
    ) -> Result<Self, ReplayError> {
        inputs.verify(operation_id)?;
        Ok(Self {
            operation_id,
            correlation_id,
            principal,
            inputs,
        })
    }

    pub fn randomness(&self) -> Result<DeterministicRandom, ReplayError> {
        self.inputs
            .random_seed
            .map(DeterministicRandom::new)
            .ok_or(ReplayError::RandomnessNotCaptured)
    }

    #[must_use]
    pub const fn ids(&self) -> DeterministicIdAllocator {
        DeterministicIdAllocator::new(self.operation_id, self.inputs.id_seed)
    }
}

/// Explicit authoritative state mutation decided by a handler.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlannedMutation {
    pub entity: EntityRef,
    pub change_kind: ChangeKind,
    pub canonical_payload: Vec<u8>,
}

/// Explicit authoritative domain event decided by a handler.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlannedEvent {
    pub event_id: EventId,
    pub event_kind: String,
    pub canonical_payload: Vec<u8>,
}

/// Durable request for an external worker. Planning never performs the external call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SideEffectIntent {
    pub job_id: JobId,
    pub effect_kind: String,
    pub idempotency_key: String,
    pub canonical_payload: Vec<u8>,
}

/// Pure decision result. Persistence adapters commit this atomically after validation.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ExecutionPlan {
    pub mutations: Vec<PlannedMutation>,
    pub events: Vec<PlannedEvent>,
    /// Canonical audit declarations committed atomically when policy requires it.
    pub audit_events: Vec<AuditEvent>,
    pub side_effects: Vec<SideEffectIntent>,
    pub canonical_result: Vec<u8>,
}

impl ExecutionPlan {
    pub fn verify(&self) -> Result<(), ReplayError> {
        if self.mutations.len() > MAX_PLAN_ITEMS
            || self.events.len() > MAX_PLAN_ITEMS
            || self.audit_events.len() > MAX_PLAN_ITEMS
            || self.side_effects.len() > MAX_PLAN_ITEMS
        {
            return Err(ReplayError::LimitExceeded);
        }
        let mut events = BTreeSet::new();
        for mutation in &self.mutations {
            validate_item(&mutation.canonical_payload)?;
        }
        for event in &self.events {
            validate_name(&event.event_kind)?;
            validate_item(&event.canonical_payload)?;
            if !events.insert(event.event_id) {
                return Err(ReplayError::DuplicatePlanIdentity);
            }
        }
        let mut audit_events = BTreeSet::new();
        for event in &self.audit_events {
            event.verify()?;
            if !audit_events.insert(event.audit_event_id) {
                return Err(ReplayError::DuplicatePlanIdentity);
            }
        }
        let mut jobs = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for intent in &self.side_effects {
            validate_name(&intent.effect_kind)?;
            validate_name(&intent.idempotency_key)?;
            validate_item(&intent.canonical_payload)?;
            if !jobs.insert(intent.job_id) || !keys.insert(intent.idempotency_key.as_str()) {
                return Err(ReplayError::DuplicatePlanIdentity);
            }
        }
        validate_item(&self.canonical_result)?;
        let encoded = postcard::to_stdvec(self)?;
        if encoded.len() > MAX_PLAN_BYTES {
            return Err(ReplayError::LimitExceeded);
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<ExecutionPlanDigest, ReplayError> {
        self.verify()?;
        Ok(ExecutionPlanDigest(
            *blake3::hash(&postcard::to_stdvec(self)?).as_bytes(),
        ))
    }
}

/// Canonical semantic digest of a decision plan.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ExecutionPlanDigest(pub [u8; 32]);

/// Captured validated operation. Payload schema remains separate from handler version.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CanonicalOperation {
    pub envelope: OperationEnvelope,
}

impl From<OperationEnvelope> for CanonicalOperation {
    fn from(envelope: OperationEnvelope) -> Self {
        Self { envelope }
    }
}

/// Canonical pre-state strategy used by a replay bundle.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ReplayStateRef {
    Embedded {
        canonical_state: Vec<u8>,
        digest: [u8; 32],
    },
    Snapshot {
        reference: String,
        digest: [u8; 32],
    },
    JournalRange {
        start: u64,
        end: u64,
        digest: [u8; 32],
    },
}

impl ReplayStateRef {
    pub fn embedded(canonical_state: Vec<u8>) -> Result<Self, ReplayError> {
        validate_item(&canonical_state)?;
        Ok(Self::Embedded {
            digest: *blake3::hash(&canonical_state).as_bytes(),
            canonical_state,
        })
    }

    pub fn verify(&self) -> Result<(), ReplayError> {
        match self {
            Self::Embedded {
                canonical_state,
                digest,
            } => {
                validate_item(canonical_state)?;
                if blake3::hash(canonical_state).as_bytes() != digest {
                    return Err(ReplayError::DigestMismatch);
                }
            }
            Self::Snapshot { reference, digest } => {
                validate_name(reference)?;
                if *digest == [0; 32] {
                    return Err(ReplayError::ZeroDigest);
                }
            }
            Self::JournalRange { start, end, digest } => {
                if start > end || *digest == [0; 32] {
                    return Err(ReplayError::InvalidStateReference);
                }
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn embedded_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Embedded {
                canonical_state, ..
            } => Some(canonical_state),
            _ => None,
        }
    }
}

/// Supported isolated replay intentions. None grant production write or side-effect authority.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ReplayMode {
    VerifyDecision,
    RebuildProjection,
    Simulate,
    HistoricalDebug,
    MigrationCheck,
}

/// Portable, integrity-bound replay artifact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReplayBundle {
    pub format_version: u16,
    pub operation: CanonicalOperation,
    pub execution_inputs: ExecutionInputs,
    pub pre_state: ReplayStateRef,
    pub handler_version: HandlerVersion,
    pub profile_version: u16,
    pub expected_plan: ExecutionPlanDigest,
    pub bundle_digest: [u8; 32],
}

impl ReplayBundle {
    pub fn build(
        operation: CanonicalOperation,
        execution_inputs: ExecutionInputs,
        pre_state: ReplayStateRef,
        handler_version: HandlerVersion,
        profile_version: u16,
        expected_plan: ExecutionPlanDigest,
    ) -> Result<Self, ReplayError> {
        let mut bundle = Self {
            format_version: REPLAY_BUNDLE_FORMAT_VERSION,
            operation,
            execution_inputs,
            pre_state,
            handler_version,
            profile_version,
            expected_plan,
            bundle_digest: [0; 32],
        };
        bundle.verify_fields()?;
        bundle.bundle_digest = bundle.calculate_digest()?;
        Ok(bundle)
    }

    pub fn verify(&self) -> Result<(), ReplayError> {
        self.verify_fields()?;
        if self.calculate_digest()? != self.bundle_digest {
            return Err(ReplayError::DigestMismatch);
        }
        Ok(())
    }

    pub fn context(&self) -> Result<ExecutionContext, ReplayError> {
        let envelope = &self.operation.envelope;
        ExecutionContext::new(
            envelope.operation_id,
            envelope
                .metadata
                .lineage
                .resolved_for_operation(envelope.operation_id)
                .correlation_id,
            OriginPrincipal {
                actor_id: envelope.actor_id,
                tenant_id: envelope.tenant_id,
                device_id: Some(envelope.device_id),
            },
            self.execution_inputs.clone(),
        )
    }

    fn verify_fields(&self) -> Result<(), ReplayError> {
        if self.format_version != REPLAY_BUNDLE_FORMAT_VERSION {
            return Err(ReplayError::UnsupportedFormat);
        }
        if self.handler_version.get() == 0 || self.profile_version == 0 {
            return Err(ReplayError::ZeroVersion);
        }
        self.execution_inputs
            .verify(self.operation.envelope.operation_id)?;
        self.pre_state.verify()?;
        if self.expected_plan.0 == [0; 32] {
            return Err(ReplayError::ZeroDigest);
        }
        Ok(())
    }

    fn calculate_digest(&self) -> Result<[u8; 32], ReplayError> {
        let encoded = postcard::to_stdvec(&(
            self.format_version,
            &self.operation,
            &self.execution_inputs,
            &self.pre_state,
            self.handler_version,
            self.profile_version,
            self.expected_plan,
        ))?;
        Ok(*blake3::hash(&encoded).as_bytes())
    }
}

/// Pure handler boundary used for first execution, verification, and differential replay.
pub trait ReplayHandler {
    fn version(&self) -> HandlerVersion;

    /// Makes a decision without persistence, network calls, or external side effects.
    fn decide(
        &self,
        operation: &CanonicalOperation,
        canonical_pre_state: &[u8],
        context: &ExecutionContext,
    ) -> Result<ExecutionPlan, ReplayError>;
}

/// Persistence boundary kept separate from pure decision construction.
pub trait PlanCommitter {
    type Outcome;

    /// Atomically validates and commits mutations, events, side-effect intents, and ledger output.
    fn commit(
        &mut self,
        operation_id: OperationId,
        handler_version: HandlerVersion,
        inputs_digest: [u8; 32],
        plan: &ExecutionPlan,
    ) -> Result<Self::Outcome, ReplayError>;
}

/// Side-effect persistence boundary. Replay uses a recorder and never a real worker SDK.
pub trait SideEffectSink {
    fn record(&mut self, intent: &SideEffectIntent) -> Result<(), ReplayError>;
}

/// In-memory replay sink that records intent only.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecordingSideEffectSink {
    pub intents: Vec<SideEffectIntent>,
}

impl SideEffectSink for RecordingSideEffectSink {
    fn record(&mut self, intent: &SideEffectIntent) -> Result<(), ReplayError> {
        if self.intents.len() >= MAX_PLAN_ITEMS {
            return Err(ReplayError::LimitExceeded);
        }
        self.intents.push(intent.clone());
        Ok(())
    }
}

/// Sandbox limits independent from production database configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplaySandboxLimits {
    pub max_pre_state_bytes: usize,
    pub max_plan_bytes: usize,
}

impl Default for ReplaySandboxLimits {
    fn default() -> Self {
        Self {
            max_pre_state_bytes: MAX_ITEM_BYTES,
            max_plan_bytes: MAX_PLAN_BYTES,
        }
    }
}

/// Isolated runner that can only read embedded canonical state and return/compare a plan.
#[derive(Clone, Copy, Debug)]
pub struct ReplaySandbox {
    limits: ReplaySandboxLimits,
}

impl ReplaySandbox {
    #[must_use]
    pub const fn new(limits: ReplaySandboxLimits) -> Self {
        Self { limits }
    }

    pub fn run<H: ReplayHandler>(
        &self,
        mode: ReplayMode,
        bundle: &ReplayBundle,
        handler: &H,
    ) -> Result<ReplayReport, ReplayError> {
        bundle.verify()?;
        if handler.version() != bundle.handler_version {
            return Err(ReplayError::HandlerVersionMismatch);
        }
        let state = bundle
            .pre_state
            .embedded_bytes()
            .ok_or(ReplayError::ExternalStateUnavailable)?;
        if state.len() > self.limits.max_pre_state_bytes {
            return Err(ReplayError::LimitExceeded);
        }
        let context = bundle.context()?;
        let plan = handler.decide(&bundle.operation, state, &context)?;
        let encoded = postcard::to_stdvec(&plan)?;
        if encoded.len() > self.limits.max_plan_bytes {
            return Err(ReplayError::LimitExceeded);
        }
        let actual = plan.digest()?;
        Ok(ReplayReport {
            mode,
            expected: bundle.expected_plan,
            actual,
            outcome: if actual == bundle.expected_plan {
                ReplayOutcome::Equivalent
            } else {
                ReplayOutcome::Diverged
            },
            planned_side_effects: plan.side_effects.len(),
        })
    }

    pub fn compare<A: ReplayHandler, B: ReplayHandler>(
        &self,
        bundle: &ReplayBundle,
        baseline: &A,
        candidate: &B,
    ) -> Result<DifferentialReport, ReplayError> {
        bundle.verify()?;
        let state = bundle
            .pre_state
            .embedded_bytes()
            .ok_or(ReplayError::ExternalStateUnavailable)?;
        let context = bundle.context()?;
        let baseline_digest = baseline
            .decide(&bundle.operation, state, &context)?
            .digest()?;
        let candidate_digest = candidate
            .decide(&bundle.operation, state, &context)?
            .digest()?;
        Ok(DifferentialReport {
            baseline_version: baseline.version(),
            candidate_version: candidate.version(),
            baseline_digest,
            candidate_digest,
            outcome: if baseline_digest == candidate_digest {
                ReplayOutcome::Equivalent
            } else {
                ReplayOutcome::Diverged
            },
        })
    }
}

impl Default for ReplaySandbox {
    fn default() -> Self {
        Self::new(ReplaySandboxLimits::default())
    }
}

/// Result of verifying one handler against a captured expected plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReplayReport {
    pub mode: ReplayMode,
    pub expected: ExecutionPlanDigest,
    pub actual: ExecutionPlanDigest,
    pub outcome: ReplayOutcome,
    pub planned_side_effects: usize,
}

/// Semantic replay comparison category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayOutcome {
    Equivalent,
    Diverged,
}

/// Old/new handler comparison over exactly the same bundle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DifferentialReport {
    pub baseline_version: HandlerVersion,
    pub candidate_version: HandlerVersion,
    pub baseline_digest: ExecutionPlanDigest,
    pub candidate_digest: ExecutionPlanDigest,
    pub outcome: ReplayOutcome,
}

/// Verifies a golden corpus and returns the first divergence.
pub fn verify_corpus<H: ReplayHandler>(
    sandbox: &ReplaySandbox,
    bundles: &[ReplayBundle],
    handler: &H,
) -> Result<Vec<ReplayReport>, ReplayError> {
    let mut reports = Vec::with_capacity(bundles.len());
    for bundle in bundles {
        let report = sandbox.run(ReplayMode::VerifyDecision, bundle, handler)?;
        if report.outcome == ReplayOutcome::Diverged {
            return Err(ReplayError::DecisionDiverged);
        }
        reports.push(report);
    }
    Ok(reports)
}

/// Fail-closed deterministic execution and replay diagnostics.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ReplayError {
    #[error("semantic versions must be non-zero")]
    ZeroVersion,
    #[error("canonical digest must be non-zero")]
    ZeroDigest,
    #[error("name or stable key must contain 1 through 128 bytes")]
    InvalidName,
    #[error("captured or planned data exceeds a hard limit")]
    LimitExceeded,
    #[error("system clock is before the Unix epoch")]
    ClockBeforeEpoch,
    #[error("system clock milliseconds exceed the supported range")]
    ClockOverflow,
    #[error("randomness was requested but no seed was captured")]
    RandomnessNotCaptured,
    #[error("captured input key is duplicated")]
    DuplicateInputKey,
    #[error("captured allocated ID does not match deterministic derivation")]
    AllocatedIdMismatch,
    #[error("plan contains duplicate event, job, or side-effect identity")]
    DuplicatePlanIdentity,
    #[error("canonical digest does not match its contents")]
    DigestMismatch,
    #[error("replay state reference is invalid")]
    InvalidStateReference,
    #[error("replay bundle format is unsupported")]
    UnsupportedFormat,
    #[error("replay requires embedded state that is unavailable in this sandbox")]
    ExternalStateUnavailable,
    #[error("handler semantic version does not match the replay bundle")]
    HandlerVersionMismatch,
    #[error("replayed decision diverged from the captured expected plan")]
    DecisionDiverged,
    #[error("fault injection interrupted plan commit")]
    InjectedFailure,
    #[error("retry attempted to change committed execution inputs or plan")]
    CommittedDecisionDrift,
    #[error("canonical replay encoding failed")]
    Codec,
    #[error("canonical audit declaration is invalid: {0}")]
    Audit(#[from] aequora_audit::AuditError),
}

impl From<postcard::Error> for ReplayError {
    fn from(_: postcard::Error) -> Self {
        Self::Codec
    }
}

fn derive_bytes(domain: &[u8], seed: ExecutionSeed, label: &str, ordinal: u32) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(domain);
    hasher.update(&seed.0);
    hasher.update(label.as_bytes());
    hasher.update(&ordinal.to_le_bytes());
    *hasher.finalize().as_bytes()
}

fn digest_parts(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    for part in parts {
        hasher.update(part);
    }
    *hasher.finalize().as_bytes()
}

fn validate_name(name: &str) -> Result<(), ReplayError> {
    if name.trim().is_empty() || name.len() > MAX_NAME_BYTES {
        Err(ReplayError::InvalidName)
    } else {
        Ok(())
    }
}

fn validate_item(bytes: &[u8]) -> Result<(), ReplayError> {
    if bytes.len() > MAX_ITEM_BYTES {
        Err(ReplayError::LimitExceeded)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_protocol::{OperationKind, OperationMetadata};
    use aequora_types::{
        EntityType, HybridTimestamp, LineageContext, NodeId, ProtocolVersion, SchemaVersion,
    };
    use proptest::prelude::*;

    fn operation() -> CanonicalOperation {
        CanonicalOperation {
            envelope: OperationEnvelope {
                protocol_version: ProtocolVersion::V1,
                operation_id: OperationId::new(),
                tenant_id: TenantId::new(),
                actor_id: ActorId::new(),
                device_id: DeviceId::new(),
                entity: EntityRef {
                    entity_type: EntityType::new(7).unwrap_or_else(|error| panic!("{error}")),
                    entity_id: EntityId::new(),
                },
                base_version: None,
                created_at: HybridTimestamp {
                    physical_ms: 1,
                    logical: 0,
                    node: NodeId::new(),
                },
                schema_version: SchemaVersion(1),
                operation_kind: OperationKind(7),
                payload: vec![4, 2],
                metadata: OperationMetadata {
                    lineage: LineageContext::root(),
                    ..OperationMetadata::default()
                },
            },
        }
    }

    fn inputs(operation_id: OperationId) -> ExecutionInputs {
        let id_seed = ExecutionSeed::derive(operation_id, "ids");
        let allocated = DeterministicIdAllocator::new(operation_id, id_seed)
            .allocate(AllocatedIdKind::Job, "email", 0)
            .unwrap_or_else(|error| panic!("{error}"));
        ExecutionInputs {
            execution_time: DomainTimestamp(1_700_000_000_000),
            id_seed,
            random_seed: Some(ExecutionSeed::derive(operation_id, "random")),
            allocated_ids: vec![allocated],
            external_results: vec![
                CapturedExternalResult::capture("tax-service", "quoted-rate", vec![20])
                    .unwrap_or_else(|error| panic!("{error}")),
            ],
            policy: PolicySnapshot {
                policy_version: 3,
                config_version: 2,
                canonical_digest: *blake3::hash(b"policy-v3-config-v2").as_bytes(),
            },
        }
    }

    #[derive(Clone, Copy)]
    struct FixtureHandler {
        version: HandlerVersion,
        result: u8,
    }

    impl ReplayHandler for FixtureHandler {
        fn version(&self) -> HandlerVersion {
            self.version
        }

        fn decide(
            &self,
            _operation: &CanonicalOperation,
            _canonical_pre_state: &[u8],
            context: &ExecutionContext,
        ) -> Result<ExecutionPlan, ReplayError> {
            let job_id = context.ids().job_id("email", 0)?;
            Ok(ExecutionPlan {
                side_effects: vec![SideEffectIntent {
                    job_id,
                    effect_kind: "send-email".to_owned(),
                    idempotency_key: format!("email:{job_id}"),
                    canonical_payload: vec![self.result],
                }],
                canonical_result: vec![self.result],
                ..ExecutionPlan::default()
            })
        }
    }

    fn bundle(handler: FixtureHandler) -> ReplayBundle {
        let operation = operation();
        let inputs = inputs(operation.envelope.operation_id);
        let context = ExecutionContext::new(
            operation.envelope.operation_id,
            operation.envelope.metadata.lineage.correlation_id,
            OriginPrincipal {
                actor_id: operation.envelope.actor_id,
                tenant_id: operation.envelope.tenant_id,
                device_id: Some(operation.envelope.device_id),
            },
            inputs.clone(),
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let state =
            ReplayStateRef::embedded(vec![1, 2, 3]).unwrap_or_else(|error| panic!("{error}"));
        let plan = handler
            .decide(
                &operation,
                state.embedded_bytes().unwrap_or_default(),
                &context,
            )
            .unwrap_or_else(|error| panic!("{error}"));
        ReplayBundle::build(
            operation,
            inputs,
            state,
            handler.version(),
            1,
            plan.digest().unwrap_or_else(|error| panic!("{error}")),
        )
        .unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn same_bundle_replays_exactly_and_only_records_side_effect_intent() {
        let handler = FixtureHandler {
            version: HandlerVersion::V1,
            result: 9,
        };
        let bundle = bundle(handler);
        let report = ReplaySandbox::default()
            .run(ReplayMode::VerifyDecision, &bundle, &handler)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(report.outcome, ReplayOutcome::Equivalent);
        assert_eq!(report.planned_side_effects, 1);
    }

    #[test]
    fn tampering_inputs_state_or_bundle_fails_closed() {
        let handler = FixtureHandler {
            version: HandlerVersion::V1,
            result: 9,
        };
        let mut tampered_inputs = bundle(handler);
        tampered_inputs.execution_inputs.external_results[0].canonical_payload[0] ^= 1;
        assert_eq!(tampered_inputs.verify(), Err(ReplayError::DigestMismatch));

        let mut tampered_state = bundle(handler);
        if let ReplayStateRef::Embedded {
            canonical_state, ..
        } = &mut tampered_state.pre_state
        {
            canonical_state.push(8);
        }
        assert_eq!(tampered_state.verify(), Err(ReplayError::DigestMismatch));
    }

    #[test]
    fn differential_handler_gate_detects_semantic_change() {
        let baseline = FixtureHandler {
            version: HandlerVersion::V1,
            result: 9,
        };
        let candidate = FixtureHandler {
            version: HandlerVersion::new(2).unwrap_or(HandlerVersion::V1),
            result: 10,
        };
        let report = ReplaySandbox::default()
            .compare(&bundle(baseline), &baseline, &candidate)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(report.outcome, ReplayOutcome::Diverged);
        assert_ne!(report.baseline_digest, report.candidate_digest);
    }

    #[test]
    fn missing_randomness_and_unversioned_policy_are_rejected() {
        let operation = operation();
        let mut inputs = inputs(operation.envelope.operation_id);
        inputs.random_seed = None;
        let context = ExecutionContext::new(
            operation.envelope.operation_id,
            operation.envelope.metadata.lineage.correlation_id,
            OriginPrincipal {
                actor_id: operation.envelope.actor_id,
                tenant_id: operation.envelope.tenant_id,
                device_id: Some(operation.envelope.device_id),
            },
            inputs,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            context.randomness(),
            Err(ReplayError::RandomnessNotCaptured)
        );

        let mut invalid = context.inputs;
        invalid.policy.config_version = 0;
        assert_eq!(
            invalid.verify(operation.envelope.operation_id),
            Err(ReplayError::ZeroVersion)
        );
    }

    proptest! {
        #[test]
        fn deterministic_sources_are_reproducible(label in "[a-z]{1,32}", ordinal in any::<u32>()) {
            let operation_id = OperationId::from_uuid(Uuid::from_u128(42));
            let seed = ExecutionSeed::derive(operation_id, "property");
            prop_assert_eq!(
                DeterministicRandom::new(seed).bytes(&label, ordinal),
                DeterministicRandom::new(seed).bytes(&label, ordinal),
            );
            let allocator = DeterministicIdAllocator::new(operation_id, seed);
            let left = allocator.allocate(AllocatedIdKind::Event, &label, ordinal);
            let right = allocator.allocate(AllocatedIdKind::Event, &label, ordinal);
            prop_assert_eq!(left, right);
        }
    }
}
