//! Authoritative transaction capability.

use crate::{AdapterError, AuditRecord, AuthorityMutation, JournalRecord, LedgerRecord};
use async_trait::async_trait;

/// One authoritative transaction preserving the complete publication boundary.
#[async_trait]
pub trait AuthorityTransaction: Send {
    /// Applies deterministic application business state.
    async fn apply_business_mutation(
        &mut self,
        mutation: AuthorityMutation,
    ) -> Result<(), AdapterError>;

    /// Appends the event in committed logical cursor order.
    async fn append_journal(&mut self, record: JournalRecord) -> Result<(), AdapterError>;

    /// Records the idempotent operation outcome and canonical payload digest.
    async fn record_ledger(&mut self, record: LedgerRecord) -> Result<(), AdapterError>;

    /// Writes required immutable audit evidence.
    async fn record_audit(&mut self, record: AuditRecord) -> Result<(), AdapterError>;

    /// Atomically publishes business state, version, journal, ledger, and audit.
    async fn commit(self) -> Result<(), AdapterError>;

    /// Discards all staged authoritative writes.
    async fn rollback(self) -> Result<(), AdapterError>;
}

/// Factory for authoritative transactions without exposing database driver types to core.
#[async_trait]
pub trait AuthorityTransactionStore: Send + Sync {
    /// Concrete transaction retained in the application/adapter composition layer.
    type Tx<'a>: AuthorityTransaction + Send
    where
        Self: 'a;

    /// Begins one authoritative transaction.
    async fn begin_authority_tx(&self) -> Result<Self::Tx<'_>, AdapterError>;
}

/// Compile-time composition marker for the complete authoritative boundary.
pub trait SupportsAtomicAuthorityCommit: AuthorityTransactionStore {}

/// Binds an application repository to the adapter's concrete transaction.
///
/// This lets domain writes and Aequora metadata share a physical transaction while the sync core
/// remains generic and database-neutral.
pub trait DomainRepositoryFactory<Tx: ?Sized> {
    /// Application repository borrowing the transaction.
    type Repository<'a>
    where
        Self: 'a,
        Tx: 'a;

    /// Creates a domain repository over the supplied physical transaction.
    fn repository<'a>(&'a self, transaction: &'a mut Tx) -> Self::Repository<'a>;
}
