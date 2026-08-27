//! Reusable consistency-profile adapter and application compliance checks.

use aequora_profile::{AdapterProfileCapabilities, ProfileError, ProfileManifest, ProfileRegistry};

/// Evidence returned after a registry and adapter capability declaration pass together.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileComplianceReport {
    pub aggregate_count: usize,
    pub operation_count: usize,
    pub manifest_digest: [u8; 32],
}

/// Verifies the coherent registry, portable manifest, and adapter requirements as one gate.
///
/// # Errors
///
/// Returns the first profile, compatibility, manifest, or missing-capability failure.
pub fn verify_profile_registry(
    registry: &ProfileRegistry,
    capabilities: &AdapterProfileCapabilities,
) -> Result<ProfileComplianceReport, ProfileError> {
    registry.validate_capabilities(capabilities)?;
    let manifest = registry.manifest()?;
    verify_profile_manifest(&manifest)
}

/// Verifies a previously generated manifest and returns compact evidence.
///
/// # Errors
///
/// Returns a structural, semantic, or digest failure from the manifest.
pub fn verify_profile_manifest(
    manifest: &ProfileManifest,
) -> Result<ProfileComplianceReport, ProfileError> {
    manifest.verify()?;
    Ok(ProfileComplianceReport {
        aggregate_count: manifest.aggregates.len(),
        operation_count: manifest.operations.len(),
        manifest_digest: manifest.digest,
    })
}
