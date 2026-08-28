//! Executable Part 22 invariant identifiers and reusable validation helpers.

use crate::{
    MetadataRoot, MetadataSchemaVersion, OperationLedgerRecord, PersistenceError,
    SnapshotChunkRecord, SnapshotRecord, validate_snapshot_publication,
};
use aequora_types::OperationId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum MetadataInvariantId {
    StoreSchemaDeclaration,
    LedgerOperationIdentity,
    ClientCursorAtomicity,
    AuthoritativeMetadataAtomicity,
    SnapshotPublicationSafety,
    AdapterSemanticEquivalence,
    StaleFenceRejection,
    MigrationIntentPreservation,
    SecretKeyExclusion,
}

impl MetadataInvariantId {
    pub const ALL: [Self; 9] = [
        Self::StoreSchemaDeclaration,
        Self::LedgerOperationIdentity,
        Self::ClientCursorAtomicity,
        Self::AuthoritativeMetadataAtomicity,
        Self::SnapshotPublicationSafety,
        Self::AdapterSemanticEquivalence,
        Self::StaleFenceRejection,
        Self::MigrationIntentPreservation,
        Self::SecretKeyExclusion,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StoreSchemaDeclaration => "AEQ-INV-META001",
            Self::LedgerOperationIdentity => "AEQ-INV-META002",
            Self::ClientCursorAtomicity => "AEQ-INV-META003",
            Self::AuthoritativeMetadataAtomicity => "AEQ-INV-META004",
            Self::SnapshotPublicationSafety => "AEQ-INV-META005",
            Self::AdapterSemanticEquivalence => "AEQ-INV-META006",
            Self::StaleFenceRejection => "AEQ-INV-META007",
            Self::MigrationIntentPreservation => "AEQ-INV-META008",
            Self::SecretKeyExclusion => "AEQ-INV-META009",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MetadataInvariantEntry {
    pub id: MetadataInvariantId,
    pub statement: &'static str,
    pub conformance_test: &'static str,
}

pub const METADATA_INVARIANTS: [MetadataInvariantEntry; 9] = [
    MetadataInvariantEntry {
        id: MetadataInvariantId::StoreSchemaDeclaration,
        statement: "every durable store declares one supported metadata schema version before operation",
        conformance_test: "metadata_root_schema_gate",
    },
    MetadataInvariantEntry {
        id: MetadataInvariantId::LedgerOperationIdentity,
        statement: "operation identity is unique and semantic payload mismatch is rejected",
        conformance_test: "ledger_duplicate_payload_mismatch",
    },
    MetadataInvariantEntry {
        id: MetadataInvariantId::ClientCursorAtomicity,
        statement: "authoritative apply and cursor advancement share one local transaction",
        conformance_test: "cursor_crash_matrix",
    },
    MetadataInvariantEntry {
        id: MetadataInvariantId::AuthoritativeMetadataAtomicity,
        statement: "authoritative business and required metadata effects share one transaction",
        conformance_test: "authoritative_crash_matrix",
    },
    MetadataInvariantEntry {
        id: MetadataInvariantId::SnapshotPublicationSafety,
        statement: "published snapshots contain durable verified chunks from one boundary",
        conformance_test: "snapshot_publish_atomicity",
    },
    MetadataInvariantEntry {
        id: MetadataInvariantId::AdapterSemanticEquivalence,
        statement: "physical mappings retain logical uniqueness, order, transaction, and retention semantics",
        conformance_test: "cross_adapter_metadata_equivalence",
    },
    MetadataInvariantEntry {
        id: MetadataInvariantId::StaleFenceRejection,
        statement: "stale fencing tokens cannot update coordinator, job, or authority state",
        conformance_test: "metadata_fencing_race",
    },
    MetadataInvariantEntry {
        id: MetadataInvariantId::MigrationIntentPreservation,
        statement: "migrations preserve pending intent or commit no partial migration",
        conformance_test: "metadata_migration_intent",
    },
    MetadataInvariantEntry {
        id: MetadataInvariantId::SecretKeyExclusion,
        statement: "ordinary metadata records never contain private key material",
        conformance_test: "metadata_secret_scan",
    },
];

pub fn validate_root(
    root: MetadataRoot,
    supported: MetadataSchemaVersion,
) -> Result<(), PersistenceError> {
    if root.schema_version.get() == 0 {
        Err(PersistenceError::CorruptionDetected(
            "zero metadata schema version".into(),
        ))
    } else if root.schema_version > supported {
        Err(PersistenceError::SchemaTooNew)
    } else {
        Ok(())
    }
}

/// Returns the retained result for an exact retry or rejects semantic identity drift.
pub fn classify_ledger_insert(
    operation_id: OperationId,
    semantic_payload_digest: [u8; 32],
    retained: Option<&OperationLedgerRecord>,
) -> Result<Option<&OperationLedgerRecord>, PersistenceError> {
    let Some(record) = retained else {
        return Ok(None);
    };
    if record.operation_id != operation_id {
        return Err(PersistenceError::CorruptionDetected(
            "ledger lookup returned another operation".into(),
        ));
    }
    if record.semantic_payload_digest != semantic_payload_digest {
        return Err(PersistenceError::PayloadDigestMismatch);
    }
    Ok(Some(record))
}

pub fn validate_fencing_token(current: u64, presented: u64) -> Result<(), PersistenceError> {
    if presented < current {
        Err(PersistenceError::VersionConflict)
    } else if presented == 0 {
        Err(PersistenceError::InvalidRecord(
            "fencing token is zero".into(),
        ))
    } else {
        Ok(())
    }
}

pub fn validate_published_snapshot(
    snapshot: &SnapshotRecord,
    chunks: &[SnapshotChunkRecord],
) -> Result<(), PersistenceError> {
    validate_snapshot_publication(snapshot, chunks)
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MetadataVerificationReport {
    pub checked_records: u64,
    pub findings: Vec<MetadataFinding>,
}

impl MetadataVerificationReport {
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MetadataFinding {
    pub invariant: MetadataInvariantId,
    pub record_kind: String,
    pub sanitized_record_key: String,
    pub detail: String,
}

/// Fault points every authoritative adapter must exercise for all-or-none commit behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthoritativeCrashPoint {
    AfterBusinessBeforeJournal,
    AfterJournalBeforeLedger,
    AfterLedgerBeforeCommit,
    AfterCommitBeforeResponse,
}

/// Fault points every local adapter must exercise for intent and cursor safety.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalCrashPoint {
    AfterBusinessBeforeOutbox,
    AfterEventApplyBeforeCursor,
    AfterCursorBeforeCommit,
}
