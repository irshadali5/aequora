//! Executable, database- and transport-neutral model of Aequora's semantic protocol.
//!
//! This crate models commits, retries, lost responses, duplicate delivery, client crashes, and
//! two-client conflicts. It deliberately does not model HTTP, SQL, Axum, or a particular local
//! database. Every transition checks the normative invariant registry.

use aequora_invariants::InvariantId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;

/// Version of the executable model and its replay trace schema.
pub const MODEL_VERSION: u32 = 3;

/// Bounded model client identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ClientId(pub u8);

/// Bounded model operation identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ModelOperationId(pub u8);

/// Bounded root correlation identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ModelCorrelationId(pub u8);

/// Bounded authoritative event identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ModelEventId(pub u64);

/// Bounded model entity identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ModelEntityId(pub u8);

/// Semantic entity state used by the abstract model.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct CanonicalEntity {
    /// Monotonic authoritative version.
    pub version: u64,
    /// Domain-independent value standing in for canonical payload state.
    pub value: i64,
    /// Whether the entity is authoritatively deleted.
    pub tombstone: bool,
}

/// Typed local operation retained until an authoritative outcome is reconciled.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ModelOperation {
    /// Stable idempotency identity.
    pub id: ModelOperationId,
    /// Retry-stable root action correlation.
    pub correlation_id: ModelCorrelationId,
    /// Target entity.
    pub entity: ModelEntityId,
    /// Authority version observed when the operation was created.
    pub expected_version: Option<u64>,
    /// Proposed replacement value.
    pub value: i64,
}

/// One immutable authoritative journal event.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ModelEvent {
    /// Stable identity distinct from journal ordering.
    pub event_id: ModelEventId,
    /// Root action inherited from the operation.
    pub correlation_id: ModelCorrelationId,
    /// Direct semantic cause of this primary event.
    pub caused_by: ModelOperationId,
    /// Contiguous authority sequence.
    pub sequence: u64,
    /// Operation that created this event.
    pub operation_id: ModelOperationId,
    /// Target entity.
    pub entity: ModelEntityId,
    /// Resulting entity version.
    pub version: u64,
    /// Resulting canonical value.
    pub value: i64,
    /// Resulting deletion state.
    pub tombstone: bool,
}

/// Durable outcome recorded by the authority operation ledger.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum ModelOutcome {
    /// The operation committed at this journal sequence and entity version.
    Accepted { sequence: u64, version: u64 },
    /// The operation lost an optimistic-version race and had no logical effect.
    Conflict { current_version: Option<u64> },
}

/// Abstract durable client state.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ClientModel {
    /// Last reconciled authoritative entity state.
    pub authoritative_state: BTreeMap<ModelEntityId, CanonicalEntity>,
    /// User-visible state including provisional local intent.
    pub optimistic_state: BTreeMap<ModelEntityId, CanonicalEntity>,
    /// Durable pending intent in creation order.
    pub outbox: VecDeque<ModelOperation>,
    /// Authority epoch attached to the cursor.
    pub cursor_epoch: u64,
    /// Highest contiguously reconciled authority sequence.
    pub cursor: u64,
    /// Events already applied, retained to prove idempotent reconciliation.
    pub applied_events: BTreeSet<u64>,
    /// Whether the client can currently submit work.
    pub online: bool,
    /// Whether volatile client execution is stopped.
    pub crashed: bool,
}

impl Default for ClientModel {
    fn default() -> Self {
        Self {
            authoritative_state: BTreeMap::new(),
            optimistic_state: BTreeMap::new(),
            outbox: VecDeque::new(),
            cursor_epoch: 1,
            cursor: 0,
            applied_events: BTreeSet::new(),
            online: true,
            crashed: false,
        }
    }
}

/// Abstract durable authority state.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ServerModel {
    /// Canonical authority state.
    pub state: BTreeMap<ModelEntityId, CanonicalEntity>,
    /// Durable idempotency ledger.
    pub applied_operations: BTreeMap<ModelOperationId, ModelOutcome>,
    /// Original retry-stable correlation recorded for every operation result.
    pub operation_correlations: BTreeMap<ModelOperationId, ModelCorrelationId>,
    /// Immutable authoritative journal.
    pub journal: Vec<ModelEvent>,
    /// Next journal sequence to allocate.
    pub next_sequence: u64,
    /// Current fenced authority timeline.
    pub authority_epoch: u64,
    /// Whether volatile server execution is stopped.
    pub crashed: bool,
}

impl Default for ServerModel {
    fn default() -> Self {
        Self {
            state: BTreeMap::new(),
            applied_operations: BTreeMap::new(),
            operation_correlations: BTreeMap::new(),
            journal: Vec::new(),
            next_sequence: 1,
            authority_epoch: 1,
            crashed: false,
        }
    }
}

/// Request delayed in the intentionally unreliable abstract network.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ModelRequest {
    /// Client expecting the result.
    pub client: ClientId,
    /// Optional operation; `None` represents a pull-only exchange.
    pub operation: Option<ModelOperation>,
    /// Cursor at request creation, used to construct an incremental response.
    pub cursor: u64,
    /// Authority timeline claimed by the client.
    pub authority_epoch: u64,
}

/// Response delayed in the intentionally unreliable abstract network.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ModelResponse {
    /// Client that must reconcile the response.
    pub client: ClientId,
    /// Operation result, absent for a pull-only exchange.
    pub outcome: Option<(ModelOperationId, ModelOutcome)>,
    /// Contiguous authority journal suffix after the request cursor.
    pub changes: Vec<ModelEvent>,
    /// Authority epoch in which the response was produced.
    pub authority_epoch: u64,
}

/// Network state with explicit delay, duplication, dropping, and reordering queues.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct NetworkModel {
    /// Requests awaiting arbitrary delivery order.
    pub requests: Vec<ModelRequest>,
    /// Responses awaiting arbitrary delivery order.
    pub responses: Vec<ModelResponse>,
    /// Whether new sends and deliveries are possible.
    pub connected: bool,
}

impl Default for NetworkModel {
    fn default() -> Self {
        Self {
            requests: Vec::new(),
            responses: Vec::new(),
            connected: true,
        }
    }
}

/// Complete semantic state explored by deterministic and exhaustive checks.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Model {
    /// Authority state.
    pub server: ServerModel,
    /// Client states in stable identity order.
    pub clients: BTreeMap<ClientId, ClientModel>,
    /// Unreliable network queues.
    pub network: NetworkModel,
}

impl Model {
    /// Creates an empty model with the requested bounded client count.
    #[must_use]
    pub fn new(client_count: u8) -> Self {
        let clients = (0..client_count)
            .map(|id| (ClientId(id), ClientModel::default()))
            .collect();
        Self {
            server: ServerModel::default(),
            clients,
            network: NetworkModel::default(),
        }
    }

    /// Applies one deterministic semantic action and checks all modeled invariants.
    ///
    /// # Errors
    ///
    /// Returns an action error when its preconditions are not met or an invariant violation when
    /// the resulting state is unsafe.
    pub fn apply(&mut self, action: ModelAction) -> Result<(), ModelError> {
        self.apply_unchecked(action)?;
        self.check_invariants().map_err(ModelError::Invariant)
    }

    /// Stable content hash used for visited-state detection and failure identity.
    ///
    /// # Errors
    ///
    /// Returns a serialization error only if the versioned model cannot be encoded as RON.
    pub fn deterministic_hash(&self) -> Result<[u8; 32], ron::Error> {
        let encoded = ron::to_string(self)?;
        Ok(*blake3::hash(encoded.as_bytes()).as_bytes())
    }

    /// Checks every invariant currently represented by this bounded model.
    ///
    /// # Errors
    ///
    /// Returns the first stable invariant violation in registry order.
    pub fn check_invariants(&self) -> Result<(), InvariantViolation> {
        self.check_idempotent_authority()?;
        self.check_local_intent_atomicity()?;
        self.check_authoritative_publication_atomicity()?;
        self.check_cursor_safety()?;
        self.check_version_monotonicity()?;
        self.check_retry_preservation()?;
        self.check_reconciliation_idempotency()?;
        self.check_timeline_safety()?;
        self.check_lineage()?;
        Ok(())
    }

    /// Enabled actions under the supplied exploration bounds.
    #[must_use]
    pub fn enabled_actions(&self, bounds: SearchBounds) -> Vec<ModelAction> {
        let mut actions = Vec::new();
        for (&client_id, client) in &self.clients {
            if client.crashed {
                actions.push(ModelAction::RestartClient(client_id));
            } else {
                actions.push(ModelAction::CrashClient(client_id));
                if client.online && self.network.connected {
                    if !client.outbox.is_empty()
                        && self.network.requests.len() < bounds.max_requests
                    {
                        actions.push(ModelAction::SendOperation(client_id));
                    }
                    if self.network.requests.len() < bounds.max_requests {
                        actions.push(ModelAction::Pull(client_id));
                    }
                }
            }
        }
        if self.network.connected && !self.server.crashed {
            actions.extend((0..self.network.requests.len()).map(ModelAction::DeliverRequest));
        }
        if self.network.connected {
            actions.extend((0..self.network.responses.len()).map(ModelAction::DeliverResponse));
        }
        if self.network.requests.len() < bounds.max_requests {
            actions.extend((0..self.network.requests.len()).map(ModelAction::DuplicateRequest));
        }
        actions.extend((0..self.network.responses.len()).map(ModelAction::DropResponse));
        if self.server.crashed {
            actions.push(ModelAction::RestartServer);
        } else {
            actions.push(ModelAction::CrashServer);
        }
        if self.network.connected {
            actions.push(ModelAction::DisconnectNetwork);
        } else {
            actions.push(ModelAction::ReconnectNetwork);
        }
        actions
    }

    fn apply_unchecked(&mut self, action: ModelAction) -> Result<(), ActionError> {
        match action {
            ModelAction::LocalMutate { client, operation } => self.local_mutate(client, operation),
            ModelAction::SendOperation(client) => self.send(client, true),
            ModelAction::Pull(client) => self.send(client, false),
            ModelAction::DuplicateRequest(index) => {
                let request = *self
                    .network
                    .requests
                    .get(index)
                    .ok_or(ActionError::MissingRequest(index))?;
                self.network.requests.push(request);
                Ok(())
            }
            ModelAction::DeliverRequest(index) => self.deliver_request(index),
            ModelAction::DropResponse(index) => {
                checked_remove(
                    &mut self.network.responses,
                    index,
                    ActionError::MissingResponse(index),
                )?;
                Ok(())
            }
            ModelAction::DeliverResponse(index) => self.deliver_response(index),
            ModelAction::CrashClient(client) => {
                self.client_mut(client)?.crashed = true;
                Ok(())
            }
            ModelAction::RestartClient(client) => {
                self.client_mut(client)?.crashed = false;
                Ok(())
            }
            ModelAction::CrashServer => {
                self.server.crashed = true;
                Ok(())
            }
            ModelAction::RestartServer => {
                self.server.crashed = false;
                Ok(())
            }
            ModelAction::DisconnectNetwork => {
                self.network.connected = false;
                Ok(())
            }
            ModelAction::ReconnectNetwork => {
                self.network.connected = true;
                Ok(())
            }
        }
    }

    fn local_mutate(
        &mut self,
        client_id: ClientId,
        operation: ModelOperation,
    ) -> Result<(), ActionError> {
        let client = self.client_mut(client_id)?;
        if client.crashed {
            return Err(ActionError::ClientUnavailable(client_id));
        }
        if client
            .outbox
            .iter()
            .any(|pending| pending.id == operation.id)
        {
            return Err(ActionError::DuplicateLocalOperation(operation.id));
        }
        let expected_version = client
            .authoritative_state
            .get(&operation.entity)
            .map(|entity| entity.version);
        if operation.expected_version != expected_version {
            return Err(ActionError::WrongExpectedVersion {
                expected: expected_version,
                supplied: operation.expected_version,
            });
        }
        let optimistic_version = expected_version.unwrap_or(0).saturating_add(1);
        client.optimistic_state.insert(
            operation.entity,
            CanonicalEntity {
                version: optimistic_version,
                value: operation.value,
                tombstone: false,
            },
        );
        client.outbox.push_back(operation);
        Ok(())
    }

    fn send(&mut self, client_id: ClientId, with_operation: bool) -> Result<(), ActionError> {
        if !self.network.connected {
            return Err(ActionError::NetworkDisconnected);
        }
        let client = self.client(client_id)?;
        if client.crashed || !client.online {
            return Err(ActionError::ClientUnavailable(client_id));
        }
        let operation = if with_operation {
            Some(
                *client
                    .outbox
                    .front()
                    .ok_or(ActionError::EmptyOutbox(client_id))?,
            )
        } else {
            None
        };
        self.network.requests.push(ModelRequest {
            client: client_id,
            operation,
            cursor: client.cursor,
            authority_epoch: client.cursor_epoch,
        });
        Ok(())
    }

    fn deliver_request(&mut self, index: usize) -> Result<(), ActionError> {
        if !self.network.connected {
            return Err(ActionError::NetworkDisconnected);
        }
        if self.server.crashed {
            return Err(ActionError::ServerUnavailable);
        }
        let request = checked_remove(
            &mut self.network.requests,
            index,
            ActionError::MissingRequest(index),
        )?;
        if request.authority_epoch != self.server.authority_epoch {
            return Err(ActionError::IncompatibleEpoch {
                client: request.authority_epoch,
                authority: self.server.authority_epoch,
            });
        }
        let outcome = request
            .operation
            .map(|operation| self.execute(operation).map(|result| (operation.id, result)))
            .transpose()?;
        let changes = self
            .server
            .journal
            .iter()
            .filter(|event| event.sequence > request.cursor)
            .copied()
            .collect();
        self.network.responses.push(ModelResponse {
            client: request.client,
            outcome,
            changes,
            authority_epoch: self.server.authority_epoch,
        });
        Ok(())
    }

    fn execute(&mut self, operation: ModelOperation) -> Result<ModelOutcome, ActionError> {
        if let Some(outcome) = self.server.applied_operations.get(&operation.id) {
            if self.server.operation_correlations.get(&operation.id)
                != Some(&operation.correlation_id)
            {
                return Err(ActionError::RetryLineageChanged(operation.id));
            }
            return Ok(*outcome);
        }
        let current_version = self
            .server
            .state
            .get(&operation.entity)
            .map(|entity| entity.version);
        let outcome = if current_version == operation.expected_version {
            let version = current_version.unwrap_or(0).saturating_add(1);
            let sequence = self.server.next_sequence;
            self.server.next_sequence = sequence.saturating_add(1);
            let entity = CanonicalEntity {
                version,
                value: operation.value,
                tombstone: false,
            };
            self.server.state.insert(operation.entity, entity);
            self.server.journal.push(ModelEvent {
                event_id: ModelEventId(sequence),
                correlation_id: operation.correlation_id,
                caused_by: operation.id,
                sequence,
                operation_id: operation.id,
                entity: operation.entity,
                version,
                value: operation.value,
                tombstone: false,
            });
            ModelOutcome::Accepted { sequence, version }
        } else {
            ModelOutcome::Conflict { current_version }
        };
        self.server
            .operation_correlations
            .insert(operation.id, operation.correlation_id);
        self.server.applied_operations.insert(operation.id, outcome);
        Ok(outcome)
    }

    fn deliver_response(&mut self, index: usize) -> Result<(), ActionError> {
        if !self.network.connected {
            return Err(ActionError::NetworkDisconnected);
        }
        let response = checked_remove(
            &mut self.network.responses,
            index,
            ActionError::MissingResponse(index),
        )?;
        let client = self.client_mut(response.client)?;
        if client.crashed {
            return Err(ActionError::ClientUnavailable(response.client));
        }
        if client.cursor_epoch != response.authority_epoch {
            return Err(ActionError::IncompatibleEpoch {
                client: client.cursor_epoch,
                authority: response.authority_epoch,
            });
        }
        for event in response.changes {
            if event.sequence <= client.cursor {
                continue;
            }
            let expected = client.cursor.saturating_add(1);
            if event.sequence != expected {
                return Err(ActionError::NonContiguousResponse {
                    expected,
                    actual: event.sequence,
                });
            }
            client.authoritative_state.insert(
                event.entity,
                CanonicalEntity {
                    version: event.version,
                    value: event.value,
                    tombstone: event.tombstone,
                },
            );
            client.applied_events.insert(event.sequence);
            client.cursor = event.sequence;
        }
        if let Some((operation_id, _)) = response.outcome {
            client
                .outbox
                .retain(|operation| operation.id != operation_id);
        }
        client.optimistic_state = client.authoritative_state.clone();
        for pending in &client.outbox {
            let version = client
                .optimistic_state
                .get(&pending.entity)
                .map_or(1, |entity| entity.version.saturating_add(1));
            client.optimistic_state.insert(
                pending.entity,
                CanonicalEntity {
                    version,
                    value: pending.value,
                    tombstone: false,
                },
            );
        }
        Ok(())
    }

    fn check_idempotent_authority(&self) -> Result<(), InvariantViolation> {
        for operation_id in self.server.applied_operations.keys() {
            let effects = self
                .server
                .journal
                .iter()
                .filter(|event| event.operation_id == *operation_id)
                .count();
            if effects > 1 {
                return Err(violation(
                    InvariantId::IdempotentAuthority,
                    format!("operation {operation_id:?} has {effects} journal effects"),
                ));
            }
        }
        Ok(())
    }

    fn check_local_intent_atomicity(&self) -> Result<(), InvariantViolation> {
        for (&client_id, client) in &self.clients {
            for (&entity_id, optimistic) in &client.optimistic_state {
                if client.authoritative_state.get(&entity_id) != Some(optimistic)
                    && !client
                        .outbox
                        .iter()
                        .any(|operation| operation.entity == entity_id)
                {
                    return Err(violation(
                        InvariantId::LocalIntentAtomicity,
                        format!(
                            "client {client_id:?} has provisional entity {entity_id:?} without intent"
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    fn check_authoritative_publication_atomicity(&self) -> Result<(), InvariantViolation> {
        for event in &self.server.journal {
            let outcome = self.server.applied_operations.get(&event.operation_id);
            if outcome
                != Some(&ModelOutcome::Accepted {
                    sequence: event.sequence,
                    version: event.version,
                })
            {
                return Err(violation(
                    InvariantId::AuthoritativePublicationAtomicity,
                    format!(
                        "journal event {} has no matching ledger result",
                        event.sequence
                    ),
                ));
            }
        }
        for (operation_id, outcome) in &self.server.applied_operations {
            if let ModelOutcome::Accepted { sequence, version } = outcome {
                if self.server.journal.iter().any(|event| {
                    event.operation_id == *operation_id
                        && event.sequence == *sequence
                        && event.version == *version
                }) {
                    continue;
                }
                return Err(violation(
                    InvariantId::AuthoritativePublicationAtomicity,
                    format!("accepted operation {operation_id:?} has no matching journal event"),
                ));
            }
        }
        Ok(())
    }

    fn check_cursor_safety(&self) -> Result<(), InvariantViolation> {
        for (&client_id, client) in &self.clients {
            let expected: BTreeSet<_> = (1..=client.cursor).collect();
            if client.applied_events != expected {
                return Err(violation(
                    InvariantId::CursorSafety,
                    format!(
                        "client {client_id:?} cursor {} has non-contiguous applied events",
                        client.cursor
                    ),
                ));
            }
        }
        Ok(())
    }

    fn check_version_monotonicity(&self) -> Result<(), InvariantViolation> {
        let mut versions = BTreeMap::new();
        for event in &self.server.journal {
            let previous = versions.insert(event.entity, event.version).unwrap_or(0);
            if event.version <= previous {
                return Err(violation(
                    InvariantId::VersionMonotonicity,
                    format!(
                        "entity {:?} version {} followed {}",
                        event.entity, event.version, previous
                    ),
                ));
            }
        }
        Ok(())
    }

    fn check_retry_preservation(&self) -> Result<(), InvariantViolation> {
        for (operation_id, outcome) in &self.server.applied_operations {
            let effects = self
                .server
                .journal
                .iter()
                .filter(|event| event.operation_id == *operation_id)
                .count();
            let expected = usize::from(matches!(outcome, ModelOutcome::Accepted { .. }));
            if effects != expected {
                return Err(violation(
                    InvariantId::RetryPreservation,
                    format!(
                        "operation {operation_id:?} has {effects} effects, expected {expected}"
                    ),
                ));
            }
        }
        Ok(())
    }

    fn check_reconciliation_idempotency(&self) -> Result<(), InvariantViolation> {
        for (&client_id, client) in &self.clients {
            if client.applied_events.len() != usize::try_from(client.cursor).unwrap_or(usize::MAX) {
                return Err(violation(
                    InvariantId::ReconciliationIdempotency,
                    format!("client {client_id:?} applied-event cardinality differs from cursor"),
                ));
            }
        }
        Ok(())
    }

    fn check_timeline_safety(&self) -> Result<(), InvariantViolation> {
        for (&client_id, client) in &self.clients {
            if client.cursor > 0 && client.cursor_epoch != self.server.authority_epoch {
                return Err(violation(
                    InvariantId::TimelineSafety,
                    format!(
                        "client {client_id:?} cursor belongs to epoch {} but authority is {}",
                        client.cursor_epoch, self.server.authority_epoch
                    ),
                ));
            }
        }
        Ok(())
    }

    fn check_lineage(&self) -> Result<(), InvariantViolation> {
        let mut event_ids = BTreeSet::new();
        for event in &self.server.journal {
            if !event_ids.insert(event.event_id) {
                return Err(violation(
                    InvariantId::UniqueEventIdentity,
                    format!("event {:?} appears more than once", event.event_id),
                ));
            }
            let correlation = self.server.operation_correlations.get(&event.operation_id);
            if correlation != Some(&event.correlation_id) {
                return Err(violation(
                    InvariantId::EventCorrelation,
                    format!("event {:?} lost its operation correlation", event.event_id),
                ));
            }
            if event.caused_by != event.operation_id {
                return Err(violation(
                    InvariantId::CausalityAcyclic,
                    format!("event {:?} has an invalid primary cause", event.event_id),
                ));
            }
        }
        for operation_id in self.server.applied_operations.keys() {
            if !self
                .server
                .operation_correlations
                .contains_key(operation_id)
            {
                return Err(violation(
                    InvariantId::RetryLineagePreservation,
                    format!("operation {operation_id:?} has no durable correlation"),
                ));
            }
        }
        Ok(())
    }

    fn client(&self, client: ClientId) -> Result<&ClientModel, ActionError> {
        self.clients
            .get(&client)
            .ok_or(ActionError::UnknownClient(client))
    }

    fn client_mut(&mut self, client: ClientId) -> Result<&mut ClientModel, ActionError> {
        self.clients
            .get_mut(&client)
            .ok_or(ActionError::UnknownClient(client))
    }
}

fn checked_remove<T>(
    items: &mut Vec<T>,
    index: usize,
    error: ActionError,
) -> Result<T, ActionError> {
    if index < items.len() {
        Ok(items.remove(index))
    } else {
        Err(error)
    }
}

fn violation(invariant: InvariantId, detail: String) -> InvariantViolation {
    InvariantViolation { invariant, detail }
}

/// Deterministic state transition available to model searches and replay fixtures.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub enum ModelAction {
    /// Atomically update provisional state and append durable local intent.
    LocalMutate {
        /// Client creating the operation.
        client: ClientId,
        /// Typed operation to persist.
        operation: ModelOperation,
    },
    /// Build an exchange from the first durable outbox operation.
    SendOperation(ClientId),
    /// Build a pull-only exchange.
    Pull(ClientId),
    /// Duplicate one delayed request.
    DuplicateRequest(usize),
    /// Deliver one delayed request, allowing reordering by index.
    DeliverRequest(usize),
    /// Drop one delayed response.
    DropResponse(usize),
    /// Deliver and reconcile one response, allowing reordering by index.
    DeliverResponse(usize),
    /// Stop volatile execution while retaining durable client state.
    CrashClient(ClientId),
    /// Resume volatile client execution.
    RestartClient(ClientId),
    /// Stop volatile authority execution while retaining durable authority state.
    CrashServer,
    /// Resume volatile authority execution.
    RestartServer,
    /// Prevent network sends and deliveries without dropping queues.
    DisconnectNetwork,
    /// Restore network sends and deliveries.
    ReconnectNetwork,
}

/// Stable invariant failure produced after a transition.
#[derive(Clone, Debug, Deserialize, Eq, Error, PartialEq, Serialize)]
#[error("{invariant}: {detail}")]
pub struct InvariantViolation {
    /// Normative registry identifier.
    pub invariant: InvariantId,
    /// Payload-free structural explanation.
    pub detail: String,
}

/// Invalid model action or violated safety invariant.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ModelError {
    /// Action precondition was not met.
    #[error(transparent)]
    Action(#[from] ActionError),
    /// The action produced an unsafe semantic state.
    #[error(transparent)]
    Invariant(#[from] InvariantViolation),
}

/// Error indicating an action whose preconditions are not met.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ActionError {
    /// Client identity is outside the configured model.
    #[error("unknown client {0:?}")]
    UnknownClient(ClientId),
    /// Client cannot currently execute volatile work.
    #[error("client {0:?} is unavailable")]
    ClientUnavailable(ClientId),
    /// No durable operation is ready to send.
    #[error("client {0:?} outbox is empty")]
    EmptyOutbox(ClientId),
    /// The same operation identity was inserted twice locally.
    #[error("operation {0:?} is already pending locally")]
    DuplicateLocalOperation(ModelOperationId),
    /// A retry reused an operation identity with a different root correlation.
    #[error("retry changed lineage for operation {0:?}")]
    RetryLineageChanged(ModelOperationId),
    /// Operation was not created against current reconciled authority state.
    #[error("wrong expected version: expected {expected:?}, supplied {supplied:?}")]
    WrongExpectedVersion {
        /// Current reconciled version.
        expected: Option<u64>,
        /// Version carried by the operation.
        supplied: Option<u64>,
    },
    /// Request index does not exist.
    #[error("missing request at index {0}")]
    MissingRequest(usize),
    /// Response index does not exist.
    #[error("missing response at index {0}")]
    MissingResponse(usize),
    /// The network is disconnected.
    #[error("network is disconnected")]
    NetworkDisconnected,
    /// The server is crashed.
    #[error("server is unavailable")]
    ServerUnavailable,
    /// Cursor or request belongs to an incompatible authority timeline.
    #[error("client epoch {client} is incompatible with authority epoch {authority}")]
    IncompatibleEpoch {
        /// Client epoch.
        client: u64,
        /// Authority epoch.
        authority: u64,
    },
    /// A response attempted to skip a required journal event.
    #[error("non-contiguous response: expected sequence {expected}, got {actual}")]
    NonContiguousResponse {
        /// Next required sequence.
        expected: u64,
        /// Received sequence.
        actual: u64,
    },
}

/// Resource limits for one exhaustive search.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SearchBounds {
    /// Maximum number of transitions in one trace.
    pub max_depth: usize,
    /// Maximum delayed request count, bounding duplication.
    pub max_requests: usize,
    /// Maximum unique states to visit.
    pub max_states: usize,
}

impl Default for SearchBounds {
    fn default() -> Self {
        Self {
            max_depth: 8,
            max_requests: 3,
            max_states: 25_000,
        }
    }
}

/// Successful bounded exhaustive-search summary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SearchReport {
    /// Canonically distinct model states checked.
    pub visited_states: usize,
    /// State transitions attempted.
    pub explored_transitions: usize,
    /// Deepest trace checked.
    pub maximum_depth: usize,
}

/// Exhaustively explores enabled failure interleavings from one initialized model.
///
/// # Errors
///
/// Returns a replayable trace for the first invariant violation or a deterministic serialization
/// error. Invalid actions are ignored because enabled-action generation may become stale only if a
/// caller mutates an internal state between generation and application, which this search does not.
pub fn explore(initial: Model, bounds: SearchBounds) -> Result<SearchReport, ExploreError> {
    let trace_initial = initial.clone();
    let mut visited = BTreeSet::new();
    let mut pending = VecDeque::from([(initial, Vec::new())]);
    let mut report = SearchReport {
        visited_states: 0,
        explored_transitions: 0,
        maximum_depth: 0,
    };
    while let Some((model, actions)) = pending.pop_front() {
        let hash = model
            .deterministic_hash()
            .map_err(ExploreError::Serialize)?;
        if !visited.insert(hash) {
            continue;
        }
        report.visited_states = visited.len();
        report.maximum_depth = report.maximum_depth.max(actions.len());
        if visited.len() >= bounds.max_states || actions.len() >= bounds.max_depth {
            continue;
        }
        for action in model.enabled_actions(bounds) {
            report.explored_transitions = report.explored_transitions.saturating_add(1);
            let mut next = model.clone();
            let mut trace_actions = actions.clone();
            trace_actions.push(action);
            match next.apply(action) {
                Ok(()) => pending.push_back((next, trace_actions)),
                Err(ModelError::Action(_)) => {}
                Err(ModelError::Invariant(violation)) => {
                    return Err(ExploreError::Invariant(Box::new(FailureTrace::capture(
                        &trace_initial,
                        trace_actions,
                        violation,
                    )?)));
                }
            }
        }
    }
    Ok(report)
}

/// Replayable, payload-free RON failure artifact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FailureTrace {
    /// Executable model/schema version.
    pub model_version: u32,
    /// Aequora crate version that produced the trace.
    pub aequora_version: String,
    /// Violated stable invariant.
    pub invariant: InvariantId,
    /// Deterministic initial-state hash.
    pub initial_state_hash: [u8; 32],
    /// Complete payload-free abstract state required for deterministic replay.
    pub initial_state: Model,
    /// Minimized or discovered semantic action sequence.
    pub actions: Vec<ModelAction>,
    /// Structural violation detail.
    pub detail: String,
}

impl FailureTrace {
    fn capture(
        initial: &Model,
        actions: Vec<ModelAction>,
        violation: InvariantViolation,
    ) -> Result<Self, ExploreError> {
        Ok(Self {
            model_version: MODEL_VERSION,
            aequora_version: env!("CARGO_PKG_VERSION").to_owned(),
            invariant: violation.invariant,
            initial_state_hash: initial
                .deterministic_hash()
                .map_err(ExploreError::Serialize)?,
            initial_state: initial.clone(),
            actions,
            detail: violation.detail,
        })
    }

    /// Serializes the trace in stable human-reviewable RON form.
    ///
    /// # Errors
    ///
    /// Returns a RON serialization error when this versioned trace cannot be encoded.
    pub fn to_ron(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
    }

    /// Parses a previously emitted RON trace.
    ///
    /// # Errors
    ///
    /// Returns a RON decoding error for malformed or incompatible artifacts.
    pub fn from_ron(encoded: &str) -> Result<Self, ron::error::SpannedError> {
        ron::from_str(encoded)
    }

    /// Replays the complete trace and returns the reproduced invariant violation.
    ///
    /// # Errors
    ///
    /// Rejects incompatible model versions, modified initial state, invalid actions, a different
    /// invariant failure, a failure before the final action, or a trace that no longer fails.
    pub fn replay(&self) -> Result<InvariantViolation, ReplayError> {
        if self.model_version != MODEL_VERSION {
            return Err(ReplayError::UnsupportedModelVersion(self.model_version));
        }
        let actual_hash = self
            .initial_state
            .deterministic_hash()
            .map_err(ReplayError::Serialize)?;
        if actual_hash != self.initial_state_hash {
            return Err(ReplayError::InitialStateHashMismatch);
        }
        let mut model = self.initial_state.clone();
        for (index, action) in self.actions.iter().copied().enumerate() {
            match model.apply(action) {
                Ok(()) => {}
                Err(ModelError::Action(source)) => {
                    return Err(ReplayError::Action { index, source });
                }
                Err(ModelError::Invariant(actual)) => {
                    if index + 1 != self.actions.len() {
                        return Err(ReplayError::EarlyViolation { index });
                    }
                    if actual.invariant != self.invariant {
                        return Err(ReplayError::InvariantMismatch {
                            expected: self.invariant,
                            actual: actual.invariant,
                        });
                    }
                    return Ok(actual);
                }
            }
        }
        Err(ReplayError::NoViolation)
    }
}

/// Failure to reproduce a versioned model trace exactly.
#[derive(Debug, Error)]
pub enum ReplayError {
    /// Trace was produced by another executable model schema.
    #[error("unsupported model trace version {0}")]
    UnsupportedModelVersion(u32),
    /// Embedded initial state differs from its recorded stable hash.
    #[error("model trace initial-state hash mismatch")]
    InitialStateHashMismatch,
    /// Initial state could not be canonicalized.
    #[error("model trace serialization failed: {0}")]
    Serialize(ron::Error),
    /// Replayed action no longer satisfies its preconditions.
    #[error("model trace action {index} failed: {source}")]
    Action {
        /// Zero-based action index.
        index: usize,
        /// Deterministic action failure.
        source: ActionError,
    },
    /// Trace continued after the first invariant violation.
    #[error("model trace violated an invariant early at action {index}")]
    EarlyViolation {
        /// Zero-based action index.
        index: usize,
    },
    /// Replayed failure refers to another normative invariant.
    #[error("model trace invariant mismatch: expected {expected}, got {actual}")]
    InvariantMismatch {
        /// Invariant recorded by the artifact.
        expected: InvariantId,
        /// Invariant observed during replay.
        actual: InvariantId,
    },
    /// All actions completed without violating an invariant.
    #[error("model trace completed without reproducing an invariant violation")]
    NoViolation,
}

/// Failure produced by exhaustive exploration.
#[derive(Debug, Error)]
pub enum ExploreError {
    /// A reachable state violated a normative invariant.
    #[error("model invariant failed: {0:?}")]
    Invariant(Box<FailureTrace>),
    /// Model state could not be canonicalized.
    #[error("model serialization failed: {0}")]
    Serialize(ron::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn operation(id: u8, value: i64, expected_version: Option<u64>) -> ModelOperation {
        ModelOperation {
            id: ModelOperationId(id),
            correlation_id: ModelCorrelationId(id),
            entity: ModelEntityId(0),
            expected_version,
            value,
        }
    }

    fn apply(model: &mut Model, action: ModelAction) {
        assert_eq!(model.apply(action), Ok(()));
    }

    #[test]
    fn duplicate_request_has_one_authoritative_effect() {
        let mut model = Model::new(1);
        apply(
            &mut model,
            ModelAction::LocalMutate {
                client: ClientId(0),
                operation: operation(1, 10, None),
            },
        );
        apply(&mut model, ModelAction::SendOperation(ClientId(0)));
        apply(&mut model, ModelAction::DuplicateRequest(0));
        apply(&mut model, ModelAction::DeliverRequest(0));
        apply(&mut model, ModelAction::DeliverRequest(0));
        assert_eq!(model.server.journal.len(), 1);
        assert_eq!(model.server.applied_operations.len(), 1);
        let event = model.server.journal[0];
        assert_eq!(event.event_id, ModelEventId(1));
        assert_eq!(event.correlation_id, ModelCorrelationId(1));
        assert_eq!(event.caused_by, ModelOperationId(1));
    }

    #[test]
    fn retry_cannot_replace_original_correlation() {
        let mut model = Model::new(1);
        let original = operation(1, 10, None);
        apply(
            &mut model,
            ModelAction::LocalMutate {
                client: ClientId(0),
                operation: original,
            },
        );
        apply(&mut model, ModelAction::SendOperation(ClientId(0)));
        apply(&mut model, ModelAction::DeliverRequest(0));
        model.network.requests.push(ModelRequest {
            client: ClientId(0),
            operation: Some(ModelOperation {
                correlation_id: ModelCorrelationId(99),
                ..original
            }),
            cursor: 0,
            authority_epoch: 1,
        });

        assert_eq!(
            model.apply(ModelAction::DeliverRequest(0)),
            Err(ModelError::Action(ActionError::RetryLineageChanged(
                ModelOperationId(1)
            )))
        );
        assert_eq!(
            model.server.operation_correlations[&ModelOperationId(1)],
            ModelCorrelationId(1)
        );
        assert_eq!(model.server.journal.len(), 1);
    }

    #[test]
    fn lost_response_retry_is_idempotent_and_reconciles() {
        let mut model = Model::new(1);
        apply(
            &mut model,
            ModelAction::LocalMutate {
                client: ClientId(0),
                operation: operation(1, 10, None),
            },
        );
        apply(&mut model, ModelAction::SendOperation(ClientId(0)));
        apply(&mut model, ModelAction::DeliverRequest(0));
        apply(&mut model, ModelAction::DropResponse(0));
        apply(&mut model, ModelAction::CrashClient(ClientId(0)));
        apply(&mut model, ModelAction::RestartClient(ClientId(0)));
        apply(&mut model, ModelAction::SendOperation(ClientId(0)));
        apply(&mut model, ModelAction::DeliverRequest(0));
        apply(&mut model, ModelAction::DeliverResponse(0));
        assert_eq!(model.server.journal.len(), 1);
        assert_eq!(model.clients[&ClientId(0)].cursor, 1);
        assert!(model.clients[&ClientId(0)].outbox.is_empty());
    }

    #[test]
    fn two_clients_produce_one_commit_and_one_conflict() {
        let mut model = Model::new(2);
        apply(
            &mut model,
            ModelAction::LocalMutate {
                client: ClientId(0),
                operation: operation(1, 10, None),
            },
        );
        apply(
            &mut model,
            ModelAction::LocalMutate {
                client: ClientId(1),
                operation: operation(2, 20, None),
            },
        );
        apply(&mut model, ModelAction::SendOperation(ClientId(0)));
        apply(&mut model, ModelAction::SendOperation(ClientId(1)));
        apply(&mut model, ModelAction::DeliverRequest(0));
        apply(&mut model, ModelAction::DeliverRequest(0));
        assert_eq!(model.server.journal.len(), 1);
        assert!(matches!(
            model.server.applied_operations[&ModelOperationId(1)],
            ModelOutcome::Accepted { .. }
        ));
        assert!(matches!(
            model.server.applied_operations[&ModelOperationId(2)],
            ModelOutcome::Conflict { .. }
        ));
    }

    #[test]
    fn bounded_search_explores_crash_duplicate_and_loss_interleavings() {
        let mut model = Model::new(1);
        apply(
            &mut model,
            ModelAction::LocalMutate {
                client: ClientId(0),
                operation: operation(1, 10, None),
            },
        );
        let report = explore(
            model,
            SearchBounds {
                max_depth: 7,
                max_requests: 3,
                max_states: 10_000,
            },
        );
        let report = report.unwrap_or_else(|error| panic!("bounded model search failed: {error}"));
        assert!(report.visited_states > 100);
        assert!(report.explored_transitions > report.visited_states);
    }

    #[test]
    fn failure_trace_round_trips_as_ron() {
        let model = Model::new(1);
        let trace = FailureTrace::capture(
            &model,
            vec![ModelAction::DisconnectNetwork],
            InvariantViolation {
                invariant: InvariantId::CursorSafety,
                detail: "synthetic regression fixture".to_owned(),
            },
        )
        .unwrap_or_else(|error| panic!("trace capture failed: {error}"));
        let encoded = trace
            .to_ron()
            .unwrap_or_else(|error| panic!("trace encoding failed: {error}"));
        assert_eq!(FailureTrace::from_ron(&encoded), Ok(trace));
    }

    #[test]
    fn ron_failure_trace_replays_from_embedded_initial_state() {
        let mut initial = Model::new(1);
        initial
            .clients
            .get_mut(&ClientId(0))
            .unwrap_or_else(|| panic!("model client fixture is missing"))
            .optimistic_state
            .insert(
                ModelEntityId(0),
                CanonicalEntity {
                    version: 1,
                    value: 42,
                    tombstone: false,
                },
            );
        let action = ModelAction::DisconnectNetwork;
        let mut failing = initial.clone();
        let violation = match failing.apply(action) {
            Err(ModelError::Invariant(violation)) => violation,
            result => panic!("fixture should violate local intent atomicity: {result:?}"),
        };
        let trace = FailureTrace::capture(&initial, vec![action], violation.clone())
            .unwrap_or_else(|error| panic!("trace capture failed: {error}"));
        let encoded = trace
            .to_ron()
            .unwrap_or_else(|error| panic!("trace encoding failed: {error}"));
        let decoded = FailureTrace::from_ron(&encoded)
            .unwrap_or_else(|error| panic!("trace decoding failed: {error}"));
        let actual = decoded
            .replay()
            .unwrap_or_else(|error| panic!("trace replay failed: {error}"));
        assert_eq!(actual.invariant, violation.invariant);
    }

    proptest! {
        #[test]
        fn randomized_duplicate_and_lost_response_sequences_preserve_invariants(
            duplicate in any::<bool>(),
            lose_first_response in any::<bool>(),
            crash_before_retry in any::<bool>(),
        ) {
            let mut model = Model::new(1);
            prop_assert_eq!(model.apply(ModelAction::LocalMutate {
                client: ClientId(0),
                operation: operation(1, 42, None),
            }), Ok(()));
            prop_assert_eq!(model.apply(ModelAction::SendOperation(ClientId(0))), Ok(()));
            if duplicate {
                prop_assert_eq!(model.apply(ModelAction::DuplicateRequest(0)), Ok(()));
            }
            while !model.network.requests.is_empty() {
                prop_assert_eq!(model.apply(ModelAction::DeliverRequest(0)), Ok(()));
            }
            if lose_first_response {
                prop_assert_eq!(model.apply(ModelAction::DropResponse(0)), Ok(()));
                if crash_before_retry {
                    prop_assert_eq!(model.apply(ModelAction::CrashClient(ClientId(0))), Ok(()));
                    prop_assert_eq!(model.apply(ModelAction::RestartClient(ClientId(0))), Ok(()));
                }
                prop_assert_eq!(model.apply(ModelAction::SendOperation(ClientId(0))), Ok(()));
                prop_assert_eq!(model.apply(ModelAction::DeliverRequest(0)), Ok(()));
            }
            while !model.network.responses.is_empty() {
                prop_assert_eq!(model.apply(ModelAction::DeliverResponse(0)), Ok(()));
            }
            prop_assert_eq!(model.check_invariants(), Ok(()));
            prop_assert_eq!(model.server.journal.len(), 1);
            prop_assert!(model.clients[&ClientId(0)].outbox.is_empty());
        }
    }
}
