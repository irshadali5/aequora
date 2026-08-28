//! Metadata store identity, UTC timestamps, and the internal schema registry.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, str::FromStr};
use thiserror::Error;
use uuid::Uuid;

macro_rules! uuid_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self { Self(Uuid::now_v7()) }
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self { Self(value) }
            #[must_use]
            pub const fn as_uuid(self) -> Uuid { self.0 }
        }

        impl Default for $name { fn default() -> Self { Self::new() } }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(f) }
        }
        impl FromStr for $name {
            type Err = uuid::Error;
            fn from_str(value: &str) -> Result<Self, Self::Err> { Uuid::parse_str(value).map(Self) }
        }
    };
}

uuid_id!(/// Stable identity of one local or authority metadata store.
    StoreId);
uuid_id!(/// Stable identity of one local coordinator process instance.
    ProcessInstanceId);

/// Internal Aequora persistence schema version, independent of domain and wire schemas.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MetadataSchemaVersion(u32);

impl Default for MetadataSchemaVersion {
    fn default() -> Self {
        Self::new_const(1)
    }
}

impl MetadataSchemaVersion {
    /// Creates a non-zero schema version.
    pub const fn new(value: u32) -> Result<Self, VersionError> {
        if value == 0 {
            Err(VersionError::Zero)
        } else {
            Ok(Self(value))
        }
    }

    /// Const constructor for checked repository constants.
    #[must_use]
    pub const fn new_const(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Advances the schema version without wrapping.
    pub const fn checked_next(self) -> Result<Self, VersionError> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(VersionError::Exhausted),
        }
    }
}

/// Monotonic local store replacement generation.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct LocalStoreGeneration(u64);

impl LocalStoreGeneration {
    pub const INITIAL: Self = Self(1);
    pub const fn new(value: u64) -> Result<Self, VersionError> {
        if value == 0 {
            Err(VersionError::Zero)
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
    pub const fn checked_next(self) -> Result<Self, VersionError> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(VersionError::Exhausted),
        }
    }
}

/// Monotonic operation ordering within one local store.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct LocalOperationSeq(u64);

impl LocalOperationSeq {
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
    pub const fn checked_next(self) -> Result<Self, VersionError> {
        match self.0.checked_add(1) {
            Some(value) => Ok(Self(value)),
            None => Err(VersionError::Exhausted),
        }
    }
}

macro_rules! monotonic_u64 {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(u64);
        impl $name {
            pub const INITIAL: Self = Self(1);
            pub const fn new(value: u64) -> Result<Self, VersionError> {
                if value == 0 { Err(VersionError::Zero) } else { Ok(Self(value)) }
            }
            #[must_use]
            pub const fn get(self) -> u64 { self.0 }
            pub const fn checked_next(self) -> Result<Self, VersionError> {
                match self.0.checked_add(1) {
                    Some(value) => Ok(Self(value)),
                    None => Err(VersionError::Exhausted),
                }
            }
        }
    };
}

monotonic_u64!(/// Version of server-owned rules for one synchronization scope.
    ScopeVersion);
monotonic_u64!(/// Incompatible generation of one synchronization scope.
    ScopeGeneration);

/// Version of the projection schema represented by scope and cursor metadata.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct ProjectionVersion(pub u32);

/// Monotonic internal migration identifier.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct MetadataMigrationId(u32);

impl MetadataMigrationId {
    pub const fn new(value: u32) -> Result<Self, VersionError> {
        if value == 0 {
            Err(VersionError::Zero)
        } else {
            Ok(Self(value))
        }
    }
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Canonical UTC timestamp for metadata. It never carries a local timezone.
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
pub struct Timestamp {
    pub unix_seconds: i64,
    pub nanoseconds: u32,
}

impl Timestamp {
    pub const fn new(unix_seconds: i64, nanoseconds: u32) -> Result<Self, VersionError> {
        if nanoseconds >= 1_000_000_000 {
            Err(VersionError::InvalidNanoseconds)
        } else {
            Ok(Self {
                unix_seconds,
                nanoseconds,
            })
        }
    }
}

/// The one root record every metadata store must contain.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MetadataRoot {
    pub schema_version: MetadataSchemaVersion,
    pub store_id: StoreId,
    pub created_at: Timestamp,
    pub last_migrated_at: Timestamp,
}

/// Stable logical description of one persistent field.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MetadataFieldSpec {
    pub record: String,
    pub field: String,
    pub semantic_type: MetadataValueType,
    pub introduced: MetadataSchemaVersion,
    pub nullable: bool,
    pub default_value: Option<Vec<u8>>,
}

/// Database-neutral types permitted in logical metadata.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MetadataValueType {
    U64,
    I64,
    Bool,
    Bytes,
    String,
    Uuid,
    Timestamp,
    Enum,
    Opaque,
}

/// Versioned registry used by migrations, generated docs, and adapter certification.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct MetadataSchemaRegistry {
    pub current: MetadataSchemaVersion,
    pub fields: BTreeMap<String, MetadataFieldSpec>,
}

impl MetadataSchemaRegistry {
    #[must_use]
    pub fn new(current: MetadataSchemaVersion) -> Self {
        Self {
            current,
            fields: BTreeMap::new(),
        }
    }

    /// Registers one field and rejects a conflicting semantic definition.
    pub fn register(&mut self, spec: MetadataFieldSpec) -> Result<(), VersionError> {
        if spec.record.trim().is_empty() || spec.field.trim().is_empty() {
            return Err(VersionError::BlankField);
        }
        if spec.introduced > self.current {
            return Err(VersionError::FutureField);
        }
        let key = format!("{}.{}", spec.record, spec.field);
        if self.fields.insert(key, spec).is_some() {
            return Err(VersionError::DuplicateField);
        }
        Ok(())
    }

    /// Revalidates a decoded registry before it is trusted.
    pub fn validate(&self) -> Result<(), VersionError> {
        if self.current.get() == 0 {
            return Err(VersionError::Zero);
        }
        for (key, field) in &self.fields {
            if key != &format!("{}.{}", field.record, field.field) {
                return Err(VersionError::FieldKeyMismatch);
            }
            if field.introduced > self.current {
                return Err(VersionError::FutureField);
            }
        }
        Ok(())
    }
}

/// Version and schema failures that are safe to surface before normal operation starts.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum VersionError {
    #[error("metadata version or identity must be non-zero")]
    Zero,
    #[error("metadata version exhausted")]
    Exhausted,
    #[error("timestamp nanoseconds are outside the UTC second")]
    InvalidNanoseconds,
    #[error("metadata field name is blank")]
    BlankField,
    #[error("metadata field was introduced after the registry version")]
    FutureField,
    #[error("metadata field is duplicated")]
    DuplicateField,
    #[error("metadata field registry key does not match its identity")]
    FieldKeyMismatch,
}
