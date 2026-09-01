//! Named, environment-bound Part 42 conformance profiles.

/// Portable core transaction, recovery, and replay profile.
pub const SQLITE_LOCAL_CORE_PROFILE: &str = "SQLiteLocalCore";
/// Full Linux, Windows, or macOS profile after target certification.
pub const SQLITE_DESKTOP_LOCAL_FULL_PROFILE: &str = "SQLiteDesktopLocalFull";
/// Full Android or iOS profile after target certification.
pub const SQLITE_MOBILE_LOCAL_FULL_PROFILE: &str = "SQLiteMobileLocalFull";
/// Stable diagnostics and certification identity.
pub const SQLITE_LOCAL_ADAPTER_IDENTITY: &str = "aequora-sqlite";
