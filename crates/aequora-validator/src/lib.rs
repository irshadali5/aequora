//! Structural validation from untrusted wire DTOs into bounded request types.

use aequora_protocol::{BootstrapRequest, OperationEnvelope, SessionMetadata, SyncRequest};
use aequora_types::{OperationId, ProtocolVersion};
use std::collections::HashSet;
use thiserror::Error;

/// Resource and compatibility limits applied before business processing.
#[derive(Clone, Copy, Debug)]
pub struct ProtocolLimits {
    /// Oldest protocol version accepted during rolling upgrades.
    pub minimum_protocol: ProtocolVersion,
    /// Current protocol version accepted and emitted by this server.
    pub current_protocol: ProtocolVersion,
    /// Maximum operations in one request.
    pub max_operations: usize,
    /// Maximum serialized bytes in one operation payload.
    pub max_operation_bytes: usize,
    /// Maximum dependencies declared by one operation.
    pub max_dependencies: usize,
    /// Maximum diagnostic trace identifier bytes.
    pub max_trace_id_bytes: usize,
    /// Maximum partial-scope partitions.
    pub max_partitions: usize,
    /// Maximum opaque value bytes in one partition.
    pub max_partition_bytes: usize,
}

impl Default for ProtocolLimits {
    fn default() -> Self {
        Self {
            minimum_protocol: ProtocolVersion::V1,
            current_protocol: ProtocolVersion::V1,
            max_operations: 256,
            max_operation_bytes: 256 * 1_024,
            max_dependencies: 32,
            max_trace_id_bytes: 128,
            max_partitions: 32,
            max_partition_bytes: 1_024,
        }
    }
}

/// A request that passed protocol-level structural validation.
#[derive(Debug)]
pub struct ValidatedRequest(SyncRequest);

impl ValidatedRequest {
    /// Borrows the validated request.
    #[must_use]
    pub const fn as_request(&self) -> &SyncRequest {
        &self.0
    }

    /// Consumes the wrapper.
    #[must_use]
    pub fn into_inner(self) -> SyncRequest {
        self.0
    }
}

/// Structural request validation failure.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ValidationError {
    /// Request protocol is unsupported.
    #[error("unsupported protocol version {actual}; accepted range is {minimum} through {current}")]
    Protocol {
        actual: u16,
        minimum: u16,
        current: u16,
    },
    /// Cursor scope differs from the session scope.
    #[error("cursor scope does not match session scope")]
    CursorScope,
    /// Batch exceeds the operation count limit.
    #[error("operation count {actual} exceeds limit {maximum}")]
    BatchSize { actual: usize, maximum: usize },
    /// An operation ID occurs more than once in the batch.
    #[error("operation {0} appears more than once")]
    DuplicateOperation(OperationId),
    /// An operation envelope has a different protocol version.
    #[error("operation {operation} uses a different protocol version")]
    EnvelopeProtocol { operation: OperationId },
    /// Operation payload exceeds its size limit.
    #[error("operation {operation} payload exceeds {maximum} bytes")]
    PayloadSize {
        operation: OperationId,
        maximum: usize,
    },
    /// Operation dependency list exceeds its limit.
    #[error("operation {operation} has more than {maximum} dependencies")]
    DependencyCount {
        operation: OperationId,
        maximum: usize,
    },
    /// Operation depends on itself.
    #[error("operation {0} depends on itself")]
    SelfDependency(OperationId),
    /// An operation lists the same dependency more than once.
    #[error("operation {operation} repeats dependency {dependency}")]
    DuplicateDependency {
        operation: OperationId,
        dependency: OperationId,
    },
    /// Trace metadata exceeds its bounded length.
    #[error("operation {operation} trace identifier exceeds {maximum} bytes")]
    TraceId {
        operation: OperationId,
        maximum: usize,
    },
    /// A checked numeric type contained its reserved zero value after deserialization.
    #[error("operation {operation} contains an invalid zero {field}")]
    ZeroValue {
        operation: OperationId,
        field: &'static str,
    },
    /// Partial synchronization scope has too many partition selectors.
    #[error("partition count {actual} exceeds limit {maximum}")]
    PartitionCount { actual: usize, maximum: usize },
    /// A partition selector is invalid or excessive.
    #[error("partition {index} is invalid or exceeds {maximum} bytes")]
    Partition { index: usize, maximum: usize },
    /// A new bootstrap request must begin at offset zero.
    #[error("a new bootstrap snapshot must begin at offset zero")]
    BootstrapOffset,
}

/// Validates protocol compatibility, identity-independent structure, and all bounds.
///
/// # Errors
///
/// Returns [`ValidationError`] when the request is incompatible, incorrectly scoped,
/// duplicated, self-dependent, or exceeds any configured resource bound.
pub fn validate_request(
    request: SyncRequest,
    limits: ProtocolLimits,
) -> Result<ValidatedRequest, ValidationError> {
    if request.protocol < limits.minimum_protocol || request.protocol > limits.current_protocol {
        return Err(ValidationError::Protocol {
            actual: request.protocol.0,
            minimum: limits.minimum_protocol.0,
            current: limits.current_protocol.0,
        });
    }
    if request
        .cursor
        .is_some_and(|cursor| cursor.scope != request.session.scope_id)
    {
        return Err(ValidationError::CursorScope);
    }
    validate_session(&request.session, &limits)?;
    if request.operations.len() > limits.max_operations {
        return Err(ValidationError::BatchSize {
            actual: request.operations.len(),
            maximum: limits.max_operations,
        });
    }
    let mut operation_ids = HashSet::with_capacity(request.operations.len());
    for operation in &request.operations {
        validate_operation(operation, request.protocol, &limits)?;
        if !operation_ids.insert(operation.operation_id) {
            return Err(ValidationError::DuplicateOperation(operation.operation_id));
        }
    }
    Ok(ValidatedRequest(request))
}

/// Validates the structure and resource bounds of a bootstrap request.
///
/// # Errors
///
/// Returns [`ValidationError`] for incompatible protocols, invalid initial offsets, or
/// excessive/invalid partial-scope partition descriptors.
pub fn validate_bootstrap_request(
    request: &BootstrapRequest,
    limits: ProtocolLimits,
) -> Result<(), ValidationError> {
    if request.protocol < limits.minimum_protocol || request.protocol > limits.current_protocol {
        return Err(ValidationError::Protocol {
            actual: request.protocol.0,
            minimum: limits.minimum_protocol.0,
            current: limits.current_protocol.0,
        });
    }
    if request.snapshot_id.is_none() && request.offset != 0 {
        return Err(ValidationError::BootstrapOffset);
    }
    validate_session(&request.session, &limits)
}

fn validate_session(
    session: &SessionMetadata,
    limits: &ProtocolLimits,
) -> Result<(), ValidationError> {
    if session.partitions.len() > limits.max_partitions {
        return Err(ValidationError::PartitionCount {
            actual: session.partitions.len(),
            maximum: limits.max_partitions,
        });
    }
    for (index, partition) in session.partitions.iter().enumerate() {
        if partition.kind == 0
            || partition.value.is_empty()
            || partition.value.len() > limits.max_partition_bytes
        {
            return Err(ValidationError::Partition {
                index,
                maximum: limits.max_partition_bytes,
            });
        }
    }
    Ok(())
}

fn validate_operation(
    operation: &OperationEnvelope,
    request_protocol: ProtocolVersion,
    limits: &ProtocolLimits,
) -> Result<(), ValidationError> {
    if operation.entity.entity_type.get() == 0 {
        return Err(ValidationError::ZeroValue {
            operation: operation.operation_id,
            field: "entity type",
        });
    }
    if operation
        .base_version
        .is_some_and(|version| version.get() == 0)
    {
        return Err(ValidationError::ZeroValue {
            operation: operation.operation_id,
            field: "base version",
        });
    }
    if operation.protocol_version != request_protocol {
        return Err(ValidationError::EnvelopeProtocol {
            operation: operation.operation_id,
        });
    }
    if operation.payload.len() > limits.max_operation_bytes {
        return Err(ValidationError::PayloadSize {
            operation: operation.operation_id,
            maximum: limits.max_operation_bytes,
        });
    }
    if operation.metadata.dependencies.len() > limits.max_dependencies {
        return Err(ValidationError::DependencyCount {
            operation: operation.operation_id,
            maximum: limits.max_dependencies,
        });
    }
    if operation
        .metadata
        .dependencies
        .contains(&operation.operation_id)
    {
        return Err(ValidationError::SelfDependency(operation.operation_id));
    }
    let mut dependencies = HashSet::with_capacity(operation.metadata.dependencies.len());
    for dependency in &operation.metadata.dependencies {
        if !dependencies.insert(*dependency) {
            return Err(ValidationError::DuplicateDependency {
                operation: operation.operation_id,
                dependency: *dependency,
            });
        }
    }
    if operation
        .metadata
        .trace_id
        .as_ref()
        .is_some_and(|trace| trace.len() > limits.max_trace_id_bytes)
    {
        return Err(ValidationError::TraceId {
            operation: operation.operation_id,
            maximum: limits.max_trace_id_bytes,
        });
    }
    Ok(())
}
