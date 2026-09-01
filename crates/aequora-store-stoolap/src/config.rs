//! Validated configuration for the embedded local replica.

use std::path::PathBuf;

/// Physical environment whose resource policy is being configured.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalPlatformClass {
    /// Linux, Windows, or macOS application-data storage.
    Desktop,
    /// Android or iOS durable application-private storage.
    Mobile,
}

/// Durability class applied to local transactions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoolapDurability {
    /// Required for Tx A, Tx C, identity, cursor, and fencing state.
    Critical,
    /// Permitted only for state that can be reconstructed from durable truth.
    Reconstructable,
}

/// Resource and placement policy for one Stoolap replica.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoolapConfig {
    /// Stoolap database path in durable application-private storage.
    pub database_path: PathBuf,
    /// Platform resource class.
    pub platform: LocalPlatformClass,
    /// Maximum operations claimed by one upload batch.
    pub max_claim_batch: usize,
    /// Bytes retained as a safety margin during bootstrap or import.
    pub storage_safety_margin_bytes: u64,
    /// Required durability for correctness-critical transactions.
    pub critical_durability: StoolapDurability,
}

impl StoolapConfig {
    /// Validates resource bounds and rejects weak durability for critical state.
    ///
    /// # Errors
    ///
    /// Returns a static explanation when a required bound is zero, the path is empty, or critical
    /// transactions are configured as reconstructable.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.database_path.as_os_str().is_empty() {
            return Err("Stoolap database path must not be empty");
        }
        if self.max_claim_batch == 0 {
            return Err("Stoolap claim batch must be bounded above zero");
        }
        if self.storage_safety_margin_bytes == 0 {
            return Err("Stoolap storage safety margin must be nonzero");
        }
        if self.critical_durability != StoolapDurability::Critical {
            return Err("critical Stoolap transactions require critical durability");
        }
        Ok(())
    }
}
