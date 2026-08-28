//! Bounded sanitized metadata diagnostics and semantic cross-adapter export.

use crate::{Digest, LogicalRecord, MetadataSchemaVersion, PersistenceError, StoreId, Timestamp};
use serde::{Deserialize, Serialize};

pub const MAX_EXPORT_RECORDS: usize = 1_000_000;
pub const MAX_EXPORT_KEY_BYTES: usize = 1_024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MetadataExportRecord {
    pub kind: LogicalRecord,
    pub canonical_key: Vec<u8>,
    pub canonical_digest: Digest,
    pub queryable_summary: Vec<(String, String)>,
    pub opaque_payload: Option<Vec<u8>>,
}

impl MetadataExportRecord {
    pub fn validate(&self) -> Result<(), PersistenceError> {
        if self.canonical_key.is_empty() || self.canonical_key.len() > MAX_EXPORT_KEY_BYTES {
            return Err(PersistenceError::LimitExceeded);
        }
        if self.canonical_digest == [0; 32] {
            return Err(PersistenceError::InvalidRecord(
                "metadata export digest is zero".into(),
            ));
        }
        if self.queryable_summary.iter().any(|(key, _)| {
            let lowered = key.to_ascii_lowercase();
            lowered.contains("private_key") || lowered.contains("secret")
        }) {
            return Err(PersistenceError::ConstraintViolation(
                "metadata export contains a forbidden secret field".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MetadataExport {
    pub schema_version: MetadataSchemaVersion,
    pub store_id: StoreId,
    pub exported_at: Timestamp,
    pub adapter_name: String,
    pub records: Vec<MetadataExportRecord>,
    pub root_digest: Digest,
}

impl MetadataExport {
    pub fn build(
        schema_version: MetadataSchemaVersion,
        store_id: StoreId,
        exported_at: Timestamp,
        adapter_name: impl Into<String>,
        mut records: Vec<MetadataExportRecord>,
    ) -> Result<Self, PersistenceError> {
        if records.len() > MAX_EXPORT_RECORDS {
            return Err(PersistenceError::LimitExceeded);
        }
        for record in &records {
            record.validate()?;
        }
        records.sort_unstable_by(|left, right| {
            left.kind
                .cmp(&right.kind)
                .then_with(|| left.canonical_key.cmp(&right.canonical_key))
        });
        let adapter_name = adapter_name.into();
        if adapter_name.trim().is_empty() {
            return Err(PersistenceError::InvalidRecord(
                "adapter name is blank".into(),
            ));
        }
        let encoded = postcard::to_stdvec(&(schema_version, store_id, &records))
            .map_err(|error| PersistenceError::InvalidRecord(error.to_string()))?;
        let root_digest = *blake3::hash(&encoded).as_bytes();
        Ok(Self {
            schema_version,
            store_id,
            exported_at,
            adapter_name,
            records,
            root_digest,
        })
    }

    pub fn verify(&self) -> Result<(), PersistenceError> {
        if self.records.len() > MAX_EXPORT_RECORDS {
            return Err(PersistenceError::LimitExceeded);
        }
        for record in &self.records {
            record.validate()?;
        }
        if self.records.windows(2).any(|pair| {
            (pair[0].kind, pair[0].canonical_key.as_slice())
                >= (pair[1].kind, pair[1].canonical_key.as_slice())
        }) {
            return Err(PersistenceError::CorruptionDetected(
                "metadata export is unsorted or contains duplicate logical keys".into(),
            ));
        }
        let encoded = postcard::to_stdvec(&(self.schema_version, self.store_id, &self.records))
            .map_err(|error| PersistenceError::InvalidRecord(error.to_string()))?;
        if self.root_digest != *blake3::hash(&encoded).as_bytes() {
            return Err(PersistenceError::CorruptionDetected(
                "metadata export root mismatch".into(),
            ));
        }
        Ok(())
    }

    /// Compares logical state independent of an adapter's physical representation and name.
    #[must_use]
    pub fn semantically_equivalent(&self, other: &Self) -> bool {
        self.schema_version == other.schema_version && self.records == other.records
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplainRecord {
    pub logical_record: LogicalRecord,
    pub canonical_key: Vec<u8>,
    pub links: Vec<ExplainLink>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExplainLink {
    pub relationship: &'static str,
    pub logical_record: LogicalRecord,
    pub sanitized_key: String,
}
