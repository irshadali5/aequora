//! Explicit canonical-to-physical schema maps for generic record and migration workflows.
//!
//! This crate validates declarations only. Database adapters remain responsible for native query,
//! transaction, and conversion implementations. No mapping is inferred from similar names.

use aequora_schema::{
    CanonicalEntityId, CanonicalFieldId, CanonicalType, CanonicalValue, EntitySchema,
    SchemaRegistry,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt::Debug};
use thiserror::Error;

/// Explicit conversion between one canonical type and a physical database representation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TypeMap {
    /// Physical storage preserves the canonical value without information loss.
    NativeLossless,
    /// Exact decimal encoded as normalized base-ten text.
    DecimalText,
    /// UTC timestamp encoded as integer Unix milliseconds, losing sub-millisecond precision.
    UnixMilliseconds(LossyRule),
    /// UUID encoded using a canonical lowercase hyphenated string.
    UuidText,
    /// UTF-8 text represented as database bytes with strict decode failure.
    Utf8Bytes,
    /// Application-defined conversion with explicit source/target and loss policy.
    Custom(CustomTypeMap),
}

impl TypeMap {
    /// Validates the conversion declaration, including explicit approval of lossy behavior.
    ///
    /// # Errors
    ///
    /// Rejects an unapproved lossy conversion or an invalid custom conversion name.
    pub fn validate(&self) -> Result<(), MappingError> {
        match self {
            Self::UnixMilliseconds(rule) => rule.validate(),
            Self::Custom(mapping) => mapping.validate(),
            Self::NativeLossless | Self::DecimalText | Self::UuidText | Self::Utf8Bytes => Ok(()),
        }
    }

    /// Reports whether this declaration permits information loss.
    #[must_use]
    pub const fn is_lossy(&self) -> bool {
        match self {
            Self::UnixMilliseconds(_) => true,
            Self::Custom(mapping) => mapping.loss.is_some(),
            Self::NativeLossless | Self::DecimalText | Self::UuidText | Self::Utf8Bytes => false,
        }
    }
}

/// Adapter-owned conversion hook between one canonical value and one physical representation.
///
/// Implementations should be small and deterministic. SQL, collection, and transaction behavior
/// remains outside this hook so the same conversion can be certified independently.
pub trait CanonicalValueConverter: Send + Sync {
    /// Database-specific value used by the adapter boundary.
    type Physical: Clone + Debug + Eq;

    /// Converts one canonical value into its declared physical representation.
    ///
    /// # Errors
    ///
    /// Returns a redaction-safe error when the canonical type or value is unsupported.
    fn encode(&self, value: &CanonicalValue) -> Result<Self::Physical, MappingConversionError>;

    /// Converts one physical representation into a canonical value.
    ///
    /// # Errors
    ///
    /// Returns a redaction-safe error when the physical value is invalid or unsupported.
    fn decode(&self, value: &Self::Physical) -> Result<CanonicalValue, MappingConversionError>;
}

/// One explicit golden fixture for a concrete adapter conversion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversionFixture<P> {
    /// Canonical input passed to the encoder.
    pub canonical: CanonicalValue,
    /// Exact physical value expected from the adapter converter.
    pub physical: P,
    /// Canonical value expected after decoding the physical representation.
    ///
    /// This normally equals `canonical`; an approved lossy map must state the normalized value.
    pub round_trip: CanonicalValue,
}

/// Verifies a concrete conversion hook against explicit canonical/physical golden fixtures.
///
/// # Errors
///
/// Rejects an invalid mapping declaration, an empty fixture set, conversion failures, physical
/// encoding drift, or a canonical round-trip mismatch.
pub fn verify_conversion_fixtures<C>(
    declaration: &TypeMap,
    converter: &C,
    fixtures: &[ConversionFixture<C::Physical>],
) -> Result<(), ConversionContractError>
where
    C: CanonicalValueConverter,
{
    declaration.validate()?;
    if fixtures.is_empty() {
        return Err(ConversionContractError::EmptyFixtures);
    }
    for (index, fixture) in fixtures.iter().enumerate() {
        let encoded = converter
            .encode(&fixture.canonical)
            .map_err(|source| ConversionContractError::Conversion { index, source })?;
        if encoded != fixture.physical {
            return Err(ConversionContractError::PhysicalMismatch { index });
        }
        let decoded = converter
            .decode(&fixture.physical)
            .map_err(|source| ConversionContractError::Conversion { index, source })?;
        if decoded != fixture.round_trip {
            return Err(ConversionContractError::RoundTripMismatch { index });
        }
        if !declaration.is_lossy() && fixture.round_trip != fixture.canonical {
            return Err(ConversionContractError::UndeclaredLoss { index });
        }
    }
    Ok(())
}

/// Payload-free failure from a concrete adapter value conversion.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("adapter value conversion failed: {message}")]
pub struct MappingConversionError {
    /// Structural diagnostic that must not include row values or credentials.
    pub message: String,
}

impl MappingConversionError {
    /// Creates a redaction-safe conversion failure.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Failure from golden conversion-hook certification.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ConversionContractError {
    /// Mapping declaration itself is invalid.
    #[error(transparent)]
    Mapping(#[from] MappingError),
    /// Certification without fixtures proves nothing.
    #[error("conversion certification requires at least one golden fixture")]
    EmptyFixtures,
    /// Concrete converter returned a redaction-safe failure.
    #[error("conversion fixture {index} failed: {source}")]
    Conversion {
        /// Zero-based fixture index.
        index: usize,
        /// Adapter conversion failure.
        source: MappingConversionError,
    },
    /// Encoded physical value differs from the golden representation.
    #[error("conversion fixture {index} physical representation mismatch")]
    PhysicalMismatch {
        /// Zero-based fixture index.
        index: usize,
    },
    /// Decoded canonical value differs from the declared result.
    #[error("conversion fixture {index} canonical round-trip mismatch")]
    RoundTripMismatch {
        /// Zero-based fixture index.
        index: usize,
    },
    /// A supposedly lossless declaration changed the canonical value.
    #[error("conversion fixture {index} loses information without a lossy declaration")]
    UndeclaredLoss {
        /// Zero-based fixture index.
        index: usize,
    },
}

/// Required acknowledgement for a conversion that can lose information.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LossyRule {
    /// Human-readable reason the loss is safe for this application boundary.
    pub reason: String,
    /// Must be explicitly true; omission/default cannot silently approve loss.
    pub accepted: bool,
}

impl LossyRule {
    fn validate(&self) -> Result<(), MappingError> {
        if !self.accepted || self.reason.trim().is_empty() {
            Err(MappingError::UnapprovedLossyConversion)
        } else {
            Ok(())
        }
    }
}

/// Named application conversion whose semantics are implemented by an adapter extension.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CustomTypeMap {
    /// Stable conversion identifier registered by the host.
    pub converter: String,
    /// Physical type name used for diagnostics, never interpolated into SQL by core code.
    pub physical_type: String,
    /// Required when the conversion is not demonstrably lossless.
    pub loss: Option<LossyRule>,
}

impl CustomTypeMap {
    fn validate(&self) -> Result<(), MappingError> {
        validate_name(&self.converter)?;
        validate_name(&self.physical_type)?;
        if let Some(loss) = &self.loss {
            loss.validate()?;
        }
        Ok(())
    }
}

/// Mapping for one canonical field into an adapter-owned physical field/path.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FieldMap {
    /// Rename-stable canonical field identity.
    pub canonical: CanonicalFieldId,
    /// Adapter-interpreted physical column, document path, or KV component name.
    pub physical: String,
    /// Explicit type conversion policy.
    pub type_map: TypeMap,
}

/// Mapping for one canonical entity into an adapter-owned physical structure.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EntityMap {
    /// Rename-stable canonical entity identity.
    pub canonical: CanonicalEntityId,
    /// Adapter-interpreted table, collection, bucket, or document-kind name.
    pub physical: String,
    /// Explicit canonical field mappings.
    pub fields: Vec<FieldMap>,
}

/// Complete mapping manifest for one adapter role and physical schema version.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SchemaMap {
    /// Stable map name suitable for diagnostics and migration manifests.
    pub name: String,
    /// Application-managed physical schema revision.
    pub physical_schema_version: u32,
    /// Entity mappings; unmapped entities are not silently synchronized.
    pub entities: Vec<EntityMap>,
}

impl SchemaMap {
    /// Validates the complete map against the canonical registry.
    ///
    /// # Errors
    ///
    /// Rejects unknown/duplicate IDs, duplicate physical names, incomplete required fields,
    /// unapproved lossy conversions, and blank/version-zero manifests.
    pub fn validate(&self, registry: &SchemaRegistry) -> Result<(), MappingError> {
        validate_name(&self.name)?;
        if self.physical_schema_version == 0 {
            return Err(MappingError::ZeroPhysicalSchemaVersion);
        }
        let mut canonical_entities = BTreeSet::new();
        let mut physical_entities = BTreeSet::new();
        for entity_map in &self.entities {
            if !canonical_entities.insert(entity_map.canonical) {
                return Err(MappingError::DuplicateEntity(entity_map.canonical));
            }
            validate_name(&entity_map.physical)?;
            if !physical_entities.insert(entity_map.physical.as_str()) {
                return Err(MappingError::DuplicatePhysicalName(
                    entity_map.physical.clone(),
                ));
            }
            let entity = registry
                .entity(entity_map.canonical)
                .ok_or(MappingError::UnknownEntity(entity_map.canonical))?;
            validate_entity_map(entity_map, entity)?;
        }
        Ok(())
    }
}

fn validate_entity_map(mapping: &EntityMap, schema: &EntitySchema) -> Result<(), MappingError> {
    let mut canonical_fields = BTreeSet::new();
    let mut physical_fields = BTreeSet::new();
    for field_map in &mapping.fields {
        let field = schema
            .field(field_map.canonical)
            .ok_or(MappingError::UnknownField {
                entity: mapping.canonical,
                field: field_map.canonical,
            })?;
        if !canonical_fields.insert(field_map.canonical) {
            return Err(MappingError::DuplicateField(field_map.canonical));
        }
        validate_name(&field_map.physical)?;
        if !physical_fields.insert(field_map.physical.as_str()) {
            return Err(MappingError::DuplicatePhysicalName(
                field_map.physical.clone(),
            ));
        }
        validate_builtin_type_map(field.field_type, &field_map.type_map)?;
        field_map.type_map.validate()?;
    }
    for field in &schema.fields {
        if !field.nullable && !canonical_fields.contains(&field.id) {
            return Err(MappingError::MissingRequiredField(field.id));
        }
    }
    Ok(())
}

fn validate_builtin_type_map(
    canonical: CanonicalType,
    mapping: &TypeMap,
) -> Result<(), MappingError> {
    let compatible = match mapping {
        TypeMap::NativeLossless | TypeMap::Custom(_) => true,
        TypeMap::DecimalText => canonical == CanonicalType::Decimal,
        TypeMap::UnixMilliseconds(_) => canonical == CanonicalType::Timestamp,
        TypeMap::UuidText => canonical == CanonicalType::Uuid,
        TypeMap::Utf8Bytes => canonical == CanonicalType::Text,
    };
    if compatible {
        Ok(())
    } else {
        Err(MappingError::IncompatibleTypeMap { canonical })
    }
}

fn validate_name(name: &str) -> Result<(), MappingError> {
    if name.trim().is_empty() {
        Err(MappingError::BlankName)
    } else {
        Ok(())
    }
}

/// Fail-closed mapping-manifest validation failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum MappingError {
    /// Manifest, entity, field, converter, or physical type name is blank.
    #[error("mapping names must not be blank")]
    BlankName,
    /// Physical schema revision zero is unassigned.
    #[error("physical schema version must be non-zero")]
    ZeroPhysicalSchemaVersion,
    /// Canonical entity is absent from the registry.
    #[error("unknown canonical entity {0:?}")]
    UnknownEntity(CanonicalEntityId),
    /// Canonical field is absent from its entity.
    #[error("unknown canonical field {field:?} in entity {entity:?}")]
    UnknownField {
        /// Entity being mapped.
        entity: CanonicalEntityId,
        /// Unknown field.
        field: CanonicalFieldId,
    },
    /// Canonical entity appears twice in one physical map.
    #[error("duplicate canonical entity mapping {0:?}")]
    DuplicateEntity(CanonicalEntityId),
    /// Canonical field appears twice within one entity map.
    #[error("duplicate canonical field mapping {0:?}")]
    DuplicateField(CanonicalFieldId),
    /// Physical entity/field name is ambiguous within its scope.
    #[error("duplicate physical mapping name {0:?}")]
    DuplicatePhysicalName(String),
    /// Non-null canonical field has no explicit physical mapping.
    #[error("required canonical field {0:?} is not mapped")]
    MissingRequiredField(CanonicalFieldId),
    /// Built-in conversion does not apply to the canonical type.
    #[error("built-in conversion is incompatible with canonical type {canonical:?}")]
    IncompatibleTypeMap {
        /// Registered canonical type.
        canonical: CanonicalType,
    },
    /// Lossy behavior lacks explicit approval and rationale.
    #[error("lossy conversion requires explicit acceptance and a non-blank reason")]
    UnapprovedLossyConversion,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_schema::{FieldSchema, SchemaError};

    struct TextBytes;

    impl CanonicalValueConverter for TextBytes {
        type Physical = Vec<u8>;

        fn encode(&self, value: &CanonicalValue) -> Result<Self::Physical, MappingConversionError> {
            match value {
                CanonicalValue::Text(value) => Ok(value.as_bytes().to_vec()),
                _ => Err(MappingConversionError::new("expected canonical text")),
            }
        }

        fn decode(&self, value: &Self::Physical) -> Result<CanonicalValue, MappingConversionError> {
            String::from_utf8(value.clone())
                .map(CanonicalValue::Text)
                .map_err(|_| MappingConversionError::new("physical bytes are not UTF-8"))
        }
    }

    fn id<T>(value: Result<T, SchemaError>) -> T {
        value.unwrap_or_else(|error| panic!("fixture ID failed: {error}"))
    }

    fn registry() -> (SchemaRegistry, CanonicalEntityId, CanonicalFieldId) {
        let entity = id(CanonicalEntityId::new(1));
        let field = id(CanonicalFieldId::new(1));
        let mut registry = SchemaRegistry::new();
        registry
            .register(EntitySchema {
                id: entity,
                name: "invoice".to_owned(),
                fields: vec![FieldSchema {
                    id: field,
                    name: "total".to_owned(),
                    field_type: CanonicalType::Decimal,
                    nullable: false,
                }],
            })
            .unwrap_or_else(|error| panic!("fixture schema failed: {error}"));
        (registry, entity, field)
    }

    #[test]
    fn explicit_lossless_map_validates() {
        let (registry, entity, field) = registry();
        let mapping = SchemaMap {
            name: "postgres-invoice-v1".to_owned(),
            physical_schema_version: 1,
            entities: vec![EntityMap {
                canonical: entity,
                physical: "invoices".to_owned(),
                fields: vec![FieldMap {
                    canonical: field,
                    physical: "total_decimal".to_owned(),
                    type_map: TypeMap::DecimalText,
                }],
            }],
        };
        assert_eq!(mapping.validate(&registry), Ok(()));
    }

    #[test]
    fn missing_required_field_and_unapproved_loss_fail_closed() {
        let (registry, entity, field) = registry();
        let missing = SchemaMap {
            name: "incomplete".to_owned(),
            physical_schema_version: 1,
            entities: vec![EntityMap {
                canonical: entity,
                physical: "invoices".to_owned(),
                fields: vec![],
            }],
        };
        assert_eq!(
            missing.validate(&registry),
            Err(MappingError::MissingRequiredField(field))
        );

        let lossy = TypeMap::UnixMilliseconds(LossyRule {
            reason: String::new(),
            accepted: false,
        });
        assert_eq!(
            lossy.validate(),
            Err(MappingError::UnapprovedLossyConversion)
        );
    }

    #[test]
    fn concrete_conversion_hook_passes_exact_golden_roundtrip() {
        let canonical = CanonicalValue::Text("Aequora".to_owned());
        let fixtures = [ConversionFixture {
            canonical: canonical.clone(),
            physical: b"Aequora".to_vec(),
            round_trip: canonical,
        }];
        assert_eq!(
            verify_conversion_fixtures(&TypeMap::Utf8Bytes, &TextBytes, &fixtures),
            Ok(())
        );
    }

    #[test]
    fn conversion_certification_rejects_drift_and_empty_evidence() {
        assert_eq!(
            verify_conversion_fixtures(
                &TypeMap::Utf8Bytes,
                &TextBytes,
                &[] as &[ConversionFixture<Vec<u8>>],
            ),
            Err(ConversionContractError::EmptyFixtures)
        );
        let fixtures = [ConversionFixture {
            canonical: CanonicalValue::Text("expected".to_owned()),
            physical: b"different".to_vec(),
            round_trip: CanonicalValue::Text("different".to_owned()),
        }];
        assert_eq!(
            verify_conversion_fixtures(&TypeMap::Utf8Bytes, &TextBytes, &fixtures),
            Err(ConversionContractError::PhysicalMismatch { index: 0 })
        );
    }
}
