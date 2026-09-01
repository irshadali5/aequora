//! Authoritative transaction policy.

/// Ordered failure points used by real-database fault tests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostgresTxFailpoint {
    /// Application business mutation completed.
    AfterBusinessMutation,
    /// Aggregate/entity version persisted.
    AfterVersionUpdate,
    /// Journal row appended.
    AfterJournalAppend,
    /// Operation ledger outcome persisted.
    AfterLedgerInsert,
    /// Required audit row persisted.
    AfterAuditInsert,
    /// All writes complete but commit not submitted.
    BeforeCommit,
}
