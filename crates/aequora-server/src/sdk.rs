//! High-level server SDK independent from Axum and physical database types.

use aequora_adapter_sdk::{
    AdapterError, Authenticator, AuthorityService, Credential, DomainContext, DomainHandler,
    DomainOutcome,
};
use aequora_operation::{EncodedOperation, OperationKind};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    fmt,
    sync::Arc,
};
use thiserror::Error;

/// Domain handler registration failure detected before traffic is served.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[non_exhaustive]
pub enum DomainRegistrationError {
    /// More than one handler claimed an operation kind.
    #[error("operation kind {kind} is registered more than once")]
    DuplicateKind {
        /// Duplicated stable kind.
        kind: OperationKind,
    },
    /// Handler did not declare a certified consistency profile.
    #[error("operation kind {kind} has no consistency profile")]
    MissingProfile {
        /// Operation kind missing its profile.
        kind: OperationKind,
    },
    /// Handler declared schema version zero.
    #[error("operation kind {kind} has reserved schema version zero")]
    MissingSchema {
        /// Operation kind missing a usable schema version.
        kind: OperationKind,
    },
}

/// Immutable, validated domain-handler registry.
#[derive(Clone)]
pub struct DomainRegistry {
    handlers: Arc<BTreeMap<OperationKind, Arc<dyn DomainHandler>>>,
}

impl fmt::Debug for DomainRegistry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DomainRegistry")
            .field("operation_kinds", &self.handlers.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl DomainRegistry {
    /// Starts an empty registry builder.
    #[must_use]
    pub fn builder() -> DomainRegistryBuilder {
        DomainRegistryBuilder::default()
    }

    fn handler(&self, kind: OperationKind) -> Option<&Arc<dyn DomainHandler>> {
        self.handlers.get(&kind)
    }
}

/// Validating domain registry builder.
#[derive(Default)]
pub struct DomainRegistryBuilder {
    handlers: BTreeMap<OperationKind, Arc<dyn DomainHandler>>,
    error: Option<DomainRegistrationError>,
}

impl DomainRegistryBuilder {
    /// Registers one explicitly profiled domain handler.
    #[must_use]
    pub fn register(mut self, handler: impl DomainHandler + 'static) -> Self {
        let kind = handler.operation_kind();
        if self.error.is_some() {
            return self;
        }
        if handler.profile().is_none() {
            self.error = Some(DomainRegistrationError::MissingProfile { kind });
        } else if handler.schema_version() == 0 {
            self.error = Some(DomainRegistrationError::MissingSchema { kind });
        } else {
            match self.handlers.entry(kind) {
                Entry::Vacant(entry) => {
                    entry.insert(Arc::new(handler));
                }
                Entry::Occupied(_) => {
                    self.error = Some(DomainRegistrationError::DuplicateKind { kind });
                }
            }
        }
        self
    }

    /// Freezes the registry after validating duplicate ownership, schema, and profile metadata.
    ///
    /// # Errors
    ///
    /// Returns the first deterministic registration error.
    pub fn build(self) -> Result<DomainRegistry, DomainRegistrationError> {
        if let Some(error) = self.error {
            Err(error)
        } else {
            Ok(DomainRegistry {
                handlers: Arc::new(self.handlers),
            })
        }
    }
}

/// Stable server-side operation outcome. Business rejection is not a system failure.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ServerOperationOutcome {
    /// Authority committed the accepted handler result.
    Accepted(Vec<u8>),
    /// Domain business rules rejected the intent.
    BusinessRejected {
        /// Stable application-defined code.
        code: Arc<str>,
    },
    /// Domain requested explicit conflict resolution.
    Conflict {
        /// Stable application-defined reason.
        reason: Arc<str>,
    },
}

/// Stable high-level server error.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[non_exhaustive]
pub enum StableServerError {
    /// Required builder field is absent.
    #[error("required server field `{field}` is missing")]
    MissingConfiguration {
        /// Missing semantic field.
        field: &'static str,
    },
    /// Operation kind is not registered.
    #[error("operation kind {kind} is not registered")]
    UnknownOperation {
        /// Unknown stable operation kind.
        kind: OperationKind,
    },
    /// Operation schema is incompatible with its registered handler.
    #[error("operation kind {kind} schema {received} does not match registered schema {expected}")]
    SchemaMismatch {
        /// Stable operation kind.
        kind: OperationKind,
        /// Received schema version.
        received: u16,
        /// Registered schema version.
        expected: u16,
    },
    /// Authentication, handler, or authority adapter failed.
    #[error(transparent)]
    Adapter(#[from] AdapterError),
    /// A newer handler outcome is not understood by this server release.
    #[error("domain handler returned an unsupported outcome variant")]
    UnsupportedOutcome,
}

/// Runtime-validated server builder.
#[derive(Default)]
pub struct AequoraServerBuilder {
    authority: Option<Arc<dyn AuthorityService>>,
    registry: Option<DomainRegistry>,
    authenticator: Option<Arc<dyn Authenticator>>,
}

impl AequoraServerBuilder {
    /// Installs the authoritative commit service.
    #[must_use]
    pub fn authority_store(mut self, authority: impl AuthorityService + 'static) -> Self {
        self.authority = Some(Arc::new(authority));
        self
    }

    /// Installs a pre-shared authoritative commit service.
    #[must_use]
    pub fn shared_authority_store(mut self, authority: Arc<dyn AuthorityService>) -> Self {
        self.authority = Some(authority);
        self
    }

    /// Installs the validated application domain registry.
    #[must_use]
    pub fn registry(mut self, registry: DomainRegistry) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Alias matching domain-oriented examples.
    #[must_use]
    pub fn domain(self, registry: DomainRegistry) -> Self {
        self.registry(registry)
    }

    /// Installs the authentication provider.
    #[must_use]
    pub fn authenticator(mut self, authenticator: impl Authenticator + 'static) -> Self {
        self.authenticator = Some(Arc::new(authenticator));
        self
    }

    /// Installs a pre-shared authentication provider.
    #[must_use]
    pub fn shared_authenticator(mut self, authenticator: Arc<dyn Authenticator>) -> Self {
        self.authenticator = Some(authenticator);
        self
    }

    /// Validates required fields before any request traffic is accepted.
    ///
    /// # Errors
    ///
    /// Returns a stable validation error naming the first missing field.
    pub fn build(self) -> Result<AequoraServer, StableServerError> {
        Ok(AequoraServer {
            inner: Arc::new(ServerInner {
                authority: self
                    .authority
                    .ok_or(StableServerError::MissingConfiguration {
                        field: "authority_store",
                    })?,
                registry: self
                    .registry
                    .ok_or(StableServerError::MissingConfiguration { field: "registry" })?,
                authenticator: self.authenticator.ok_or(
                    StableServerError::MissingConfiguration {
                        field: "authenticator",
                    },
                )?,
            }),
        })
    }
}

struct ServerInner {
    authority: Arc<dyn AuthorityService>,
    registry: DomainRegistry,
    authenticator: Arc<dyn Authenticator>,
}

/// Cheaply clonable high-level server core. Transport integration belongs in `aequora-axum` or
/// another adapter crate.
#[derive(Clone)]
pub struct AequoraServer {
    inner: Arc<ServerInner>,
}

impl fmt::Debug for AequoraServer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AequoraServer")
            .field("registry", &self.inner.registry)
            .finish_non_exhaustive()
    }
}

impl AequoraServer {
    /// Starts a runtime-validated high-level server builder.
    #[must_use]
    pub fn builder() -> AequoraServerBuilder {
        AequoraServerBuilder::default()
    }

    /// Authenticates, dispatches, and authoritatively commits one semantic operation.
    ///
    /// # Errors
    ///
    /// Returns a stable authentication, registry, handler, schema, or authority failure.
    pub async fn execute(
        &self,
        credential: &Credential,
        operation: EncodedOperation,
    ) -> Result<ServerOperationOutcome, StableServerError> {
        let identity = self.inner.authenticator.authenticate(credential).await?;
        let kind = operation.kind();
        let handler = self
            .inner
            .registry
            .handler(kind)
            .ok_or(StableServerError::UnknownOperation { kind })?;
        let expected = handler.schema_version();
        let received = operation.schema_version().get();
        if received != expected {
            return Err(StableServerError::SchemaMismatch {
                kind,
                received,
                expected,
            });
        }
        let context = DomainContext {
            identity,
            operation_id: operation.operation_id(),
        };
        match handler.handle(context, operation.payload()).await? {
            DomainOutcome::Accepted(outcome) => self
                .inner
                .authority
                .commit(identity, operation, outcome)
                .await
                .map(ServerOperationOutcome::Accepted)
                .map_err(StableServerError::from),
            DomainOutcome::BusinessRejected { code } => {
                Ok(ServerOperationOutcome::BusinessRejected { code })
            }
            DomainOutcome::Conflict { reason } => Ok(ServerOperationOutcome::Conflict { reason }),
            _ => Err(StableServerError::UnsupportedOutcome),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aequora_adapter_sdk::{AdapterErrorKind, ClientIdentity, DomainContext};
    use aequora_operation::{Operation, OperationEncodingError, OperationSchemaVersion};
    use aequora_types::{DeviceId, TenantId};
    use async_trait::async_trait;

    struct Handler;

    #[async_trait]
    impl DomainHandler for Handler {
        fn operation_kind(&self) -> OperationKind {
            OperationKind::new(77).unwrap_or_else(|error| panic!("kind is valid: {error}"))
        }

        fn schema_version(&self) -> u16 {
            1
        }

        fn profile(&self) -> Option<&'static str> {
            Some("strong-aggregate")
        }

        async fn handle(
            &self,
            _context: DomainContext,
            payload: &[u8],
        ) -> Result<DomainOutcome, AdapterError> {
            Ok(DomainOutcome::Accepted(payload.to_vec()))
        }
    }

    struct Auth;

    #[async_trait]
    impl Authenticator for Auth {
        async fn authenticate(
            &self,
            _credential: &Credential,
        ) -> Result<ClientIdentity, AdapterError> {
            Ok(ClientIdentity {
                tenant_id: TenantId::new(),
                device_id: DeviceId::new(),
            })
        }
    }

    struct Authority;

    #[async_trait]
    impl AuthorityService for Authority {
        async fn commit(
            &self,
            _identity: ClientIdentity,
            _operation: EncodedOperation,
            outcome: Vec<u8>,
        ) -> Result<Vec<u8>, AdapterError> {
            Ok(outcome)
        }
    }

    struct TestOperation;

    impl Operation for TestOperation {
        type Outcome = ();

        const KIND: OperationKind = OperationKind::from_static(77);
        const SCHEMA_VERSION: OperationSchemaVersion = OperationSchemaVersion::from_static(1);

        fn encode(&self) -> Result<Vec<u8>, OperationEncodingError> {
            Ok(vec![7])
        }
    }

    #[test]
    fn duplicate_registration_fails_before_runtime_traffic() {
        let result = DomainRegistry::builder()
            .register(Handler)
            .register(Handler)
            .build();
        assert!(matches!(
            result,
            Err(DomainRegistrationError::DuplicateKind { .. })
        ));
    }

    #[tokio::test]
    async fn server_dispatches_without_http_or_database_types() {
        let registry = DomainRegistry::builder()
            .register(Handler)
            .build()
            .unwrap_or_else(|error| panic!("registry should build: {error}"));
        let server = AequoraServer::builder()
            .authority_store(Authority)
            .registry(registry)
            .authenticator(Auth)
            .build()
            .unwrap_or_else(|error| panic!("server should build: {error}"));
        let operation = EncodedOperation::from_operation(&TestOperation)
            .unwrap_or_else(|error| panic!("operation should encode: {error}"));
        assert_eq!(
            server.execute(&Credential::new("test"), operation).await,
            Ok(ServerOperationOutcome::Accepted(vec![7]))
        );
    }

    #[test]
    fn adapter_errors_remain_system_failures() {
        let error = AdapterError::new(AdapterErrorKind::Internal, "redacted");
        assert!(matches!(
            StableServerError::from(error),
            StableServerError::Adapter(_)
        ));
    }
}
