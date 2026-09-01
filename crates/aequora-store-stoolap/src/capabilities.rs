//! Named, environment-bound Part 38 conformance profiles.

/// Minimal Tx A, Tx C, identity, migration, and crash-recovery semantics.
pub const STOOLAP_LOCAL_CORE_PROFILE: &str = "StoolapLocalCore";
/// Full desktop local-store semantics after target-specific certification.
pub const STOOLAP_DESKTOP_LOCAL_FULL_PROFILE: &str = "StoolapDesktopLocalFull";
/// Full mobile local-store semantics after target-specific certification.
pub const STOOLAP_MOBILE_LOCAL_FULL_PROFILE: &str = "StoolapMobileLocalFull";

/// Stable implementation identity used by diagnostics and certification evidence.
pub const STOOLAP_LOCAL_ADAPTER_IDENTITY: &str = "aequora-stoolap";
