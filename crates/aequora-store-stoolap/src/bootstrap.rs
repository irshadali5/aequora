//! Durable bootstrap lifecycle contracts.

/// Persisted phase of a scoped snapshot bootstrap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapPhase {
    NotStarted,
    ManifestReceived,
    Downloading,
    Verifying,
    Installing,
    Activating,
    CatchUp,
    Complete,
    Failed,
}

impl BootstrapPhase {
    /// Stable database representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::ManifestReceived => "manifest_received",
            Self::Downloading => "downloading",
            Self::Verifying => "verifying",
            Self::Installing => "installing",
            Self::Activating => "activating",
            Self::CatchUp => "catch_up",
            Self::Complete => "complete",
            Self::Failed => "failed",
        }
    }
}
