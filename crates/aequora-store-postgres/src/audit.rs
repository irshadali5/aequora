//! `PostgreSQL` audit policy vocabulary.

/// Whether a domain operation requires audit evidence in Tx B.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PostgresAuditRequirement {
    /// Audit evidence is mandatory and transaction failure must roll back the operation.
    Required,
    /// An explicit domain profile declares no audit row for this operation.
    NotRequired,
}
