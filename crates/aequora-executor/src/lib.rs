//! Application-owned authorization and domain execution boundary.

use aequora_protocol::{ChangeKind, OperationEnvelope, RejectionCode, SessionMetadata};
use aequora_types::{ActorId, DeviceId, SchemaVersion, TenantId};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    sync::Arc,
};
use thiserror::Error;

/// Server-derived authenticated identity. Client claims must match it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthContext {
    /// Authenticated actor.
    pub actor_id: ActorId,
    /// Authenticated tenant.
    pub tenant_id: TenantId,
    /// Authenticated device.
    pub device_id: DeviceId,
}

/// Untrusted operation before authenticated identity claims are checked.
#[derive(Clone, Copy, Debug)]
pub struct IncomingOperation<'a>(&'a OperationEnvelope);

impl<'a> IncomingOperation<'a> {
    /// Wraps one structurally validated but still untrusted operation.
    #[must_use]
    pub const fn new(operation: &'a OperationEnvelope) -> Self {
        Self(operation)
    }

    /// Checks every claimed identity against the connection-derived context.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError`] when tenant, actor, or device claims differ.
    pub fn authenticate(
        self,
        auth: &AuthContext,
    ) -> Result<AuthenticatedOperation<'a>, ExecutionError> {
        if self.0.tenant_id != auth.tenant_id
            || self.0.actor_id != auth.actor_id
            || self.0.device_id != auth.device_id
        {
            return Err(ExecutionError::identity_mismatch(
                "operation identity does not match the authenticated context",
            ));
        }
        Ok(AuthenticatedOperation(self.0))
    }
}

/// Operation whose identity claims match the authenticated connection.
#[derive(Clone, Copy, Debug)]
pub struct AuthenticatedOperation<'a>(&'a OperationEnvelope);

impl<'a> AuthenticatedOperation<'a> {
    /// Borrows the underlying wire envelope for application authorization.
    #[must_use]
    pub const fn envelope(self) -> &'a OperationEnvelope {
        self.0
    }

    /// Marks successful application authorization. Callers should invoke this only after their
    /// policy has approved the operation.
    #[must_use]
    pub const fn authorize(self) -> AuthorizedOperation<'a> {
        AuthorizedOperation(self.0)
    }
}

/// Authenticated operation approved by application authorization policy.
#[derive(Clone, Copy, Debug)]
pub struct AuthorizedOperation<'a>(&'a OperationEnvelope);

impl<'a> AuthorizedOperation<'a> {
    /// Borrows the underlying envelope for conflict and authoritative-state validation.
    #[must_use]
    pub const fn envelope(self) -> &'a OperationEnvelope {
        self.0
    }

    /// Marks successful structural, business-precondition, and conflict validation.
    #[must_use]
    pub const fn validate(self) -> ValidatedOperation<'a> {
        ValidatedOperation(self.0)
    }
}

/// Authorized operation whose authoritative preconditions were validated.
#[derive(Clone, Copy, Debug)]
pub struct ValidatedOperation<'a>(&'a OperationEnvelope);

impl<'a> ValidatedOperation<'a> {
    /// Converts a validated operation into the only type accepted by execution.
    #[must_use]
    pub const fn executable(self) -> ExecutableOperation<'a> {
        ExecutableOperation(self.0)
    }
}

/// Fully planned operation accepted by [`OperationExecutor::execute`].
#[derive(Clone, Copy, Debug)]
pub struct ExecutableOperation<'a>(&'a OperationEnvelope);

impl<'a> ExecutableOperation<'a> {
    /// Borrows the underlying envelope for decoding and domain execution.
    #[must_use]
    pub const fn envelope(self) -> &'a OperationEnvelope {
        self.0
    }
}

/// Current authoritative snapshot passed into an application handler.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrentEntity {
    /// Current version.
    pub version: aequora_types::EntityVersion,
    /// Opaque application-owned snapshot bytes.
    pub payload: Vec<u8>,
    /// Whether the snapshot is a tombstone.
    pub tombstone: bool,
}

/// Mutation produced only after application authorization and validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoritativeMutation {
    /// Authoritative payload to persist and journal.
    pub payload: Vec<u8>,
    /// Upsert or tombstone.
    pub change_kind: ChangeKind,
}

/// Typed application rejection.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{message}")]
pub struct ExecutionError {
    /// Stable rejection category.
    pub code: RejectionCode,
    /// Bounded, non-sensitive explanation.
    pub message: String,
}

impl ExecutionError {
    /// Constructs an authenticated-identity mismatch.
    #[must_use]
    pub fn identity_mismatch(message: impl Into<String>) -> Self {
        Self {
            code: RejectionCode::IdentityMismatch,
            message: message.into(),
        }
    }

    /// Constructs an authorization rejection.
    #[must_use]
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            code: RejectionCode::Unauthorized,
            message: message.into(),
        }
    }

    /// Constructs a business-rule rejection.
    #[must_use]
    pub fn business_rule(message: impl Into<String>) -> Self {
        Self {
            code: RejectionCode::BusinessRule,
            message: message.into(),
        }
    }

    /// Constructs a malformed or unknown operation rejection.
    #[must_use]
    pub fn invalid_operation(message: impl Into<String>) -> Self {
        Self {
            code: RejectionCode::InvalidOperation,
            message: message.into(),
        }
    }

    /// Constructs an unsupported domain-schema rejection.
    #[must_use]
    pub fn schema_incompatible(message: impl Into<String>) -> Self {
        Self {
            code: RejectionCode::SchemaIncompatible,
            message: message.into(),
        }
    }
}

/// Application operation registry/dispatcher.
///
/// Implementations decode `operation.payload` according to `operation_kind`, authorize
/// against `AuthContext`, validate business rules, and return an authoritative mutation.
#[async_trait]
pub trait OperationExecutor: Send + Sync {
    /// Authorizes a requested partial sync scope before any entity or journal data is read.
    async fn authorize_scope(
        &self,
        auth: &AuthContext,
        session: &SessionMetadata,
    ) -> Result<(), ExecutionError>;

    /// Authorizes an operation before authoritative state is disclosed or conflict-tested.
    async fn authorize<'a>(
        &self,
        auth: &AuthContext,
        operation: AuthenticatedOperation<'a>,
    ) -> Result<AuthorizedOperation<'a>, ExecutionError>;

    /// Validates business rules and executes an authorized operation into a
    /// store-independent mutation.
    async fn execute(
        &self,
        auth: &AuthContext,
        operation: ExecutableOperation<'_>,
        current: Option<&CurrentEntity>,
    ) -> Result<AuthoritativeMutation, ExecutionError>;
}

/// Strongly typed domain command registered with [`OperationRegistry`].
pub trait DomainOperation: DeserializeOwned + Send + Sync + 'static {
    /// Stable operation kind on the wire.
    const KIND: u16;
    /// Current application payload schema understood by its handler.
    const CURRENT_SCHEMA: u16;
}

/// Authorization and execution logic for one typed domain command.
#[async_trait]
pub trait OperationHandler<O>: Send + Sync
where
    O: DomainOperation,
{
    /// Authorizes the decoded command before authoritative entity state is read.
    async fn authorize(
        &self,
        auth: &AuthContext,
        operation: &O,
        envelope: &OperationEnvelope,
    ) -> Result<(), ExecutionError>;

    /// Validates and executes the decoded command against current authoritative state.
    async fn execute(
        &self,
        auth: &AuthContext,
        operation: &O,
        envelope: &OperationEnvelope,
        current: Option<&CurrentEntity>,
    ) -> Result<AuthoritativeMutation, ExecutionError>;
}

/// Application authorization for a requested partial synchronization scope.
#[async_trait]
pub trait ScopeAuthorizer: Send + Sync {
    /// Authorizes all requested opaque partition selectors before data access.
    async fn authorize_scope(
        &self,
        auth: &AuthContext,
        session: &SessionMetadata,
    ) -> Result<(), ExecutionError>;
}

/// Pure adapter that upgrades one supported historical payload directly to the handler's
/// current schema. Database migrations remain separate.
pub trait PayloadMigrator: Send + Sync {
    /// Returns current-schema Postcard bytes for a historical payload.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError`] when the historical payload is malformed or cannot be upgraded.
    fn migrate(&self, from: SchemaVersion, payload: &[u8]) -> Result<Vec<u8>, ExecutionError>;
}

#[async_trait]
trait ErasedOperationHandler: Send + Sync {
    async fn authorize(
        &self,
        auth: &AuthContext,
        envelope: &OperationEnvelope,
    ) -> Result<(), ExecutionError>;

    async fn execute(
        &self,
        auth: &AuthContext,
        envelope: &OperationEnvelope,
        current: Option<&CurrentEntity>,
    ) -> Result<AuthoritativeMutation, ExecutionError>;
}

struct TypedHandler<O, H> {
    handler: H,
    minimum_schema: u16,
    migrator: Option<Arc<dyn PayloadMigrator>>,
    marker: std::marker::PhantomData<fn() -> O>,
}

impl<O, H> TypedHandler<O, H>
where
    O: DomainOperation,
{
    fn decode(&self, envelope: &OperationEnvelope) -> Result<O, ExecutionError> {
        let schema = envelope.schema_version.0;
        if schema < self.minimum_schema || schema > O::CURRENT_SCHEMA {
            return Err(ExecutionError::schema_incompatible(format!(
                "operation schema {schema} is outside supported range {}..={}",
                self.minimum_schema,
                O::CURRENT_SCHEMA
            )));
        }
        if schema == O::CURRENT_SCHEMA {
            return postcard::from_bytes(&envelope.payload).map_err(|_| {
                ExecutionError::invalid_operation("current operation payload is malformed")
            });
        }
        let migrator = self.migrator.as_ref().ok_or_else(|| {
            ExecutionError::schema_incompatible("historical operation requires a migration adapter")
        })?;
        let current = migrator.migrate(envelope.schema_version, &envelope.payload)?;
        postcard::from_bytes(&current).map_err(|_| {
            ExecutionError::invalid_operation("migrated operation payload is malformed")
        })
    }
}

#[async_trait]
impl<O, H> ErasedOperationHandler for TypedHandler<O, H>
where
    O: DomainOperation,
    H: OperationHandler<O>,
{
    async fn authorize(
        &self,
        auth: &AuthContext,
        envelope: &OperationEnvelope,
    ) -> Result<(), ExecutionError> {
        let operation = self.decode(envelope)?;
        self.handler.authorize(auth, &operation, envelope).await
    }

    async fn execute(
        &self,
        auth: &AuthContext,
        envelope: &OperationEnvelope,
        current: Option<&CurrentEntity>,
    ) -> Result<AuthoritativeMutation, ExecutionError> {
        let operation = self.decode(envelope)?;
        self.handler
            .execute(auth, &operation, envelope, current)
            .await
    }
}

/// Invalid typed registration configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RegistrationError {
    /// An operation kind already has a handler.
    #[error("operation kind {0} is already registered")]
    Duplicate(u16),
    /// The minimum supported schema is zero or newer than the command's current schema.
    #[error("operation schema window is invalid")]
    SchemaWindow,
}

/// Typed operation dispatcher with an explicit scope authorizer and schema compatibility window.
pub struct OperationRegistry {
    scope_authorizer: Arc<dyn ScopeAuthorizer>,
    handlers: HashMap<u16, Arc<dyn ErasedOperationHandler>>,
}

impl OperationRegistry {
    /// Creates an empty registry. Requiring an authorizer prevents an accidental allow-all scope.
    #[must_use]
    pub fn new<A>(scope_authorizer: A) -> Self
    where
        A: ScopeAuthorizer + 'static,
    {
        Self {
            scope_authorizer: Arc::new(scope_authorizer),
            handlers: HashMap::new(),
        }
    }

    /// Registers a handler for only its current payload schema.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError::Duplicate`] when the operation kind is already registered.
    pub fn register<O, H>(&mut self, handler: H) -> Result<&mut Self, RegistrationError>
    where
        O: DomainOperation,
        H: OperationHandler<O> + 'static,
    {
        self.register_inner::<O, H>(handler, O::CURRENT_SCHEMA, None)
    }

    /// Registers a handler and direct migration adapter for a bounded historical schema window.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationError`] for duplicate kinds or an invalid compatibility window.
    pub fn register_with_migration<O, H, M>(
        &mut self,
        minimum_schema: u16,
        handler: H,
        migrator: M,
    ) -> Result<&mut Self, RegistrationError>
    where
        O: DomainOperation,
        H: OperationHandler<O> + 'static,
        M: PayloadMigrator + 'static,
    {
        self.register_inner::<O, H>(handler, minimum_schema, Some(Arc::new(migrator)))
    }

    fn register_inner<O, H>(
        &mut self,
        handler: H,
        minimum_schema: u16,
        migrator: Option<Arc<dyn PayloadMigrator>>,
    ) -> Result<&mut Self, RegistrationError>
    where
        O: DomainOperation,
        H: OperationHandler<O> + 'static,
    {
        if minimum_schema == 0 || minimum_schema > O::CURRENT_SCHEMA {
            return Err(RegistrationError::SchemaWindow);
        }
        if self.handlers.contains_key(&O::KIND) {
            return Err(RegistrationError::Duplicate(O::KIND));
        }
        self.handlers.insert(
            O::KIND,
            Arc::new(TypedHandler::<O, H> {
                handler,
                minimum_schema,
                migrator,
                marker: std::marker::PhantomData,
            }),
        );
        Ok(self)
    }

    fn handler(
        &self,
        operation_kind: u16,
    ) -> Result<&Arc<dyn ErasedOperationHandler>, ExecutionError> {
        self.handlers.get(&operation_kind).ok_or_else(|| {
            ExecutionError::invalid_operation(format!(
                "operation kind {operation_kind} is not registered"
            ))
        })
    }
}

#[async_trait]
impl OperationExecutor for OperationRegistry {
    async fn authorize_scope(
        &self,
        auth: &AuthContext,
        session: &SessionMetadata,
    ) -> Result<(), ExecutionError> {
        self.scope_authorizer.authorize_scope(auth, session).await
    }

    async fn authorize<'a>(
        &self,
        auth: &AuthContext,
        operation: AuthenticatedOperation<'a>,
    ) -> Result<AuthorizedOperation<'a>, ExecutionError> {
        let envelope = operation.envelope();
        self.handler(envelope.operation_kind.0)?
            .authorize(auth, envelope)
            .await?;
        Ok(operation.authorize())
    }

    async fn execute(
        &self,
        auth: &AuthContext,
        operation: ExecutableOperation<'_>,
        current: Option<&CurrentEntity>,
    ) -> Result<AuthoritativeMutation, ExecutionError> {
        let envelope = operation.envelope();
        self.handler(envelope.operation_kind.0)?
            .execute(auth, envelope, current)
            .await
    }
}

/// Invalid intra-batch operation dependency graph.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DependencyError {
    /// An operation ID appeared more than once.
    #[error("operation {0} appears more than once in the dependency graph")]
    Duplicate(aequora_types::OperationId),
    /// Intra-batch dependencies contain a cycle. No operation should execute.
    #[error("operation dependency graph contains a cycle")]
    Cycle,
}

/// Deterministic topological execution plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyPlan {
    ordered_indices: Vec<usize>,
    groups: Vec<Vec<usize>>,
}

impl DependencyPlan {
    /// Operation indexes in stable topological order.
    #[must_use]
    pub fn ordered_indices(&self) -> &[usize] {
        &self.ordered_indices
    }

    /// Independent topological levels. CPU-only validation within one level may run in parallel.
    #[must_use]
    pub fn groups(&self) -> &[Vec<usize>] {
        &self.groups
    }
}

/// Builds a stable `O(V + E)` dependency plan for operations contained in one batch.
/// Dependencies outside the batch remain server-ledger prerequisites and are not graph edges.
///
/// # Errors
///
/// Returns [`DependencyError::Duplicate`] for duplicate operation IDs and
/// [`DependencyError::Cycle`] when no topological execution order exists.
pub fn plan_dependencies(
    operations: &[OperationEnvelope],
) -> Result<DependencyPlan, DependencyError> {
    let mut positions = HashMap::with_capacity(operations.len());
    for (index, operation) in operations.iter().enumerate() {
        if positions.insert(operation.operation_id, index).is_some() {
            return Err(DependencyError::Duplicate(operation.operation_id));
        }
    }

    let mut indegree = vec![0_usize; operations.len()];
    let mut outgoing = vec![Vec::new(); operations.len()];
    for (index, operation) in operations.iter().enumerate() {
        for dependency in &operation.metadata.dependencies {
            if let Some(&dependency_index) = positions.get(dependency) {
                indegree[index] = indegree[index].saturating_add(1);
                outgoing[dependency_index].push(index);
            }
        }
    }

    let mut ready: BinaryHeap<Reverse<usize>> = indegree
        .iter()
        .enumerate()
        .filter_map(|(index, degree)| (*degree == 0).then_some(Reverse(index)))
        .collect();
    let mut ordered_indices = Vec::with_capacity(operations.len());
    let mut groups = Vec::new();
    while !ready.is_empty() {
        let mut group = Vec::with_capacity(ready.len());
        while let Some(Reverse(index)) = ready.pop() {
            group.push(index);
        }
        for &index in &group {
            ordered_indices.push(index);
            for &dependent in &outgoing[index] {
                indegree[dependent] = indegree[dependent].saturating_sub(1);
                if indegree[dependent] == 0 {
                    ready.push(Reverse(dependent));
                }
            }
        }
        groups.push(group);
    }
    if ordered_indices.len() != operations.len() {
        return Err(DependencyError::Cycle);
    }
    Ok(DependencyPlan {
        ordered_indices,
        groups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_protocol::{OperationKind, OperationMetadata};
    use aequora_types::{
        EntityId, EntityRef, EntityType, HybridTimestamp, NodeId, OperationId, ProtocolVersion,
    };
    use serde::{Deserialize, Serialize};

    #[derive(Deserialize, Serialize)]
    struct CurrentCommand {
        value: u16,
    }

    impl DomainOperation for CurrentCommand {
        const KIND: u16 = 9;
        const CURRENT_SCHEMA: u16 = 2;
    }

    #[derive(Deserialize, Serialize)]
    struct LegacyCommand {
        value: u8,
    }

    struct CommandMigration;

    impl PayloadMigrator for CommandMigration {
        fn migrate(&self, from: SchemaVersion, payload: &[u8]) -> Result<Vec<u8>, ExecutionError> {
            if from != SchemaVersion(1) {
                return Err(ExecutionError::schema_incompatible(
                    "only schema one can be migrated",
                ));
            }
            let legacy: LegacyCommand = postcard::from_bytes(payload)
                .map_err(|_| ExecutionError::invalid_operation("legacy payload is malformed"))?;
            postcard::to_stdvec(&CurrentCommand {
                value: u16::from(legacy.value),
            })
            .map_err(|_| ExecutionError::invalid_operation("migration encoding failed"))
        }
    }

    struct AllowScope;

    #[async_trait]
    impl ScopeAuthorizer for AllowScope {
        async fn authorize_scope(
            &self,
            _auth: &AuthContext,
            _session: &SessionMetadata,
        ) -> Result<(), ExecutionError> {
            Ok(())
        }
    }

    struct CurrentHandler;

    #[async_trait]
    impl OperationHandler<CurrentCommand> for CurrentHandler {
        async fn authorize(
            &self,
            _auth: &AuthContext,
            _operation: &CurrentCommand,
            _envelope: &OperationEnvelope,
        ) -> Result<(), ExecutionError> {
            Ok(())
        }

        async fn execute(
            &self,
            _auth: &AuthContext,
            operation: &CurrentCommand,
            _envelope: &OperationEnvelope,
            _current: Option<&CurrentEntity>,
        ) -> Result<AuthoritativeMutation, ExecutionError> {
            Ok(AuthoritativeMutation {
                payload: operation.value.to_be_bytes().to_vec(),
                change_kind: ChangeKind::Upsert,
            })
        }
    }

    fn envelope(payload: Vec<u8>, schema_version: SchemaVersion) -> OperationEnvelope {
        OperationEnvelope {
            protocol_version: ProtocolVersion::V1,
            operation_id: OperationId::new(),
            tenant_id: TenantId::new(),
            actor_id: ActorId::new(),
            device_id: DeviceId::new(),
            entity: EntityRef {
                entity_type: EntityType::new(1).unwrap_or_else(|error| panic!("{error}")),
                entity_id: EntityId::new(),
            },
            base_version: None,
            created_at: HybridTimestamp {
                physical_ms: 1,
                logical: 0,
                node: NodeId::new(),
            },
            schema_version,
            operation_kind: OperationKind(CurrentCommand::KIND),
            payload,
            metadata: OperationMetadata::default(),
        }
    }

    #[tokio::test]
    async fn registry_migrates_only_the_explicit_schema_window() {
        let mut registry = OperationRegistry::new(AllowScope);
        registry
            .register_with_migration::<CurrentCommand, _, _>(1, CurrentHandler, CommandMigration)
            .unwrap_or_else(|error| panic!("{error}"));
        let legacy = LegacyCommand { value: 42 };
        let operation = envelope(
            postcard::to_stdvec(&legacy).unwrap_or_else(|error| panic!("{error}")),
            SchemaVersion(1),
        );
        let auth = AuthContext {
            actor_id: operation.actor_id,
            tenant_id: operation.tenant_id,
            device_id: operation.device_id,
        };

        let authenticated = IncomingOperation::new(&operation)
            .authenticate(&auth)
            .unwrap_or_else(|error| panic!("{error}"));
        let authorized = registry
            .authorize(&auth, authenticated)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let mutation = registry
            .execute(&auth, authorized.validate().executable(), None)
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(mutation.payload, 42_u16.to_be_bytes());

        let unsupported = envelope(Vec::new(), SchemaVersion(3));
        let unsupported_auth = AuthContext {
            actor_id: unsupported.actor_id,
            tenant_id: unsupported.tenant_id,
            device_id: unsupported.device_id,
        };
        let unsupported = IncomingOperation::new(&unsupported)
            .authenticate(&unsupported_auth)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(matches!(
            registry.authorize(&unsupported_auth, unsupported).await,
            Err(ExecutionError {
                code: RejectionCode::SchemaIncompatible,
                ..
            })
        ));
    }
}
