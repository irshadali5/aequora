use serde::{Deserialize, Serialize};

use crate::LoadState;

/// Optional features that may be disabled without weakening correctness or security.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct BrownoutPolicy {
    pub disable_presence: bool,
    pub pause_maintenance: bool,
    pub pause_bulk_builds: bool,
    pub pause_background_integrity: bool,
    pub disable_rich_diagnostics: bool,
}

impl BrownoutPolicy {
    /// Derives safe degradation. Authorization, revocation, required audit, and commit integrity
    /// have no switches here and therefore cannot be browned out.
    #[must_use]
    pub const fn for_load(state: LoadState) -> Self {
        match state {
            LoadState::Healthy => Self {
                disable_presence: false,
                pause_maintenance: false,
                pause_bulk_builds: false,
                pause_background_integrity: false,
                disable_rich_diagnostics: false,
            },
            LoadState::Elevated => Self {
                disable_presence: false,
                pause_maintenance: true,
                pause_bulk_builds: false,
                pause_background_integrity: false,
                disable_rich_diagnostics: false,
            },
            LoadState::Overloaded => Self {
                disable_presence: true,
                pause_maintenance: true,
                pause_bulk_builds: true,
                pause_background_integrity: true,
                disable_rich_diagnostics: false,
            },
            LoadState::Critical => Self {
                disable_presence: true,
                pause_maintenance: true,
                pause_bulk_builds: true,
                pause_background_integrity: true,
                disable_rich_diagnostics: true,
            },
        }
    }
}
