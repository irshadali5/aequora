//! Connection/open policy for a physical local replica.

/// Open mode chosen after schema and recovery inspection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenMode {
    ReadWrite,
    RecoveryRequired,
    RefuseNewerSchema,
}
