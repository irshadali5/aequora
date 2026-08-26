//! Canonical, database-neutral record values and schema declarations.
//!
//! Domain-operation synchronization does not need this crate. It exists for explicit generic
//! record, import/export, and database-migration workflows and deliberately excludes SQL types,
//! table names, floating-point ambiguity, and automatic semantic guessing.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

macro_rules! nonzero_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        pub struct $name(u32);

        impl $name {
            /// Creates a stable identifier, rejecting zero as unassigned.
            ///
            /// # Errors
            ///
            /// Returns [`SchemaError::ZeroIdentifier`] when `value` is zero.
            pub const fn new(value: u32) -> Result<Self, SchemaError> {
                if value == 0 {
                    Err(SchemaError::ZeroIdentifier)
                } else {
                    Ok(Self(value))
                }
            }

            /// Returns the stable numeric value used in manifests and canonical records.
            #[must_use]
            pub const fn get(self) -> u32 {
                self.0
            }
        }
    };
}

nonzero_id!(
    /// Stable canonical entity schema identifier.
    CanonicalEntityId
);
nonzero_id!(
    /// Stable canonical field identifier that survives physical column renames.
    CanonicalFieldId
);

/// Decimal represented without database-specific precision or binary floating-point behavior.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct CanonicalDecimal {
    coefficient: i128,
    scale: u32,
}

impl CanonicalDecimal {
    /// Creates a normalized base-ten decimal by removing insignificant trailing zeros.
    #[must_use]
    pub const fn new(mut coefficient: i128, mut scale: u32) -> Self {
        if coefficient == 0 {
            return Self {
                coefficient: 0,
                scale: 0,
            };
        }
        while scale > 0 && coefficient % 10 == 0 {
            coefficient /= 10;
            scale -= 1;
        }
        Self { coefficient, scale }
    }

    /// Signed base-ten coefficient.
    #[must_use]
    pub const fn coefficient(self) -> i128 {
        self.coefficient
    }

    /// Number of decimal digits to the right of the decimal point.
    #[must_use]
    pub const fn scale(self) -> u32 {
        self.scale
    }
}

/// UTC instant with explicit source precision and no implicit local timezone.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct CanonicalTimestamp {
    /// Whole Unix seconds.
    pub unix_seconds: i64,
    /// Nanoseconds within the second.
    pub nanoseconds: u32,
}

impl CanonicalTimestamp {
    /// Creates a timestamp after validating the subsecond range.
    ///
    /// # Errors
    ///
    /// Returns [`SchemaError::InvalidNanoseconds`] for values of one billion or greater.
    pub const fn new(unix_seconds: i64, nanoseconds: u32) -> Result<Self, SchemaError> {
        if nanoseconds >= 1_000_000_000 {
            Err(SchemaError::InvalidNanoseconds(nanoseconds))
        } else {
            Ok(Self {
                unix_seconds,
                nanoseconds,
            })
        }
    }
}

/// Lossless canonical value model used only by optional record-oriented workflows.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CanonicalValue {
    /// Explicit database null, distinct from an absent field.
    Null,
    /// Boolean value.
    Boolean(bool),
    /// Signed integer without width-dependent coercion.
    Signed(i64),
    /// Unsigned integer without silently narrowing into a signed database column.
    Unsigned(u64),
    /// Exact normalized decimal.
    Decimal(CanonicalDecimal),
    /// UTF-8 text.
    Text(String),
    /// Opaque bytes.
    Bytes(Vec<u8>),
    /// Stable UUID.
    Uuid(uuid::Uuid),
    /// UTC timestamp with nanosecond precision.
    Timestamp(CanonicalTimestamp),
    /// Ordered list of canonical values.
    List(Vec<Self>),
    /// Field-ID keyed nested record.
    Object(BTreeMap<CanonicalFieldId, Self>),
}

/// Logical type expected for a canonical field.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum CanonicalType {
    /// Boolean.
    Boolean,
    /// Signed 64-bit integer.
    Signed,
    /// Unsigned 64-bit integer.
    Unsigned,
    /// Exact base-ten decimal.
    Decimal,
    /// UTF-8 text.
    Text,
    /// Opaque bytes.
    Bytes,
    /// UUID.
    Uuid,
    /// UTC timestamp.
    Timestamp,
    /// Homogeneous or application-validated list.
    List,
    /// Nested field-ID keyed object.
    Object,
}

impl CanonicalValue {
    /// Returns the logical type, or `None` for null which is validated through field nullability.
    #[must_use]
    pub const fn logical_type(&self) -> Option<CanonicalType> {
        match self {
            Self::Null => None,
            Self::Boolean(_) => Some(CanonicalType::Boolean),
            Self::Signed(_) => Some(CanonicalType::Signed),
            Self::Unsigned(_) => Some(CanonicalType::Unsigned),
            Self::Decimal(_) => Some(CanonicalType::Decimal),
            Self::Text(_) => Some(CanonicalType::Text),
            Self::Bytes(_) => Some(CanonicalType::Bytes),
            Self::Uuid(_) => Some(CanonicalType::Uuid),
            Self::Timestamp(_) => Some(CanonicalType::Timestamp),
            Self::List(_) => Some(CanonicalType::List),
            Self::Object(_) => Some(CanonicalType::Object),
        }
    }
}

/// One stable field in a canonical entity schema.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FieldSchema {
    /// Rename-stable identifier.
    pub id: CanonicalFieldId,
    /// Human-readable canonical name.
    pub name: String,
    /// Logical value type.
    pub field_type: CanonicalType,
    /// Whether explicit `Null` is permitted.
    pub nullable: bool,
}

/// Canonical schema for one record-oriented entity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EntitySchema {
    /// Rename-stable entity identifier.
    pub id: CanonicalEntityId,
    /// Human-readable canonical name.
    pub name: String,
    /// Stable fields; physical databases may use different names and layouts.
    pub fields: Vec<FieldSchema>,
}

impl EntitySchema {
    /// Validates stable IDs/names and rejects ambiguous schemas.
    ///
    /// # Errors
    ///
    /// Returns a typed error for blank or duplicate names/IDs.
    pub fn validate(&self) -> Result<(), SchemaError> {
        validate_name(&self.name)?;
        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for field in &self.fields {
            validate_name(&field.name)?;
            if !ids.insert(field.id) {
                return Err(SchemaError::DuplicateFieldId(field.id));
            }
            if !names.insert(field.name.as_str()) {
                return Err(SchemaError::DuplicateFieldName(field.name.clone()));
            }
        }
        Ok(())
    }

    /// Looks up one stable field identifier.
    #[must_use]
    pub fn field(&self, id: CanonicalFieldId) -> Option<&FieldSchema> {
        self.fields.iter().find(|field| field.id == id)
    }
}

/// Validated canonical schema registry for record-mode sync and migration tooling.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SchemaRegistry {
    entities: BTreeMap<CanonicalEntityId, EntitySchema>,
}

impl SchemaRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entities: BTreeMap::new(),
        }
    }

    /// Registers one validated entity and rejects ID or canonical-name reuse.
    ///
    /// # Errors
    ///
    /// Returns a schema validation error or duplicate entity error.
    pub fn register(&mut self, mut entity: EntitySchema) -> Result<(), SchemaError> {
        entity.validate()?;
        entity.fields.sort_unstable_by_key(|field| field.id);
        if self.entities.contains_key(&entity.id) {
            return Err(SchemaError::DuplicateEntityId(entity.id));
        }
        if self
            .entities
            .values()
            .any(|existing| existing.name == entity.name)
        {
            return Err(SchemaError::DuplicateEntityName(entity.name));
        }
        self.entities.insert(entity.id, entity);
        Ok(())
    }

    /// Revalidates a deserialized registry before it is used for mapping or migration.
    ///
    /// # Errors
    ///
    /// Rejects invalid entity schemas, map-key/identity mismatch, or duplicate canonical names.
    pub fn validate(&self) -> Result<(), SchemaError> {
        let mut names = BTreeSet::new();
        for (&id, entity) in &self.entities {
            entity.validate()?;
            if id != entity.id {
                return Err(SchemaError::EntityKeyMismatch {
                    key: id,
                    entity: entity.id,
                });
            }
            if !names.insert(entity.name.as_str()) {
                return Err(SchemaError::DuplicateEntityName(entity.name.clone()));
            }
        }
        Ok(())
    }

    /// Looks up an entity by its stable identifier.
    #[must_use]
    pub fn entity(&self, id: CanonicalEntityId) -> Option<&EntitySchema> {
        self.entities.get(&id)
    }

    /// Registered schemas in stable identifier order.
    #[must_use]
    pub fn entities(&self) -> impl ExactSizeIterator<Item = &EntitySchema> {
        self.entities.values()
    }
}

/// One canonical record independent of database row/document layout.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CanonicalRecord {
    /// Canonical entity schema.
    pub entity: CanonicalEntityId,
    /// Stable application-defined key encoded without database-specific column structure.
    pub key: CanonicalValue,
    /// Present canonical fields; absence is distinct from explicit null.
    pub fields: BTreeMap<CanonicalFieldId, CanonicalValue>,
}

impl CanonicalRecord {
    /// Validates field existence, nullability, and logical types against a registry.
    ///
    /// # Errors
    ///
    /// Returns a typed schema error without coercing values.
    pub fn validate(&self, registry: &SchemaRegistry) -> Result<(), SchemaError> {
        let entity = registry
            .entity(self.entity)
            .ok_or(SchemaError::UnknownEntity(self.entity))?;
        for (&field_id, value) in &self.fields {
            let field = entity
                .field(field_id)
                .ok_or(SchemaError::UnknownField(field_id))?;
            if matches!(value, CanonicalValue::Null) {
                if !field.nullable {
                    return Err(SchemaError::NullNotAllowed(field_id));
                }
            } else if value.logical_type() != Some(field.field_type) {
                return Err(SchemaError::TypeMismatch {
                    field: field_id,
                    expected: field.field_type,
                    actual: value.logical_type(),
                });
            }
        }
        Ok(())
    }
}

fn validate_name(name: &str) -> Result<(), SchemaError> {
    if name.trim().is_empty() {
        Err(SchemaError::BlankName)
    } else {
        Ok(())
    }
}

/// Fail-closed canonical schema or record validation error.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum SchemaError {
    /// Zero is reserved for unassigned IDs.
    #[error("canonical identifiers must be non-zero")]
    ZeroIdentifier,
    /// Timestamp subsecond component is outside its canonical range.
    #[error("timestamp nanoseconds {0} must be less than one billion")]
    InvalidNanoseconds(u32),
    /// A canonical or physical name is blank.
    #[error("schema names must not be blank")]
    BlankName,
    /// Entity ID was reused.
    #[error("duplicate canonical entity ID {0:?}")]
    DuplicateEntityId(CanonicalEntityId),
    /// Entity canonical name was reused.
    #[error("duplicate canonical entity name {0:?}")]
    DuplicateEntityName(String),
    /// Serialized registry key differs from the entity's declared stable ID.
    #[error("registry key {key:?} does not match entity ID {entity:?}")]
    EntityKeyMismatch {
        /// Map key.
        key: CanonicalEntityId,
        /// Entity's own ID.
        entity: CanonicalEntityId,
    },
    /// Field ID was reused within an entity.
    #[error("duplicate canonical field ID {0:?}")]
    DuplicateFieldId(CanonicalFieldId),
    /// Field canonical name was reused within an entity.
    #[error("duplicate canonical field name {0:?}")]
    DuplicateFieldName(String),
    /// Record refers to an unregistered entity.
    #[error("unknown canonical entity {0:?}")]
    UnknownEntity(CanonicalEntityId),
    /// Record or map refers to an unknown field.
    #[error("unknown canonical field {0:?}")]
    UnknownField(CanonicalFieldId),
    /// Explicit null was supplied to a non-null field.
    #[error("null is not allowed for canonical field {0:?}")]
    NullNotAllowed(CanonicalFieldId),
    /// Value type did not match the registered field type.
    #[error("canonical field {field:?} expects {expected:?}, got {actual:?}")]
    TypeMismatch {
        /// Field being validated.
        field: CanonicalFieldId,
        /// Registered type.
        expected: CanonicalType,
        /// Actual non-null type, or `None` only for malformed validation paths.
        actual: Option<CanonicalType>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id<T>(value: Result<T, SchemaError>) -> T {
        value.unwrap_or_else(|error| panic!("fixture ID failed: {error}"))
    }

    #[test]
    fn decimals_have_one_canonical_representation() {
        assert_eq!(
            CanonicalDecimal::new(12_300, 3),
            CanonicalDecimal::new(123, 1)
        );
        assert_eq!(CanonicalDecimal::new(0, 99).scale(), 0);
    }

    #[test]
    fn registry_and_record_validation_fail_closed() {
        let entity_id = id(CanonicalEntityId::new(1));
        let field_id = id(CanonicalFieldId::new(1));
        let mut registry = SchemaRegistry::new();
        assert_eq!(
            registry.register(EntitySchema {
                id: entity_id,
                name: "student".to_owned(),
                fields: vec![FieldSchema {
                    id: field_id,
                    name: "phone".to_owned(),
                    field_type: CanonicalType::Text,
                    nullable: false,
                }],
            }),
            Ok(())
        );
        let valid = CanonicalRecord {
            entity: entity_id,
            key: CanonicalValue::Uuid(uuid::Uuid::nil()),
            fields: BTreeMap::from([(field_id, CanonicalValue::Text("123".to_owned()))]),
        };
        assert_eq!(valid.validate(&registry), Ok(()));
        let invalid = CanonicalRecord {
            fields: BTreeMap::from([(field_id, CanonicalValue::Signed(123))]),
            ..valid
        };
        assert_eq!(
            invalid.validate(&registry),
            Err(SchemaError::TypeMismatch {
                field: field_id,
                expected: CanonicalType::Text,
                actual: Some(CanonicalType::Signed),
            })
        );
    }
}
