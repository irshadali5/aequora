//! Database-, transport-, and platform-neutral release engineering contracts.
//!
//! This crate owns the signed data model and compatibility decisions. Platform packagers,
//! artifact repositories, update transports, CI systems, and protected signing services remain
//! adapters around these contracts.

pub mod productization;

pub use aequora_crypto::CryptoTimestamp;
use aequora_crypto::{
    KeyProvider, KeyPurpose, KeyStatus, PublicKeyBytes, SignatureEnvelope, SigningError,
    SigningKeyId, VerificationError, canonical_bytes, domain_digest, sign_digest, verify_digest,
};
pub use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

const MANIFEST_FORMAT_VERSION: u16 = 1;
const MAX_TEXT_BYTES: usize = 512;
const MAX_ARTIFACTS: usize = 512;
const MAX_STORE_FORMATS: usize = 64;
const MAX_SUPPORT_MATRIX_RON_BYTES: usize = 1024 * 1024;

/// Both interoperable and Aequora-native hashes for immutable bytes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct ArtifactHashes {
    pub sha256: [u8; 32],
    pub blake3: [u8; 32],
}

impl ArtifactHashes {
    #[must_use]
    pub fn digest(bytes: &[u8]) -> Self {
        Self {
            sha256: Sha256::digest(bytes).into(),
            blake3: *blake3::hash(bytes).as_bytes(),
        }
    }

    #[must_use]
    pub fn matches(self, bytes: &[u8]) -> bool {
        self == Self::digest(bytes)
    }
}

/// Stable release channel. Enterprise-pinned installations are externally managed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ReleaseChannel {
    Nightly,
    Alpha,
    Beta,
    ReleaseCandidate,
    Stable,
    EnterprisePinned,
}

/// Whether reverting binaries is compatible with state written by this release.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RollbackClass {
    BinaryRollbackSafe,
    RollbackRequiresMigration,
    ForwardOnly,
}

/// Purpose-specific artifact category used by packaging and retention policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ArtifactKind {
    RustCrate,
    Cli,
    Server,
    DesktopAgent,
    DesktopApplication,
    AndroidArchive,
    AndroidApplication,
    IosFramework,
    IosApplication,
    ContainerImage,
    MigrationBundle,
    RegistrySnapshot,
    ConfigurationSchema,
    ConformanceReport,
    DebugSymbols,
    Sbom,
    Provenance,
    Documentation,
}

/// Supported delivery environment without binding core release logic to an OS API.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum DeliveryPlatform {
    Linux,
    Windows,
    MacOs,
    Android,
    Ios,
    Oci,
    Rust,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum PackageFormat {
    TarZstd,
    Zip,
    Deb,
    Rpm,
    AppImage,
    Msi,
    Msix,
    AppBundle,
    Dmg,
    Pkg,
    Aar,
    Apk,
    AndroidAppBundle,
    XcFramework,
    Ipa,
    OciImage,
    CargoCrate,
}

/// One explicitly supported build/package/smoke-test target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TargetSupport {
    pub platform: DeliveryPlatform,
    pub target: String,
    pub formats: BTreeSet<PackageFormat>,
    pub native_signing_required: bool,
    pub native_runner_required: bool,
    pub smoke_test: String,
}

/// Versioned machine-readable cross-platform support matrix.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SupportMatrix {
    pub schema_version: u16,
    pub targets: Vec<TargetSupport>,
}

impl SupportMatrix {
    /// Parses and validates the checked-in RON support matrix.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed RON or an invalid matrix.
    pub fn from_ron(input: &str) -> Result<Self, ReleaseError> {
        if input.len() > MAX_SUPPORT_MATRIX_RON_BYTES {
            return Err(ReleaseError::SupportMatrixEncoding);
        }
        let matrix: Self = ron::from_str(input).map_err(|_| ReleaseError::SupportMatrixEncoding)?;
        matrix.validate()?;
        Ok(matrix)
    }

    /// Validates unique targets and concrete package/smoke-test declarations.
    ///
    /// # Errors
    ///
    /// Returns an error for unsupported schema versions, duplicates, or incomplete targets.
    pub fn validate(&self) -> Result<(), ReleaseError> {
        if self.schema_version != 1 || self.targets.is_empty() || self.targets.len() > 128 {
            return Err(ReleaseError::InvalidSupportMatrix);
        }
        let mut targets = BTreeSet::new();
        for target in &self.targets {
            validate_token(&target.target)?;
            validate_token(&target.smoke_test)?;
            if target.formats.is_empty() || !targets.insert(target.target.as_str()) {
                return Err(ReleaseError::InvalidSupportMatrix);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum ReleaseGate {
    Correctness,
    Compatibility,
    Security,
    Migration,
    Conformance,
    Performance,
    Documentation,
    Packaging,
    Provenance,
}

impl ReleaseGate {
    pub const ALL: [Self; 9] = [
        Self::Correctness,
        Self::Compatibility,
        Self::Security,
        Self::Migration,
        Self::Conformance,
        Self::Performance,
        Self::Documentation,
        Self::Packaging,
        Self::Provenance,
    ];
}

/// Evidence used to promote an already-built release candidate without rebuilding it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromotionEvidence {
    pub release_id: String,
    pub candidate_manifest_digest: [u8; 32],
    pub promoted_manifest_digest: [u8; 32],
    pub passed_gates: BTreeSet<ReleaseGate>,
    pub source_commit: String,
    pub approvers: BTreeSet<String>,
    pub production_credentials_present_in_build_job: bool,
}

impl PromotionEvidence {
    /// Authorizes stable promotion only for the same bytes after every release gate passes.
    ///
    /// # Errors
    ///
    /// Rejects rebuilds, incomplete gates, missing approval, malformed source identity, and build
    /// jobs that possessed production credentials.
    pub fn authorize_stable(&self, minimum_approvers: usize) -> Result<(), ReleaseError> {
        validate_token(&self.release_id)?;
        validate_hex(&self.source_commit, 40)?;
        if self.candidate_manifest_digest == [0; 32]
            || self.candidate_manifest_digest != self.promoted_manifest_digest
            || !ReleaseGate::ALL
                .iter()
                .all(|gate| self.passed_gates.contains(gate))
            || self.approvers.len() < minimum_approvers
            || self
                .approvers
                .iter()
                .any(|approver| validate_token(approver).is_err())
            || self.production_credentials_present_in_build_job
        {
            return Err(ReleaseError::PromotionRejected);
        }
        Ok(())
    }
}

/// A final packaged artifact and all data needed to verify it independently of transport.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactDescriptor {
    pub name: String,
    pub kind: ArtifactKind,
    pub target: String,
    pub size: u64,
    pub hashes: ArtifactHashes,
    pub signature: SignatureEnvelope,
    pub sbom: Option<String>,
    pub provenance: Option<String>,
}

impl ArtifactDescriptor {
    /// Verifies identity, final byte hashes, size, key purpose, and signature.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed metadata, changed bytes, or untrusted signing material.
    pub fn verify(
        &self,
        bytes: &[u8],
        trust: &ReleaseTrustStore,
        now: CryptoTimestamp,
    ) -> Result<(), ReleaseError> {
        self.validate()?;
        if usize::try_from(self.size).ok() != Some(bytes.len()) || !self.hashes.matches(bytes) {
            return Err(ReleaseError::ArtifactDigestMismatch(self.name.clone()));
        }
        trust.verify(
            KeyPurpose::ReleaseArtifactSigning,
            artifact_signing_digest(bytes),
            &self.signature,
            now,
        )
    }

    fn validate(&self) -> Result<(), ReleaseError> {
        validate_token(&self.name)?;
        validate_token(&self.target)?;
        if self.size == 0 || self.hashes.sha256 == [0; 32] || self.hashes.blake3 == [0; 32] {
            return Err(ReleaseError::InvalidManifest("empty artifact"));
        }
        validate_optional_ref(self.sbom.as_deref())?;
        validate_optional_ref(self.provenance.as_deref())?;
        if self.signature.purpose != KeyPurpose::ReleaseArtifactSigning {
            return Err(ReleaseError::SigningPurposeMismatch);
        }
        Ok(())
    }
}

/// Build inputs that make an artifact traceable and reproducibility-checkable.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildProvenance {
    pub source_repository: String,
    pub git_commit: String,
    pub build_id: String,
    pub rust_toolchain: String,
    pub target: String,
    pub cargo_features: BTreeSet<String>,
    pub source_digest: [u8; 32],
    pub workflow_identity: String,
}

/// Compatibility dimensions which deliberately evolve independently from product `SemVer`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompatibilityMatrix {
    pub protocol_min: u32,
    pub protocol_max: u32,
    pub operation_schema: u32,
    pub config_schema: u32,
    pub snapshot_schema: u32,
    pub registry_generation: u64,
    pub registry_digest: [u8; 32],
    pub store_formats: BTreeMap<String, u32>,
    pub minimum_client: Version,
    pub minimum_server: Version,
    pub migration_set_digest: [u8; 32],
}

impl CompatibilityMatrix {
    fn validate(&self) -> Result<(), ReleaseError> {
        if self.protocol_min > self.protocol_max
            || self.protocol_min == 0
            || self.operation_schema == 0
            || self.config_schema == 0
            || self.snapshot_schema == 0
            || self.registry_generation == 0
            || self.registry_digest == [0; 32]
            || self.migration_set_digest == [0; 32]
            || self.store_formats.is_empty()
            || self.store_formats.len() > MAX_STORE_FORMATS
            || self
                .store_formats
                .iter()
                .any(|(name, version)| validate_token(name).is_err() || *version == 0)
        {
            return Err(ReleaseError::InvalidCompatibility);
        }
        Ok(())
    }
}

/// A migration bundle bound to its exact ordered migration set.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationBundleDescriptor {
    pub adapter: String,
    pub artifact: String,
    pub from_format: u32,
    pub to_format: u32,
    pub migration_ids: Vec<String>,
    pub digest: [u8; 32],
    pub reversible: bool,
}

impl MigrationBundleDescriptor {
    fn validate(&self) -> Result<(), ReleaseError> {
        validate_token(&self.adapter)?;
        validate_ref(&self.artifact)?;
        if self.from_format == 0
            || self.from_format > self.to_format
            || self.migration_ids.is_empty()
            || self.migration_ids.len() > 1_024
            || self.digest == [0; 32]
            || self
                .migration_ids
                .iter()
                .any(|id| validate_token(id).is_err())
        {
            return Err(ReleaseError::InvalidMigrationBundle);
        }
        Ok(())
    }
}

/// Unsigned, machine-readable description of one immutable coordinated release.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    pub format_version: u16,
    pub release_id: String,
    pub release_version: Version,
    pub channel: ReleaseChannel,
    pub provenance: BuildProvenance,
    pub compatibility: CompatibilityMatrix,
    pub rollback_class: RollbackClass,
    pub artifact_set: Vec<ArtifactDescriptor>,
    pub migrations: Vec<MigrationBundleDescriptor>,
    pub release_notes: String,
}

impl ReleaseManifest {
    /// Validates completeness, uniqueness, references, and compatibility dimensions.
    ///
    /// # Errors
    ///
    /// Returns an error when the manifest is malformed or internally inconsistent.
    pub fn validate(&self) -> Result<(), ReleaseError> {
        if self.format_version != MANIFEST_FORMAT_VERSION {
            return Err(ReleaseError::UnsupportedManifestVersion);
        }
        validate_token(&self.release_id)?;
        validate_channel_version(self.channel, &self.release_version)?;
        validate_token(&self.provenance.source_repository)?;
        validate_hex(&self.provenance.git_commit, 40)?;
        validate_token(&self.provenance.build_id)?;
        validate_token(&self.provenance.rust_toolchain)?;
        validate_token(&self.provenance.target)?;
        validate_token(&self.provenance.workflow_identity)?;
        validate_ref(&self.release_notes)?;
        if self.provenance.source_digest == [0; 32]
            || self.artifact_set.is_empty()
            || self.artifact_set.len() > MAX_ARTIFACTS
        {
            return Err(ReleaseError::InvalidManifest("missing release evidence"));
        }
        self.compatibility.validate()?;

        let mut names = BTreeSet::new();
        for artifact in &self.artifact_set {
            artifact.validate()?;
            if requires_versioned_name(artifact.kind)
                && (!artifact.name.contains(&self.release_version.to_string())
                    || !artifact.name.contains(&artifact.target))
            {
                return Err(ReleaseError::InvalidArtifactName(artifact.name.clone()));
            }
            if !names.insert(artifact.name.as_str()) {
                return Err(ReleaseError::DuplicateArtifact(artifact.name.clone()));
            }
        }
        for artifact in &self.artifact_set {
            for reference in [artifact.sbom.as_deref(), artifact.provenance.as_deref()]
                .into_iter()
                .flatten()
            {
                if !names.contains(reference) {
                    return Err(ReleaseError::MissingArtifactReference(reference.to_owned()));
                }
            }
        }
        for required in [
            ArtifactKind::Sbom,
            ArtifactKind::Provenance,
            ArtifactKind::MigrationBundle,
            ArtifactKind::RegistrySnapshot,
            ArtifactKind::ConfigurationSchema,
            ArtifactKind::ConformanceReport,
            ArtifactKind::Documentation,
        ] {
            if !self
                .artifact_set
                .iter()
                .any(|artifact| artifact.kind == required)
            {
                return Err(ReleaseError::MissingRequiredArtifact(required));
            }
        }
        for artifact in &self.artifact_set {
            if requires_supply_chain_references(artifact.kind)
                && (artifact.sbom.is_none() || artifact.provenance.is_none())
            {
                return Err(ReleaseError::MissingSupplyChainReference(
                    artifact.name.clone(),
                ));
            }
        }
        for migration in &self.migrations {
            migration.validate()?;
            if !self.artifact_set.iter().any(|artifact| {
                artifact.name == migration.artifact
                    && artifact.kind == ArtifactKind::MigrationBundle
            }) {
                return Err(ReleaseError::MissingArtifactReference(
                    migration.artifact.clone(),
                ));
            }
        }
        if self.migrations.is_empty()
            || !self
                .migrations
                .iter()
                .any(|migration| migration.digest == self.compatibility.migration_set_digest)
        {
            return Err(ReleaseError::MigrationDigestMismatch);
        }
        Ok(())
    }

    /// Computes the domain-separated digest signed by the release-manifest key.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical serialization fails.
    pub fn signing_digest(&self) -> Result<[u8; 32], ReleaseError> {
        let bytes = canonical_bytes(self).map_err(|_| ReleaseError::CanonicalEncoding)?;
        Ok(domain_digest("AEQUORA:RELEASE-MANIFEST:v1", &bytes))
    }
}

/// Official release metadata with a purpose-bound signature.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignedReleaseManifest {
    pub manifest: ReleaseManifest,
    pub signature: SignatureEnvelope,
}

impl SignedReleaseManifest {
    /// Verifies manifest structure and its purpose-bound signature without reading artifacts.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed metadata, untrusted keys, or an invalid signature.
    pub fn verify_manifest(
        &self,
        trust: &ReleaseTrustStore,
        now: CryptoTimestamp,
    ) -> Result<(), ReleaseError> {
        self.manifest.validate()?;
        trust.verify(
            KeyPurpose::ReleaseManifestSigning,
            self.manifest.signing_digest()?,
            &self.signature,
            now,
        )
    }

    /// Verifies the manifest and all locally supplied final artifacts.
    ///
    /// The callback permits streaming/file-backed verification without retaining a whole bundle.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid metadata, trust, compatibility, or artifact bytes.
    pub fn verify<F>(
        &self,
        trust: &ReleaseTrustStore,
        now: CryptoTimestamp,
        mut artifact: F,
    ) -> Result<(), ReleaseError>
    where
        F: FnMut(&str) -> Result<Vec<u8>, ReleaseError>,
    {
        self.verify_manifest(trust, now)?;
        for descriptor in &self.manifest.artifact_set {
            descriptor.verify(&artifact(&descriptor.name)?, trust, now)?;
        }
        Ok(())
    }
}

/// Signs a completely assembled manifest. Artifact descriptors must already contain signatures.
///
/// # Errors
///
/// Returns an error if validation, canonical encoding, or the signing provider fails.
pub async fn sign_release_manifest<P: KeyProvider + ?Sized>(
    manifest: ReleaseManifest,
    provider: &P,
    key_id: SigningKeyId,
    signed_at: CryptoTimestamp,
) -> Result<SignedReleaseManifest, ReleaseError> {
    manifest.validate()?;
    let signature = sign_digest(
        provider,
        KeyPurpose::ReleaseManifestSigning,
        key_id,
        signed_at,
        manifest.signing_digest()?,
    )
    .await?;
    Ok(SignedReleaseManifest {
        manifest,
        signature,
    })
}

/// Signs the hash of final packaged bytes. Calling this before packaging produces a signature that
/// will fail verification once bytes change.
///
/// # Errors
///
/// Returns an error when the protected signing provider rejects the operation.
pub async fn sign_artifact<P: KeyProvider + ?Sized>(
    bytes: &[u8],
    provider: &P,
    key_id: SigningKeyId,
    signed_at: CryptoTimestamp,
) -> Result<SignatureEnvelope, ReleaseError> {
    Ok(sign_digest(
        provider,
        KeyPurpose::ReleaseArtifactSigning,
        key_id,
        signed_at,
        artifact_signing_digest(bytes),
    )
    .await?)
}

fn artifact_signing_digest(bytes: &[u8]) -> [u8; 32] {
    domain_digest("AEQUORA:RELEASE-ARTIFACT:v1", bytes)
}

const fn requires_supply_chain_references(kind: ArtifactKind) -> bool {
    matches!(
        kind,
        ArtifactKind::RustCrate
            | ArtifactKind::Cli
            | ArtifactKind::Server
            | ArtifactKind::DesktopAgent
            | ArtifactKind::DesktopApplication
            | ArtifactKind::AndroidArchive
            | ArtifactKind::AndroidApplication
            | ArtifactKind::IosFramework
            | ArtifactKind::IosApplication
            | ArtifactKind::ContainerImage
            | ArtifactKind::MigrationBundle
    )
}

const fn requires_versioned_name(kind: ArtifactKind) -> bool {
    matches!(
        kind,
        ArtifactKind::Cli
            | ArtifactKind::Server
            | ArtifactKind::DesktopAgent
            | ArtifactKind::DesktopApplication
            | ArtifactKind::AndroidArchive
            | ArtifactKind::AndroidApplication
            | ArtifactKind::IosFramework
            | ArtifactKind::IosApplication
    )
}

/// Public verification material. Secret signing credentials cannot be represented here.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseTrustStore {
    keys: BTreeMap<SigningKeyId, TrustedReleaseKey>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedReleaseKey {
    pub public_key: PublicKeyBytes,
    pub purpose: KeyPurpose,
    pub status: KeyStatus,
    pub not_before: CryptoTimestamp,
    pub not_after: Option<CryptoTimestamp>,
}

impl ReleaseTrustStore {
    pub fn insert(&mut self, key_id: SigningKeyId, key: TrustedReleaseKey) {
        self.keys.insert(key_id, key);
    }

    /// Verifies lifecycle, purpose, validity window, signature timestamp, and signature bytes.
    ///
    /// # Errors
    ///
    /// Returns an error when the key is unknown, revoked, expired, purpose-mismatched, or invalid.
    pub fn verify(
        &self,
        expected_purpose: KeyPurpose,
        digest: [u8; 32],
        signature: &SignatureEnvelope,
        now: CryptoTimestamp,
    ) -> Result<(), ReleaseError> {
        let key = self
            .keys
            .get(&signature.key_id)
            .ok_or(ReleaseError::UnknownSigningKey)?;
        if key.purpose != expected_purpose || signature.purpose != expected_purpose {
            return Err(ReleaseError::SigningPurposeMismatch);
        }
        if !key.status.permits_verification() {
            return Err(ReleaseError::SigningKeyRevoked);
        }
        if signature.signed_at.0 < key.not_before.0
            || signature.signed_at.0 > now.0
            || key
                .not_after
                .is_some_and(|end| signature.signed_at.0 > end.0)
        {
            return Err(ReleaseError::SigningKeyExpired);
        }
        verify_digest(key.public_key, digest, signature)?;
        Ok(())
    }
}

/// Signed update metadata lifecycle. Halted and revoked releases are never installable.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum UpdateStatus {
    Available,
    Halted,
    Revoked,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum UpdateCriticality {
    Routine,
    Recommended,
    RequiredForSecurity,
    RequiredForCompatibility,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AutomaticUpdatePolicy {
    NotifyOnly,
    DownloadAutomatically,
    InstallOnRestart,
    ManagedExternally,
}

/// Replay-resistant, rollout-aware metadata used to decide whether bytes may be installed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateMetadata {
    pub metadata_generation: u64,
    pub release_id: String,
    pub version: Version,
    pub channel: ReleaseChannel,
    pub target: String,
    pub artifact_name: String,
    pub artifact_size: u64,
    pub artifact_hashes: ArtifactHashes,
    pub minimum_compatible_version: Version,
    pub criticality: UpdateCriticality,
    pub status: UpdateStatus,
    pub rollout_basis_points: u16,
    pub issued_at: CryptoTimestamp,
    pub expires_at: CryptoTimestamp,
}

impl UpdateMetadata {
    fn validate(&self) -> Result<(), ReleaseError> {
        validate_token(&self.release_id)?;
        validate_token(&self.target)?;
        validate_ref(&self.artifact_name)?;
        if self.metadata_generation == 0
            || self.artifact_size == 0
            || self.rollout_basis_points > 10_000
            || self.issued_at.0 >= self.expires_at.0
            || self.artifact_hashes.sha256 == [0; 32]
            || self.artifact_hashes.blake3 == [0; 32]
        {
            return Err(ReleaseError::InvalidUpdateMetadata);
        }
        Ok(())
    }

    fn signing_digest(&self) -> Result<[u8; 32], ReleaseError> {
        let bytes = canonical_bytes(self).map_err(|_| ReleaseError::CanonicalEncoding)?;
        Ok(domain_digest("AEQUORA:UPDATE-METADATA:v1", &bytes))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SignedUpdateMetadata {
    pub metadata: UpdateMetadata,
    pub signature: SignatureEnvelope,
}

impl SignedUpdateMetadata {
    /// Verifies signed metadata, expiry with bounded clock skew, and artifact bytes.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid signatures, expired/revoked metadata, or changed bytes.
    pub fn verify(
        &self,
        artifact: &[u8],
        trust: &ReleaseTrustStore,
        now: CryptoTimestamp,
        allowed_clock_skew_seconds: u32,
    ) -> Result<(), ReleaseError> {
        self.metadata.validate()?;
        let earliest = now.0.saturating_sub(i64::from(allowed_clock_skew_seconds));
        let latest = now.0.saturating_add(i64::from(allowed_clock_skew_seconds));
        if latest < self.metadata.issued_at.0 || earliest > self.metadata.expires_at.0 {
            return Err(ReleaseError::ExpiredUpdateMetadata);
        }
        if self.metadata.status != UpdateStatus::Available {
            return Err(ReleaseError::ReleaseUnavailable(self.metadata.status));
        }
        trust.verify(
            KeyPurpose::UpdateMetadataSigning,
            self.metadata.signing_digest()?,
            &self.signature,
            now,
        )?;
        if usize::try_from(self.metadata.artifact_size).ok() != Some(artifact.len())
            || !self.metadata.artifact_hashes.matches(artifact)
        {
            return Err(ReleaseError::ArtifactDigestMismatch(
                self.metadata.artifact_name.clone(),
            ));
        }
        Ok(())
    }
}

/// Signs complete update metadata with a key that cannot sign artifacts or manifests.
///
/// # Errors
///
/// Returns an error for invalid metadata or a rejected signing operation.
pub async fn sign_update_metadata<P: KeyProvider + ?Sized>(
    metadata: UpdateMetadata,
    provider: &P,
    key_id: SigningKeyId,
    signed_at: CryptoTimestamp,
) -> Result<SignedUpdateMetadata, ReleaseError> {
    metadata.validate()?;
    let signature = sign_digest(
        provider,
        KeyPurpose::UpdateMetadataSigning,
        key_id,
        signed_at,
        metadata.signing_digest()?,
    )
    .await?;
    Ok(SignedUpdateMetadata {
        metadata,
        signature,
    })
}

/// Current installed semantic and state-format dimensions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallationState {
    pub version: Version,
    pub target: String,
    pub last_update_generation: u64,
    pub protocol_version: u32,
    pub config_schema: u32,
    pub registry_generation: u64,
    pub store_formats: BTreeMap<String, u32>,
    pub pending_operations: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateDecision {
    NoUpdate,
    Notify,
    Download,
    InstallOnRestart,
    ManagedExternally,
    OutsideRollout,
    UpdateRequired,
}

/// Makes a deterministic channel, platform, minimum-version, compatibility, and cohort decision.
///
/// # Errors
///
/// Returns an error when an offered update is incompatible or attempts a downgrade.
pub fn decide_update(
    installation: &InstallationState,
    metadata: &UpdateMetadata,
    compatibility: &CompatibilityMatrix,
    configured_channel: ReleaseChannel,
    policy: AutomaticUpdatePolicy,
    installation_id: &[u8],
) -> Result<UpdateDecision, ReleaseError> {
    metadata.validate()?;
    compatibility.validate()?;
    validate_token(&installation.target)?;
    if metadata.status != UpdateStatus::Available {
        return Err(ReleaseError::ReleaseUnavailable(metadata.status));
    }
    if metadata.metadata_generation < installation.last_update_generation {
        return Err(ReleaseError::StaleUpdateMetadata);
    }
    if metadata.target != installation.target {
        return Err(ReleaseError::TargetMismatch);
    }
    if configured_channel == ReleaseChannel::EnterprisePinned
        || policy == AutomaticUpdatePolicy::ManagedExternally
    {
        return Ok(UpdateDecision::ManagedExternally);
    }
    if metadata.channel != configured_channel {
        return Err(ReleaseError::ChannelMismatch);
    }
    if metadata.version < installation.version {
        return Err(ReleaseError::DowngradeRejected);
    }
    if metadata.version == installation.version {
        return Ok(UpdateDecision::NoUpdate);
    }
    if installation.version < metadata.minimum_compatible_version
        || !(compatibility.protocol_min..=compatibility.protocol_max)
            .contains(&installation.protocol_version)
        || installation.config_schema > compatibility.config_schema
        || installation.registry_generation > compatibility.registry_generation
        || installation.store_formats.iter().any(|(adapter, current)| {
            compatibility
                .store_formats
                .get(adapter)
                .is_none_or(|target| current > target)
        })
    {
        return Err(ReleaseError::IncompatibleUpgrade);
    }
    let cohort = u16::from_be_bytes(
        blake3::hash(&[installation_id, metadata.release_id.as_bytes()].concat()).as_bytes()[..2]
            .try_into()
            .map_err(|_| ReleaseError::InvalidUpdateMetadata)?,
    ) % 10_000;
    if cohort >= metadata.rollout_basis_points {
        return Ok(UpdateDecision::OutsideRollout);
    }
    if matches!(
        metadata.criticality,
        UpdateCriticality::RequiredForSecurity | UpdateCriticality::RequiredForCompatibility
    ) {
        return Ok(UpdateDecision::UpdateRequired);
    }
    Ok(match policy {
        AutomaticUpdatePolicy::NotifyOnly => UpdateDecision::Notify,
        AutomaticUpdatePolicy::DownloadAutomatically => UpdateDecision::Download,
        AutomaticUpdatePolicy::InstallOnRestart => UpdateDecision::InstallOnRestart,
        AutomaticUpdatePolicy::ManagedExternally => UpdateDecision::ManagedExternally,
    })
}

/// Proves whether an older binary can safely open state produced by the target release.
///
/// # Errors
///
/// Rejects forward-only releases, missing rollback migrations, or older format limits.
pub fn verify_rollback(
    target: &ReleaseManifest,
    older: &CompatibilityMatrix,
) -> Result<(), ReleaseError> {
    target.validate()?;
    older.validate()?;
    match target.rollback_class {
        RollbackClass::ForwardOnly => return Err(ReleaseError::RollbackRejected),
        RollbackClass::RollbackRequiresMigration
            if !target
                .migrations
                .iter()
                .all(|migration| migration.reversible) =>
        {
            return Err(ReleaseError::RollbackMigrationMissing);
        }
        RollbackClass::BinaryRollbackSafe | RollbackClass::RollbackRequiresMigration => {}
    }
    if target.compatibility.protocol_min < older.protocol_min
        || target.compatibility.protocol_max > older.protocol_max
        || target.compatibility.config_schema > older.config_schema
        || target
            .compatibility
            .store_formats
            .iter()
            .any(|(adapter, version)| {
                older
                    .store_formats
                    .get(adapter)
                    .is_none_or(|older_max| version > older_max)
            })
    {
        return Err(ReleaseError::RollbackRejected);
    }
    Ok(())
}

/// Immutable semantic-version index used by repositories and channel promotion tooling.
#[derive(Clone, Debug, Default)]
pub struct ImmutableReleaseIndex(BTreeMap<Version, [u8; 32]>);

impl ImmutableReleaseIndex {
    /// Records a release, allowing exact idempotent republishing but never different bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the semantic version already names a different manifest digest.
    pub fn record(&mut self, version: Version, manifest_bytes: &[u8]) -> Result<(), ReleaseError> {
        let digest = *blake3::hash(manifest_bytes).as_bytes();
        if self
            .0
            .get(&version)
            .is_some_and(|existing| existing != &digest)
        {
            return Err(ReleaseError::ImmutableVersionConflict(version));
        }
        self.0.insert(version, digest);
        Ok(())
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ReleaseError {
    #[error("release manifest format is unsupported")]
    UnsupportedManifestVersion,
    #[error("release manifest is invalid: {0}")]
    InvalidManifest(&'static str),
    #[error("release compatibility matrix is invalid")]
    InvalidCompatibility,
    #[error("migration bundle is invalid")]
    InvalidMigrationBundle,
    #[error("release metadata contains an invalid identifier or path")]
    InvalidText,
    #[error("release channel does not match its semantic pre-release version")]
    InvalidChannelVersion,
    #[error("release artifact name is not versioned and target-qualified: {0}")]
    InvalidArtifactName(String),
    #[error("artifact is listed more than once: {0}")]
    DuplicateArtifact(String),
    #[error("artifact reference is not present in the release set: {0}")]
    MissingArtifactReference(String),
    #[error("release is missing required artifact kind: {0:?}")]
    MissingRequiredArtifact(ArtifactKind),
    #[error("distributable artifact lacks SBOM or provenance reference: {0}")]
    MissingSupplyChainReference(String),
    #[error("migration set digest does not match any bundled migration set")]
    MigrationDigestMismatch,
    #[error("artifact bytes do not match the release descriptor: {0}")]
    ArtifactDigestMismatch(String),
    #[error("canonical release encoding failed")]
    CanonicalEncoding,
    #[error("signing identity has the wrong purpose")]
    SigningPurposeMismatch,
    #[error("signing key is unknown")]
    UnknownSigningKey,
    #[error("signing key is revoked or destroyed")]
    SigningKeyRevoked,
    #[error("signing key or signature is outside its validity window")]
    SigningKeyExpired,
    #[error("update metadata is invalid")]
    InvalidUpdateMetadata,
    #[error("update metadata has expired")]
    ExpiredUpdateMetadata,
    #[error("release is not available: {0:?}")]
    ReleaseUnavailable(UpdateStatus),
    #[error("update channel does not match configured policy")]
    ChannelMismatch,
    #[error("update target does not match this installation")]
    TargetMismatch,
    #[error("update metadata generation is older than the last accepted generation")]
    StaleUpdateMetadata,
    #[error("update would downgrade the installed semantic version")]
    DowngradeRejected,
    #[error("update is incompatible with installed protocol, store, config, or registry state")]
    IncompatibleUpgrade,
    #[error("rollback is incompatible with target state")]
    RollbackRejected,
    #[error("rollback requires a reversible migration that is not present")]
    RollbackMigrationMissing,
    #[error("semantic version {0} is already bound to different immutable bytes")]
    ImmutableVersionConflict(Version),
    #[error("cross-platform support matrix is invalid")]
    InvalidSupportMatrix,
    #[error("cross-platform support matrix RON is malformed")]
    SupportMatrixEncoding,
    #[error(
        "stable promotion lacks complete gates, immutable bytes, approval, or credential isolation"
    )]
    PromotionRejected,
    #[error("release signing failed")]
    Signing,
    #[error("release signature verification failed")]
    Verification,
}

impl From<SigningError> for ReleaseError {
    fn from(_: SigningError) -> Self {
        Self::Signing
    }
}

impl From<VerificationError> for ReleaseError {
    fn from(_: VerificationError) -> Self {
        Self::Verification
    }
}

fn validate_token(value: &str) -> Result<(), ReleaseError> {
    if value.is_empty()
        || value.len() > MAX_TEXT_BYTES
        || value.chars().any(char::is_control)
        || value.contains("..")
    {
        return Err(ReleaseError::InvalidText);
    }
    Ok(())
}

fn validate_channel_version(
    channel: ReleaseChannel,
    version: &Version,
) -> Result<(), ReleaseError> {
    let prerelease = version.pre.as_str();
    let valid = match channel {
        ReleaseChannel::Stable | ReleaseChannel::EnterprisePinned => prerelease.is_empty(),
        ReleaseChannel::Alpha => prerelease.starts_with("alpha."),
        ReleaseChannel::Beta => prerelease.starts_with("beta."),
        ReleaseChannel::ReleaseCandidate => prerelease.starts_with("rc."),
        ReleaseChannel::Nightly => !prerelease.is_empty(),
    };
    if valid {
        Ok(())
    } else {
        Err(ReleaseError::InvalidChannelVersion)
    }
}

fn validate_ref(value: &str) -> Result<(), ReleaseError> {
    validate_token(value)?;
    if value.starts_with('/') || value.starts_with('\\') {
        return Err(ReleaseError::InvalidText);
    }
    Ok(())
}

fn validate_optional_ref(value: Option<&str>) -> Result<(), ReleaseError> {
    if let Some(value) = value {
        validate_ref(value)?;
    }
    Ok(())
}

fn validate_hex(value: &str, exact_len: usize) -> Result<(), ReleaseError> {
    if value.len() != exact_len || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ReleaseError::InvalidText);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
