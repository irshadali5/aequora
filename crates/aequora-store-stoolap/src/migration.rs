//! Local schema migration compatibility decisions.

/// Result of comparing an on-disk schema with this build.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaCompatibility {
    Current,
    UpgradeRequired,
    RefuseDestructiveDowngrade,
}
