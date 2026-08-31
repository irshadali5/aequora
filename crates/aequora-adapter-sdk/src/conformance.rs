//! Public adapter conformance factory contracts and bounded runners.

use crate::AdapterError;
use aequora_conformance::{TestObservation, TestStatus};
use aequora_registry_types::ConformanceTestId;
use async_trait::async_trait;
use std::collections::BTreeMap;

/// Observable local adapter scenarios. Failpoints remain test-only behind the factory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalConformanceCase {
    /// Domain mutation and outbox either both commit or neither commits.
    AtomicDomainAndOutbox,
    /// Process loss and reopen preserve durable intent, cursor, and identity.
    CrashReopen,
    /// Cursor update never commits before its reconcile writes.
    CursorAtomicity,
    /// Migration preserves identity, cursor, conflict, and pending outbox state.
    MigrationPreservation,
    /// Critical intent never reports success under simulated storage exhaustion.
    CriticalDurability,
}

/// Observable authoritative adapter scenarios.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthorityConformanceCase {
    /// Business mutation, version, journal, ledger, and required audit commit atomically.
    AtomicPublication,
    /// Duplicate identifier and same payload return the stored outcome.
    DuplicateReplay,
    /// Duplicate identifier and different canonical digest is rejected.
    PayloadMismatch,
    /// Serialization/deadlock failure is explicit and safely classifiable.
    SerializationBehavior,
    /// Concurrent version compare-and-swap rejects a stale writer.
    VersionCompareAndSwap,
    /// Journal scans expose committed order and the correct retention floor.
    JournalRetention,
}

/// Observable snapshot adapter scenarios.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SnapshotConformanceCase {
    /// Complete verified generation publishes successfully.
    Publish,
    /// Partial generation remains invisible.
    PartialWriteIsolation,
    /// Interrupted staging resumes idempotently.
    Resume,
    /// Corrupt chunks fail verification.
    CorruptChunk,
    /// Activation swaps the complete generation atomically.
    AtomicActivation,
}

/// Observable fencing adapter scenarios.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FencingConformanceCase {
    /// A new owner takes over only after lease expiry.
    LeaseTakeover,
    /// The prior owner cannot write after takeover.
    StaleWriter,
    /// Every takeover increases the fencing token.
    TokenMonotonicity,
}

/// Payload-free result returned by an adapter test probe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeResult {
    /// Whether the exact semantic scenario passed.
    pub passed: bool,
    /// Bounded reference to logs, fixture identity, or deterministic trace.
    pub evidence: String,
}

impl ProbeResult {
    /// Creates a passing probe with non-empty evidence.
    #[must_use]
    pub fn passed(evidence: impl Into<String>) -> Self {
        Self {
            passed: true,
            evidence: evidence.into(),
        }
    }

    /// Creates a failing probe with non-empty evidence.
    #[must_use]
    pub fn failed(evidence: impl Into<String>) -> Self {
        Self {
            passed: false,
            evidence: evidence.into(),
        }
    }
}

/// Clean local-store fixture factory.
#[async_trait]
pub trait LocalStoreFactory: Send + Sync {
    /// Concrete test fixture.
    type Store: LocalConformanceStore;

    /// Creates an isolated empty physical store using test-only failpoint configuration.
    async fn create_clean(&self) -> Result<Self::Store, AdapterError>;
}

/// Local adapter observation surface used only by the conformance runner.
#[async_trait]
pub trait LocalConformanceStore: Send + Sync {
    /// Executes one scenario and returns observable semantic evidence.
    async fn observe(&self, case: LocalConformanceCase) -> Result<ProbeResult, AdapterError>;
}

/// Clean authoritative-store fixture factory.
#[async_trait]
pub trait AuthorityStoreFactory: Send + Sync {
    /// Concrete test fixture.
    type Store: AuthorityConformanceStore;

    /// Creates an isolated empty authority database.
    async fn create_clean(&self) -> Result<Self::Store, AdapterError>;
}

/// Authoritative adapter observation surface used only by the conformance runner.
#[async_trait]
pub trait AuthorityConformanceStore: Send + Sync {
    /// Executes one scenario and returns observable semantic evidence.
    async fn observe(&self, case: AuthorityConformanceCase) -> Result<ProbeResult, AdapterError>;
}

/// Snapshot fixture factory.
#[async_trait]
pub trait SnapshotStoreFactory: Send + Sync {
    /// Concrete test fixture.
    type Store: SnapshotConformanceStore;

    /// Creates an isolated empty artifact store.
    async fn create_clean(&self) -> Result<Self::Store, AdapterError>;
}

/// Snapshot observation surface used only by conformance.
#[async_trait]
pub trait SnapshotConformanceStore: Send + Sync {
    /// Executes one scenario and returns observable semantic evidence.
    async fn observe(&self, case: SnapshotConformanceCase) -> Result<ProbeResult, AdapterError>;
}

/// Fencing fixture factory.
#[async_trait]
pub trait FencingStoreFactory: Send + Sync {
    /// Concrete test fixture.
    type Store: FencingConformanceStore;

    /// Creates an isolated empty coordination store.
    async fn create_clean(&self) -> Result<Self::Store, AdapterError>;
}

/// Fencing observation surface used only by conformance.
#[async_trait]
pub trait FencingConformanceStore: Send + Sync {
    /// Executes one scenario and returns observable semantic evidence.
    async fn observe(&self, case: FencingConformanceCase) -> Result<ProbeResult, AdapterError>;
}

/// Runs local atomicity, reopen, cursor, migration, and durability scenarios.
///
/// # Errors
///
/// Returns the adapter error when fixture creation or a probe cannot execute. Semantic failures are
/// returned as failed observations rather than transport errors.
pub async fn run_local_store_conformance<F: LocalStoreFactory>(
    factory: &F,
) -> Result<Vec<TestObservation>, AdapterError> {
    let store = factory.create_clean().await?;
    let cases = [
        (50, LocalConformanceCase::AtomicDomainAndOutbox),
        (54, LocalConformanceCase::MigrationPreservation),
        (57, LocalConformanceCase::CriticalDurability),
        (50, LocalConformanceCase::CrashReopen),
        (50, LocalConformanceCase::CursorAtomicity),
    ];
    let mut results = Vec::with_capacity(cases.len());
    for (id, case) in cases {
        results.push((id, store.observe(case).await?));
    }
    Ok(finish_observations(results))
}

/// Runs authority atomicity, idempotency, digest, retry, CAS, and journal scenarios.
///
/// # Errors
///
/// Returns the adapter error when fixture creation or a probe cannot execute.
pub async fn run_authority_store_conformance<F: AuthorityStoreFactory>(
    factory: &F,
) -> Result<Vec<TestObservation>, AdapterError> {
    let store = factory.create_clean().await?;
    let cases = [
        (51, AuthorityConformanceCase::AtomicPublication),
        (53, AuthorityConformanceCase::PayloadMismatch),
        (51, AuthorityConformanceCase::DuplicateReplay),
        (51, AuthorityConformanceCase::SerializationBehavior),
        (51, AuthorityConformanceCase::VersionCompareAndSwap),
        (51, AuthorityConformanceCase::JournalRetention),
    ];
    let mut results = Vec::with_capacity(cases.len());
    for (id, case) in cases {
        results.push((id, store.observe(case).await?));
    }
    Ok(finish_observations(results))
}

/// Runs publication, partial-write, resume, corruption, and activation scenarios.
///
/// # Errors
///
/// Returns the adapter error when fixture creation or a probe cannot execute.
pub async fn run_snapshot_conformance<F: SnapshotStoreFactory>(
    factory: &F,
) -> Result<Vec<TestObservation>, AdapterError> {
    let store = factory.create_clean().await?;
    let cases = [
        (49, SnapshotConformanceCase::Publish),
        (49, SnapshotConformanceCase::PartialWriteIsolation),
        (49, SnapshotConformanceCase::Resume),
        (49, SnapshotConformanceCase::CorruptChunk),
        (49, SnapshotConformanceCase::AtomicActivation),
    ];
    let mut results = Vec::with_capacity(cases.len());
    for (id, case) in cases {
        results.push((id, store.observe(case).await?));
    }
    Ok(finish_observations(results))
}

/// Runs takeover, stale-writer, and token-monotonicity scenarios.
///
/// # Errors
///
/// Returns the adapter error when fixture creation or a probe cannot execute.
pub async fn run_fencing_conformance<F: FencingStoreFactory>(
    factory: &F,
) -> Result<Vec<TestObservation>, AdapterError> {
    let store = factory.create_clean().await?;
    let cases = [
        (49, FencingConformanceCase::LeaseTakeover),
        (49, FencingConformanceCase::StaleWriter),
        (49, FencingConformanceCase::TokenMonotonicity),
    ];
    let mut results = Vec::with_capacity(cases.len());
    for (id, case) in cases {
        results.push((id, store.observe(case).await?));
    }
    Ok(finish_observations(results))
}

fn finish_observations(results: Vec<(u32, ProbeResult)>) -> Vec<TestObservation> {
    let mut grouped = BTreeMap::<u32, (bool, Vec<String>)>::new();
    for (id, result) in results {
        let entry = grouped.entry(id).or_insert_with(|| (true, Vec::new()));
        entry.0 &= result.passed;
        entry.1.push(result.evidence);
    }
    grouped
        .into_iter()
        .map(|(id, (passed, evidence))| TestObservation {
            test_id: ConformanceTestId(id),
            status: if passed {
                TestStatus::Passed
            } else {
                TestStatus::Failed
            },
            seed: None,
            evidence,
            message: "adapter SDK conformance probe".to_owned(),
        })
        .collect()
}
