//! Durable `SQLite` placement and resource policy.

use std::{path::PathBuf, time::Duration};

/// Physical platform class used to validate durable placement policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SQLitePlatform {
    /// Linux, Windows, or macOS application-data storage.
    Desktop,
    /// Android application-private data storage.
    Android,
    /// iOS Application Support storage.
    Ios,
}

/// Configuration for one durable embedded replica.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SQLiteConfig {
    /// Database file in durable application-private storage.
    pub database_path: PathBuf,
    /// Target placement policy.
    pub platform: SQLitePlatform,
    /// Busy wait used to serialize the one logical writer.
    pub busy_timeout: Duration,
    /// Whether production WAL enforcement is required.
    pub production: bool,
}

impl SQLiteConfig {
    /// Creates the recommended production configuration.
    #[must_use]
    pub fn production(database_path: impl Into<PathBuf>, platform: SQLitePlatform) -> Self {
        Self {
            database_path: database_path.into(),
            platform,
            busy_timeout: Duration::from_secs(5),
            production: true,
        }
    }

    /// Validates path and concurrency bounds.
    ///
    /// # Errors
    ///
    /// Returns a static explanation for an empty path or zero busy timeout.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.database_path.as_os_str().is_empty() {
            return Err("SQLite database path must not be empty");
        }
        if self.busy_timeout.is_zero() {
            return Err("SQLite busy timeout must be nonzero");
        }
        Ok(())
    }
}
