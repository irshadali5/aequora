//! Runtime-, transport-, provider-, and database-neutral security contracts.
//!
//! The crate turns Part 27's threat model into typed, fail-closed boundaries. Authentication
//! adapters establish [`AuthenticationEvidence`]; authoritative services consume only a
//! [`ValidatedAuthContext`]. External inputs pass explicit budgets, outbound destinations are
//! checked after DNS resolution, durable operation identities bind immutable semantics, and
//! security telemetry remains payload-free and low-cardinality.

#![forbid(unsafe_code)]
#![allow(clippy::missing_errors_doc)]

use aequora_types::{ActorId, AuthorityEpoch, DeviceId, OperationId, TenantId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    net::{IpAddr, Ipv4Addr},
};
use thiserror::Error;
use uuid::Uuid;
use zeroize::Zeroizing;

/// Schema version for serialized security policy and event contracts.
pub const SECURITY_SCHEMA_VERSION: u16 = 1;
/// Maximum length of issuer, audience, host, and stable reason identifiers.
pub const MAX_SECURITY_IDENTIFIER_BYTES: usize = 256;

macro_rules! uuid_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

uuid_id!(SecurityEventId, "Stable identity of one security event.");

/// Deployment security posture. These are architecture profiles, not certifications.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum SecurityLevel {
    Standard,
    Enterprise,
    HighAssurance,
}

/// Server-observed authentication mechanism.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum AuthMethod {
    BearerToken,
    CookieSession,
    MutualTls,
    WorkloadIdentity,
    DeviceSignature,
}

/// Authentication strength used for step-up decisions.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum AssuranceLevel {
    Normal,
    MultiFactor,
    HardwareBacked,
    BreakGlass,
}

/// Current server-side device state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DeviceState {
    Active,
    VerificationOnly,
    Revoked,
}

/// Authentication evidence produced by a trusted authentication adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthenticationEvidence {
    pub principal_id: ActorId,
    pub tenant_id: TenantId,
    pub device_id: Option<DeviceId>,
    pub auth_method: AuthMethod,
    pub assurance: AssuranceLevel,
    pub issuer: String,
    pub audience: String,
    pub authenticated_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub device_state: Option<DeviceState>,
}

/// Authentication policy enforced before protected traffic enters domain authorization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthenticationPolicy {
    pub trusted_issuers: BTreeSet<String>,
    pub required_audience: String,
    pub require_device_binding: bool,
    pub minimum_assurance: AssuranceLevel,
    pub max_authentication_age_ms: u64,
}

impl AuthenticationPolicy {
    pub fn validate(&self) -> Result<(), SecurityError> {
        if self.trusted_issuers.is_empty()
            || self.required_audience.is_empty()
            || self.max_authentication_age_ms == 0
            || self.required_audience.len() > MAX_SECURITY_IDENTIFIER_BYTES
            || self
                .trusted_issuers
                .iter()
                .any(|issuer| issuer.is_empty() || issuer.len() > MAX_SECURITY_IDENTIFIER_BYTES)
        {
            return Err(SecurityError::InvalidPolicy("authentication"));
        }
        Ok(())
    }

    pub fn authenticate(
        &self,
        evidence: AuthenticationEvidence,
        now_unix_ms: u64,
    ) -> Result<ValidatedAuthContext, SecurityError> {
        self.validate()?;
        if !self.trusted_issuers.contains(&evidence.issuer)
            || evidence.audience != self.required_audience
        {
            return Err(SecurityError::AuthenticationInvalid);
        }
        if now_unix_ms >= evidence.expires_at_unix_ms {
            return Err(SecurityError::AuthenticationExpired);
        }
        let age = now_unix_ms
            .checked_sub(evidence.authenticated_at_unix_ms)
            .ok_or(SecurityError::AuthenticationInvalid)?;
        if age > self.max_authentication_age_ms || evidence.assurance < self.minimum_assurance {
            return Err(SecurityError::InsufficientAssurance);
        }
        if self.require_device_binding && evidence.device_id.is_none() {
            return Err(SecurityError::DeviceBindingRequired);
        }
        if matches!(evidence.device_state, Some(DeviceState::Revoked)) {
            return Err(SecurityError::DeviceRevoked);
        }
        if evidence.device_id.is_some() && evidence.device_state.is_none() {
            return Err(SecurityError::AuthenticationInvalid);
        }
        Ok(ValidatedAuthContext(evidence))
    }
}

/// Validated identity. Fields remain private so callers cannot construct trusted context directly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedAuthContext(AuthenticationEvidence);

impl ValidatedAuthContext {
    #[must_use]
    pub const fn principal_id(&self) -> ActorId {
        self.0.principal_id
    }

    #[must_use]
    pub const fn tenant_id(&self) -> TenantId {
        self.0.tenant_id
    }

    #[must_use]
    pub const fn device_id(&self) -> Option<DeviceId> {
        self.0.device_id
    }

    #[must_use]
    pub const fn auth_method(&self) -> AuthMethod {
        self.0.auth_method
    }

    #[must_use]
    pub const fn assurance(&self) -> AssuranceLevel {
        self.0.assurance
    }

    pub fn bind_tenant(&self, claimed_tenant: TenantId) -> Result<TenantBinding, SecurityError> {
        if claimed_tenant != self.0.tenant_id {
            return Err(SecurityError::TenantMismatch);
        }
        Ok(TenantBinding {
            tenant_id: self.0.tenant_id,
            principal_id: self.0.principal_id,
        })
    }
}

/// Proof that a client claim was checked against server-derived identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TenantBinding {
    tenant_id: TenantId,
    principal_id: ActorId,
}

impl TenantBinding {
    #[must_use]
    pub const fn tenant_id(self) -> TenantId {
        self.tenant_id
    }

    #[must_use]
    pub const fn principal_id(self) -> ActorId {
        self.principal_id
    }

    pub fn authorize_resource(
        self,
        resource: TenantResource,
    ) -> Result<AuthorizedResource, SecurityError> {
        if resource.tenant_id != self.tenant_id {
            return Err(SecurityError::NotFoundOrForbidden);
        }
        Ok(AuthorizedResource(resource))
    }
}

/// Identifier category used without exposing the underlying business identifier.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ResourceKind {
    Entity,
    Scope,
    Blob,
    Operation,
    Snapshot,
    Export,
}

/// Tenant ownership resolved from a trusted repository lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TenantResource {
    pub tenant_id: TenantId,
    pub kind: ResourceKind,
}

/// Resource whose trusted ownership matched the authenticated tenant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorizedResource(TenantResource);

impl AuthorizedResource {
    #[must_use]
    pub const fn kind(self) -> ResourceKind {
        self.0.kind
    }
}

/// Bounds applied before or during parsing externally controlled input.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProtocolLimits {
    pub max_frame_bytes: usize,
    pub max_operations_per_batch: usize,
    pub max_collection_items: usize,
    pub max_nesting_depth: usize,
    pub max_dependency_edges: usize,
    pub max_string_bytes: usize,
    pub max_compressed_bytes: usize,
    pub max_decompressed_bytes: usize,
    pub max_compression_ratio: usize,
}

impl ProtocolLimits {
    pub fn validate(self) -> Result<(), SecurityError> {
        let values = [
            self.max_frame_bytes,
            self.max_operations_per_batch,
            self.max_collection_items,
            self.max_nesting_depth,
            self.max_dependency_edges,
            self.max_string_bytes,
            self.max_compressed_bytes,
            self.max_decompressed_bytes,
            self.max_compression_ratio,
        ];
        if values.contains(&0)
            || self.max_compressed_bytes > self.max_decompressed_bytes
            || self.max_frame_bytes > self.max_decompressed_bytes
        {
            return Err(SecurityError::InvalidPolicy("protocol limits"));
        }
        Ok(())
    }

    pub fn validate_input(self, input: InputShape) -> Result<(), SecurityError> {
        self.validate()?;
        if input.frame_bytes > self.max_frame_bytes
            || input.collection_items > self.max_collection_items
            || input.operations > self.max_operations_per_batch
            || input.nesting_depth > self.max_nesting_depth
            || input.dependency_edges > self.max_dependency_edges
            || input.longest_string_bytes > self.max_string_bytes
        {
            return Err(SecurityError::InputLimitExceeded);
        }
        Ok(())
    }

    pub fn validate_compression(
        self,
        compressed_bytes: usize,
        decompressed_bytes: usize,
    ) -> Result<(), SecurityError> {
        self.validate()?;
        if compressed_bytes > self.max_compressed_bytes
            || decompressed_bytes > self.max_decompressed_bytes
        {
            return Err(SecurityError::CompressionLimitExceeded);
        }
        let permitted = compressed_bytes
            .max(1)
            .checked_mul(self.max_compression_ratio)
            .ok_or(SecurityError::ArithmeticOverflow)?;
        if decompressed_bytes > permitted {
            return Err(SecurityError::CompressionLimitExceeded);
        }
        Ok(())
    }
}

/// Measured shape supplied by a bounded decoder.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InputShape {
    pub frame_bytes: usize,
    pub collection_items: usize,
    pub operations: usize,
    pub nesting_depth: usize,
    pub dependency_edges: usize,
    pub longest_string_bytes: usize,
}

/// Bounds for uploaded blobs and archive expansion.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UploadLimits {
    pub max_blob_bytes: u64,
    pub max_chunks: usize,
    pub max_archive_entries: usize,
    pub max_archive_expanded_bytes: u64,
    pub max_archive_ratio: u64,
    pub max_path_bytes: usize,
}

impl UploadLimits {
    pub fn validate(self) -> Result<(), SecurityError> {
        if self.max_blob_bytes == 0
            || self.max_chunks == 0
            || self.max_archive_entries == 0
            || self.max_archive_expanded_bytes == 0
            || self.max_archive_ratio == 0
            || self.max_path_bytes == 0
        {
            return Err(SecurityError::InvalidPolicy("upload limits"));
        }
        Ok(())
    }

    pub fn validate_archive(self, entries: &[ArchiveEntry]) -> Result<(), SecurityError> {
        self.validate()?;
        if entries.len() > self.max_archive_entries {
            return Err(SecurityError::ArchiveLimitExceeded);
        }
        let mut compressed = 0_u64;
        let mut expanded = 0_u64;
        for entry in entries {
            entry.validate_path(self.max_path_bytes)?;
            if entry.kind != ArchiveEntryKind::RegularFile {
                return Err(SecurityError::UnsafeArchiveEntry);
            }
            compressed = compressed
                .checked_add(entry.compressed_bytes)
                .ok_or(SecurityError::ArithmeticOverflow)?;
            expanded = expanded
                .checked_add(entry.expanded_bytes)
                .ok_or(SecurityError::ArithmeticOverflow)?;
        }
        let allowed_expansion = compressed
            .max(1)
            .checked_mul(self.max_archive_ratio)
            .ok_or(SecurityError::ArithmeticOverflow)?;
        if expanded > self.max_archive_expanded_bytes || expanded > allowed_expansion {
            return Err(SecurityError::ArchiveLimitExceeded);
        }
        Ok(())
    }
}

/// Archive entry metadata validated before extraction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveEntry {
    pub relative_path: String,
    pub kind: ArchiveEntryKind,
    pub compressed_bytes: u64,
    pub expanded_bytes: u64,
}

impl ArchiveEntry {
    fn validate_path(&self, max_path_bytes: usize) -> Result<(), SecurityError> {
        let path = self.relative_path.as_str();
        if path.is_empty()
            || path.len() > max_path_bytes
            || path.starts_with('/')
            || path.starts_with('\\')
            || path.contains('\\')
            || path.contains('\0')
            || path
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
            || path.as_bytes().get(1) == Some(&b':')
        {
            return Err(SecurityError::UnsafeArchivePath);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveEntryKind {
    RegularFile,
    Directory,
    SymbolicLink,
    HardLink,
    Device,
}

/// Egress policy applied to every initial URL and every redirect.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EgressPolicy {
    pub allow_plain_http: bool,
    pub block_private_networks: bool,
    pub max_redirects: usize,
    pub max_url_bytes: usize,
}

impl EgressPolicy {
    pub fn validate(self) -> Result<(), SecurityError> {
        if self.max_url_bytes == 0 || self.max_url_bytes > 16 * 1024 {
            return Err(SecurityError::InvalidPolicy("egress"));
        }
        Ok(())
    }

    pub fn validate_target(
        self,
        url: &str,
        resolved_addresses: &[IpAddr],
        redirect_count: usize,
    ) -> Result<ValidatedOutboundTarget, SecurityError> {
        self.validate()?;
        if url.len() > self.max_url_bytes || redirect_count > self.max_redirects {
            return Err(SecurityError::SsrfBlocked);
        }
        let parsed = ParsedTarget::parse(url, self.allow_plain_http)?;
        parsed.validate_addresses(resolved_addresses, self.block_private_networks)?;
        Ok(ValidatedOutboundTarget {
            scheme: parsed.scheme,
            host: parsed.host,
            port: parsed.port,
            addresses: resolved_addresses.to_vec(),
            redirect_count,
        })
    }
}

struct ParsedTarget {
    scheme: OutboundScheme,
    host: String,
    port: Option<u16>,
}

impl ParsedTarget {
    fn parse(url: &str, allow_plain_http: bool) -> Result<Self, SecurityError> {
        let (raw_scheme, rest) = url.split_once("://").ok_or(SecurityError::SsrfBlocked)?;
        let scheme = match raw_scheme.to_ascii_lowercase().as_str() {
            "https" => OutboundScheme::Https,
            "http" if allow_plain_http => OutboundScheme::Http,
            _ => return Err(SecurityError::SsrfBlocked),
        };
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if authority.is_empty() || authority.contains('@') {
            return Err(SecurityError::SsrfBlocked);
        }
        let (host, port) = parse_authority(authority)?;
        let normalized = host.trim_end_matches('.').to_ascii_lowercase();
        if normalized.is_empty()
            || normalized == "localhost"
            || normalized.ends_with(".localhost")
            || normalized == "metadata.google.internal"
        {
            return Err(SecurityError::SsrfBlocked);
        }
        Ok(Self {
            scheme,
            host: normalized,
            port,
        })
    }

    fn validate_addresses(
        &self,
        addresses: &[IpAddr],
        block_private: bool,
    ) -> Result<(), SecurityError> {
        if addresses.is_empty()
            || addresses
                .iter()
                .any(|address| block_private && !is_public_address(*address))
        {
            return Err(SecurityError::SsrfBlocked);
        }
        if let Ok(literal) = self.host.parse::<IpAddr>() {
            if !addresses.contains(&literal) {
                return Err(SecurityError::SsrfBlocked);
            }
        }
        Ok(())
    }
}

fn parse_authority(authority: &str) -> Result<(&str, Option<u16>), SecurityError> {
    if let Some(bracketed) = authority.strip_prefix('[') {
        let (host, suffix) = bracketed
            .split_once(']')
            .ok_or(SecurityError::SsrfBlocked)?;
        let port = match suffix.strip_prefix(':') {
            Some(value) if !value.is_empty() => {
                Some(value.parse().map_err(|_| SecurityError::SsrfBlocked)?)
            }
            None if suffix.is_empty() => None,
            _ => return Err(SecurityError::SsrfBlocked),
        };
        host.parse::<std::net::Ipv6Addr>()
            .map_err(|_| SecurityError::SsrfBlocked)?;
        return Ok((host, port));
    }
    if authority.matches(':').count() > 1 {
        return Err(SecurityError::SsrfBlocked);
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && !port.is_empty() => Ok((
            host,
            Some(port.parse().map_err(|_| SecurityError::SsrfBlocked)?),
        )),
        Some(_) => Err(SecurityError::SsrfBlocked),
        None => Ok((authority, None)),
    }
}

fn is_public_address(address: IpAddr) -> bool {
    if address.is_loopback() || address.is_unspecified() || address.is_multicast() {
        return false;
    }
    match address {
        IpAddr::V4(value) => {
            !value.is_private()
                && !value.is_link_local()
                && !value.is_broadcast()
                && !value.is_documentation()
                && value != Ipv4Addr::new(169, 254, 169, 254)
                && value.octets()[0] != 0
                && !(value.octets()[0] == 100 && (64..=127).contains(&value.octets()[1]))
                && !(value.octets()[0] == 198 && matches!(value.octets()[1], 18 | 19))
                && value.octets()[0] < 240
        }
        IpAddr::V6(value) => {
            if let Some(mapped) = value.to_ipv4_mapped() {
                return is_public_address(IpAddr::V4(mapped));
            }
            !(value.is_unique_local()
                || value.is_unicast_link_local()
                || value.segments()[0] & 0xffc0 == 0xfec0
                || (value.segments()[0] == 0x2001 && value.segments()[1] == 0x0db8))
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum OutboundScheme {
    Http,
    Https,
}

/// Destination checked against DNS answers. Revalidate immediately before every connection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedOutboundTarget {
    pub scheme: OutboundScheme,
    pub host: String,
    pub port: Option<u16>,
    pub addresses: Vec<IpAddr>,
    pub redirect_count: usize,
}

impl ValidatedOutboundTarget {
    pub fn revalidate_connection(
        &self,
        policy: EgressPolicy,
        connected_address: IpAddr,
    ) -> Result<(), SecurityError> {
        if !self.addresses.contains(&connected_address)
            || (policy.block_private_networks && !is_public_address(connected_address))
        {
            return Err(SecurityError::SsrfBlocked);
        }
        Ok(())
    }
}

/// Immutable semantic binding for one operation identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OperationBinding {
    pub operation_id: OperationId,
    pub tenant_id: TenantId,
    pub digest: [u8; 32],
}

impl OperationBinding {
    #[must_use]
    pub fn new(
        operation_id: OperationId,
        tenant_id: TenantId,
        actor_id: ActorId,
        device_id: Option<DeviceId>,
        semantic_payload: &[u8],
    ) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"aequora.security.operation-binding.v1\0");
        hasher.update(tenant_id.as_uuid().as_bytes());
        hasher.update(actor_id.as_uuid().as_bytes());
        match device_id {
            Some(device_id) => {
                hasher.update(&[1]);
                hasher.update(device_id.as_uuid().as_bytes());
            }
            None => {
                hasher.update(&[0]);
            }
        }
        hasher.update(&(semantic_payload.len() as u64).to_le_bytes());
        hasher.update(semantic_payload);
        Self {
            operation_id,
            tenant_id,
            digest: *hasher.finalize().as_bytes(),
        }
    }
}

/// Bounded reference replay registry. Durable ledgers implement the same comparison contract.
#[derive(Debug)]
pub struct OperationReplayGuard {
    max_entries: usize,
    bindings: BTreeMap<OperationId, OperationBinding>,
}

impl OperationReplayGuard {
    pub fn new(max_entries: usize) -> Result<Self, SecurityError> {
        if max_entries == 0 {
            return Err(SecurityError::InvalidPolicy("replay registry"));
        }
        Ok(Self {
            max_entries,
            bindings: BTreeMap::new(),
        })
    }

    pub fn observe(
        &mut self,
        binding: OperationBinding,
    ) -> Result<ReplayDisposition, SecurityError> {
        if let Some(existing) = self.bindings.get(&binding.operation_id) {
            return if *existing == binding {
                Ok(ReplayDisposition::Duplicate)
            } else {
                Err(SecurityError::PayloadMismatch)
            };
        }
        if self.bindings.len() >= self.max_entries {
            return Err(SecurityError::InputLimitExceeded);
        }
        self.bindings.insert(binding.operation_id, binding);
        Ok(ReplayDisposition::FirstSeen)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayDisposition {
    FirstSeen,
    Duplicate,
}

/// Authority rollback guard retained by clients and trusted stores.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuthorityEpochGuard {
    highest_trusted: AuthorityEpoch,
}

impl AuthorityEpochGuard {
    #[must_use]
    pub const fn new(highest_trusted: AuthorityEpoch) -> Self {
        Self { highest_trusted }
    }

    pub fn observe(&mut self, observed: AuthorityEpoch) -> Result<EpochDisposition, SecurityError> {
        if observed < self.highest_trusted {
            return Err(SecurityError::AuthorityRollback);
        }
        if observed > self.highest_trusted {
            self.highest_trusted = observed;
            return Ok(EpochDisposition::Advanced);
        }
        Ok(EpochDisposition::Current)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EpochDisposition {
    Current,
    Advanced,
}

/// Side-effect risk category used to enforce explicit idempotency/reconciliation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SideEffectRisk {
    Reversible,
    Financial,
    Irreversible,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SideEffectSafety {
    pub risk: SideEffectRisk,
    pub idempotency_key: Option<String>,
    pub reconciliation_kind: Option<String>,
}

impl SideEffectSafety {
    pub fn validate(&self) -> Result<(), SecurityError> {
        if matches!(
            self.risk,
            SideEffectRisk::Financial | SideEffectRisk::Irreversible
        ) && (self.idempotency_key.as_deref().is_none_or(str::is_empty)
            || self
                .reconciliation_kind
                .as_deref()
                .is_none_or(str::is_empty))
        {
            return Err(SecurityError::UnsafeSideEffect);
        }
        if self
            .idempotency_key
            .iter()
            .chain(self.reconciliation_kind.iter())
            .any(|value| value.len() > MAX_SECURITY_IDENTIFIER_BYTES)
        {
            return Err(SecurityError::InputLimitExceeded);
        }
        Ok(())
    }
}

/// Secret text that zeroizes on drop, never serializes, and always redacts `Debug`/`Display`.
pub struct SecretString(Zeroizing<String>);

impl SecretString {
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    #[must_use]
    pub fn expose_secret(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("[REDACTED]")
    }
}

/// Typed, payload-free security events for audit and immediate alert routing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SecurityEvent {
    pub event_id: SecurityEventId,
    pub kind: SecurityEventKind,
    pub tenant_id: Option<TenantId>,
    pub principal_id: Option<ActorId>,
    pub severity: SecuritySeverity,
    pub occurred_at_unix_ms: u64,
    pub reason_code: SecurityErrorCode,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SecurityEventKind {
    AuthenticationFailure,
    AuthorizationDenied,
    DeviceRevokedAttempt,
    ProtocolDowngradeRejected,
    PayloadSubstitutionRejected,
    CrossTenantAttempt,
    SsrfBlocked,
    AuthorityRollbackDetected,
    ForkDetected,
    AdminOverride,
    LegacyWriteAfterCutover,
    KeyRevoked,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum SecuritySeverity {
    Informational,
    Low,
    Medium,
    High,
    Critical,
}

/// Stable client-safe error codes; variants do not carry secrets or internal details.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SecurityErrorCode {
    AuthInvalid,
    AuthExpired,
    AuthRevoked,
    AuthzDenied,
    TenantMismatch,
    ProtocolDowngrade,
    PayloadMismatch,
    InputLimitExceeded,
    SnapshotSignatureInvalid,
    AuthorityRollback,
    SsrfBlocked,
    UnsafeArchive,
    UnsafeSideEffect,
    Internal,
}

impl SecurityErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthInvalid => "AUTH_INVALID",
            Self::AuthExpired => "AUTH_EXPIRED",
            Self::AuthRevoked => "AUTH_REVOKED",
            Self::AuthzDenied => "AUTHZ_DENIED",
            Self::TenantMismatch => "TENANT_MISMATCH",
            Self::ProtocolDowngrade => "PROTOCOL_DOWNGRADE",
            Self::PayloadMismatch => "PAYLOAD_MISMATCH",
            Self::InputLimitExceeded => "INPUT_LIMIT_EXCEEDED",
            Self::SnapshotSignatureInvalid => "SNAPSHOT_SIGNATURE_INVALID",
            Self::AuthorityRollback => "AUTHORITY_ROLLBACK",
            Self::SsrfBlocked => "SSRF_BLOCKED",
            Self::UnsafeArchive => "UNSAFE_ARCHIVE",
            Self::UnsafeSideEffect => "UNSAFE_SIDE_EFFECT",
            Self::Internal => "INTERNAL",
        }
    }
}

/// Metric series allowed by core. Identity values belong in access-controlled logs/traces.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityMetric {
    AuthenticationFailure,
    AuthorizationDenied,
    DeviceRevokedAttempt,
    ProtocolDowngradeRejected,
    SsrfBlocked,
    SignatureInvalid,
    CrossTenantDenied,
}

impl SecurityMetric {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AuthenticationFailure => "auth_failure_total",
            Self::AuthorizationDenied => "authz_denied_total",
            Self::DeviceRevokedAttempt => "device_revoked_attempt_total",
            Self::ProtocolDowngradeRejected => "protocol_downgrade_rejected_total",
            Self::SsrfBlocked => "ssrf_blocked_total",
            Self::SignatureInvalid => "signature_invalid_total",
            Self::CrossTenantDenied => "cross_tenant_denied_total",
        }
    }
}

/// Mandatory browser control that a host adapter must enforce.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum BrowserControl {
    SecureCookie,
    HttpOnlyCookie,
    SameSiteCookie,
    CsrfProtection,
    Hsts,
    ContentSecurityPolicy,
    NoSniff,
}

/// Deployment control required by a security profile.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum DeploymentControl {
    PrivateAdminListener,
    MfaForDestructiveAdmin,
    SignedArtifacts,
    InternalMutualTls,
    TwoPersonDestructiveApproval,
}

/// Complete security policy with explicit safe profiles.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SecurityPolicy {
    pub schema_version: u16,
    pub level: SecurityLevel,
    pub protocol: ProtocolLimits,
    pub auth: AuthenticationPolicy,
    pub egress: EgressPolicy,
    pub uploads: UploadLimits,
    pub browser_controls: BTreeSet<BrowserControl>,
    pub deployment_controls: BTreeSet<DeploymentControl>,
}

impl SecurityPolicy {
    #[must_use]
    pub fn standard(trusted_issuer: impl Into<String>, audience: impl Into<String>) -> Self {
        let trusted_issuer = trusted_issuer.into();
        Self {
            schema_version: SECURITY_SCHEMA_VERSION,
            level: SecurityLevel::Standard,
            protocol: ProtocolLimits {
                max_frame_bytes: 4 * 1024 * 1024,
                max_operations_per_batch: 1_000,
                max_collection_items: 10_000,
                max_nesting_depth: 64,
                max_dependency_edges: 5_000,
                max_string_bytes: 256 * 1024,
                max_compressed_bytes: 4 * 1024 * 1024,
                max_decompressed_bytes: 64 * 1024 * 1024,
                max_compression_ratio: 64,
            },
            auth: AuthenticationPolicy {
                trusted_issuers: BTreeSet::from([trusted_issuer]),
                required_audience: audience.into(),
                require_device_binding: true,
                minimum_assurance: AssuranceLevel::Normal,
                max_authentication_age_ms: 24 * 60 * 60 * 1_000,
            },
            egress: EgressPolicy {
                allow_plain_http: false,
                block_private_networks: true,
                max_redirects: 0,
                max_url_bytes: 2_048,
            },
            uploads: UploadLimits {
                max_blob_bytes: 100 * 1024 * 1024,
                max_chunks: 10_000,
                max_archive_entries: 10_000,
                max_archive_expanded_bytes: 512 * 1024 * 1024,
                max_archive_ratio: 64,
                max_path_bytes: 512,
            },
            browser_controls: BTreeSet::from([
                BrowserControl::SecureCookie,
                BrowserControl::HttpOnlyCookie,
                BrowserControl::SameSiteCookie,
                BrowserControl::CsrfProtection,
                BrowserControl::Hsts,
                BrowserControl::ContentSecurityPolicy,
                BrowserControl::NoSniff,
            ]),
            deployment_controls: BTreeSet::from([
                DeploymentControl::PrivateAdminListener,
                DeploymentControl::MfaForDestructiveAdmin,
            ]),
        }
    }

    #[must_use]
    pub fn enterprise(trusted_issuer: impl Into<String>, audience: impl Into<String>) -> Self {
        let mut policy = Self::standard(trusted_issuer, audience);
        policy.level = SecurityLevel::Enterprise;
        policy.auth.minimum_assurance = AssuranceLevel::MultiFactor;
        policy
            .deployment_controls
            .insert(DeploymentControl::SignedArtifacts);
        policy
            .deployment_controls
            .insert(DeploymentControl::InternalMutualTls);
        policy
    }

    #[must_use]
    pub fn high_assurance(trusted_issuer: impl Into<String>, audience: impl Into<String>) -> Self {
        let mut policy = Self::enterprise(trusted_issuer, audience);
        policy.level = SecurityLevel::HighAssurance;
        policy.auth.minimum_assurance = AssuranceLevel::HardwareBacked;
        policy
            .deployment_controls
            .insert(DeploymentControl::TwoPersonDestructiveApproval);
        policy
    }

    pub fn validate(&self) -> Result<(), SecurityError> {
        if self.schema_version != SECURITY_SCHEMA_VERSION {
            return Err(SecurityError::InvalidPolicy("schema version"));
        }
        self.protocol.validate()?;
        self.auth.validate()?;
        self.egress.validate()?;
        self.uploads.validate()?;
        let mandatory_browser = BTreeSet::from([
            BrowserControl::SecureCookie,
            BrowserControl::HttpOnlyCookie,
            BrowserControl::SameSiteCookie,
            BrowserControl::CsrfProtection,
            BrowserControl::NoSniff,
        ]);
        let mandatory_deployment = BTreeSet::from([
            DeploymentControl::PrivateAdminListener,
            DeploymentControl::MfaForDestructiveAdmin,
        ]);
        if !mandatory_browser.is_subset(&self.browser_controls)
            || !mandatory_deployment.is_subset(&self.deployment_controls)
        {
            return Err(SecurityError::UnsafeDefault);
        }
        match self.level {
            SecurityLevel::Standard => {}
            SecurityLevel::Enterprise => {
                if !self
                    .deployment_controls
                    .contains(&DeploymentControl::SignedArtifacts)
                    || !self
                        .deployment_controls
                        .contains(&DeploymentControl::InternalMutualTls)
                {
                    return Err(SecurityError::InvalidPolicy("enterprise requirements"));
                }
            }
            SecurityLevel::HighAssurance => {
                if !self
                    .deployment_controls
                    .contains(&DeploymentControl::SignedArtifacts)
                    || !self
                        .deployment_controls
                        .contains(&DeploymentControl::InternalMutualTls)
                    || !self
                        .deployment_controls
                        .contains(&DeploymentControl::TwoPersonDestructiveApproval)
                    || self.auth.minimum_assurance < AssuranceLevel::HardwareBacked
                {
                    return Err(SecurityError::InvalidPolicy("high-assurance requirements"));
                }
            }
        }
        Ok(())
    }
}

/// Critical assets enumerated by the normative threat model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityAsset {
    AuthoritativeState,
    OperationLedger,
    Journal,
    AuditTrail,
    TenantData,
    IdentityAndAuthorization,
    CryptographicKeys,
    AuthorityMetadata,
    GovernanceControls,
    ProviderReferences,
    AdminControlPlane,
}

/// Attacker classes that every subsystem threat review must consider.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttackerClass {
    UnauthenticatedInternet,
    AuthenticatedMaliciousUser,
    CompromisedClientDevice,
    MaliciousTenantAdministrator,
    CompromisedApplicationNode,
    MaliciousInsider,
    CompromisedProvider,
    NetworkAttacker,
    SupplyChainAttacker,
    ResourceExhaustionAttacker,
}

/// Explicit boundary where data must be authenticated, authorized, and bounded again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrustBoundary {
    ClientInternet,
    InternetDataPlane,
    AdminControlPlane,
    ServerDatabase,
    ServerObjectStorage,
    ServerKms,
    ServerProvider,
    WorkerProvider,
    ReplicaAuthority,
    LegacyBridgeSystem,
}

pub const SECURITY_ASSETS: [SecurityAsset; 11] = [
    SecurityAsset::AuthoritativeState,
    SecurityAsset::OperationLedger,
    SecurityAsset::Journal,
    SecurityAsset::AuditTrail,
    SecurityAsset::TenantData,
    SecurityAsset::IdentityAndAuthorization,
    SecurityAsset::CryptographicKeys,
    SecurityAsset::AuthorityMetadata,
    SecurityAsset::GovernanceControls,
    SecurityAsset::ProviderReferences,
    SecurityAsset::AdminControlPlane,
];

pub const ATTACKER_CLASSES: [AttackerClass; 10] = [
    AttackerClass::UnauthenticatedInternet,
    AttackerClass::AuthenticatedMaliciousUser,
    AttackerClass::CompromisedClientDevice,
    AttackerClass::MaliciousTenantAdministrator,
    AttackerClass::CompromisedApplicationNode,
    AttackerClass::MaliciousInsider,
    AttackerClass::CompromisedProvider,
    AttackerClass::NetworkAttacker,
    AttackerClass::SupplyChainAttacker,
    AttackerClass::ResourceExhaustionAttacker,
];

pub const TRUST_BOUNDARIES: [TrustBoundary; 10] = [
    TrustBoundary::ClientInternet,
    TrustBoundary::InternetDataPlane,
    TrustBoundary::AdminControlPlane,
    TrustBoundary::ServerDatabase,
    TrustBoundary::ServerObjectStorage,
    TrustBoundary::ServerKms,
    TrustBoundary::ServerProvider,
    TrustBoundary::WorkerProvider,
    TrustBoundary::ReplicaAuthority,
    TrustBoundary::LegacyBridgeSystem,
];

/// Stable Part 27 invariant registry entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecurityInvariant {
    pub id: &'static str,
    pub summary: &'static str,
    pub regression_test: &'static str,
    pub metric: &'static str,
}

pub const SECURITY_INVARIANTS: [SecurityInvariant; 10] = [
    SecurityInvariant {
        id: "AEQ-INV-SEC001",
        summary: "client claims are never authorization evidence",
        regression_test: "authentication_and_tenant_binding",
        metric: "authz_denied_total",
    },
    SecurityInvariant {
        id: "AEQ-INV-SEC002",
        summary: "operation identity binds immutable semantics",
        regression_test: "operation_payload_substitution",
        metric: "payload_mismatch_total",
    },
    SecurityInvariant {
        id: "AEQ-INV-SEC003",
        summary: "required security capabilities fail closed",
        regression_test: "required_security_capability_downgrade",
        metric: "protocol_downgrade_rejected_total",
    },
    SecurityInvariant {
        id: "AEQ-INV-SEC004",
        summary: "external input has explicit complexity bounds",
        regression_test: "boundary_plus_one_and_archive_bomb",
        metric: "input_limit_rejected_total",
    },
    SecurityInvariant {
        id: "AEQ-INV-SEC005",
        summary: "known identifiers never bypass tenant isolation",
        regression_test: "cross_tenant_resource_matrix",
        metric: "cross_tenant_denied_total",
    },
    SecurityInvariant {
        id: "AEQ-INV-SEC006",
        summary: "keys and auth secrets never enter ordinary output",
        regression_test: "secret_redaction_and_serialization_exclusion",
        metric: "secret_exposure_total",
    },
    SecurityInvariant {
        id: "AEQ-INV-SEC007",
        summary: "authority rollback fails closed",
        regression_test: "authority_epoch_rollback",
        metric: "authority_rollback_total",
    },
    SecurityInvariant {
        id: "AEQ-INV-SEC008",
        summary: "irreversible side effects reconcile idempotently",
        regression_test: "side_effect_safety",
        metric: "unsafe_side_effect_total",
    },
    SecurityInvariant {
        id: "AEQ-INV-SEC009",
        summary: "admin overrides require stronger auth and audit",
        regression_test: "admin_override_policy",
        metric: "admin_override_total",
    },
    SecurityInvariant {
        id: "AEQ-INV-SEC010",
        summary: "integration inputs remain untrusted",
        regression_test: "ssrf_archive_and_provider_input",
        metric: "untrusted_input_rejected_total",
    },
];

#[derive(Debug, Error, Eq, PartialEq)]
pub enum SecurityError {
    #[error("security policy is invalid: {0}")]
    InvalidPolicy(&'static str),
    #[error("authentication evidence is invalid")]
    AuthenticationInvalid,
    #[error("authentication evidence is expired")]
    AuthenticationExpired,
    #[error("authentication assurance is insufficient")]
    InsufficientAssurance,
    #[error("device binding is required")]
    DeviceBindingRequired,
    #[error("device is revoked")]
    DeviceRevoked,
    #[error("tenant claim does not match authenticated identity")]
    TenantMismatch,
    #[error("resource was not found or is forbidden")]
    NotFoundOrForbidden,
    #[error("input exceeds a configured bound")]
    InputLimitExceeded,
    #[error("compressed input exceeds expansion bounds")]
    CompressionLimitExceeded,
    #[error("checked security arithmetic overflowed")]
    ArithmeticOverflow,
    #[error("archive contains an unsafe path")]
    UnsafeArchivePath,
    #[error("archive contains a forbidden entry type")]
    UnsafeArchiveEntry,
    #[error("archive exceeds configured bounds")]
    ArchiveLimitExceeded,
    #[error("outbound target was blocked by SSRF policy")]
    SsrfBlocked,
    #[error("operation identity was reused with different semantics")]
    PayloadMismatch,
    #[error("authority epoch rollback detected")]
    AuthorityRollback,
    #[error("side effect lacks idempotency or reconciliation")]
    UnsafeSideEffect,
    #[error("security policy weakens a mandatory safe default")]
    UnsafeDefault,
}

impl SecurityError {
    #[must_use]
    pub const fn code(&self) -> SecurityErrorCode {
        match self {
            Self::AuthenticationInvalid => SecurityErrorCode::AuthInvalid,
            Self::AuthenticationExpired => SecurityErrorCode::AuthExpired,
            Self::DeviceRevoked => SecurityErrorCode::AuthRevoked,
            Self::TenantMismatch => SecurityErrorCode::TenantMismatch,
            Self::NotFoundOrForbidden
            | Self::InsufficientAssurance
            | Self::DeviceBindingRequired => SecurityErrorCode::AuthzDenied,
            Self::PayloadMismatch => SecurityErrorCode::PayloadMismatch,
            Self::InputLimitExceeded
            | Self::CompressionLimitExceeded
            | Self::ArithmeticOverflow => SecurityErrorCode::InputLimitExceeded,
            Self::UnsafeArchivePath | Self::UnsafeArchiveEntry | Self::ArchiveLimitExceeded => {
                SecurityErrorCode::UnsafeArchive
            }
            Self::SsrfBlocked => SecurityErrorCode::SsrfBlocked,
            Self::AuthorityRollback => SecurityErrorCode::AuthorityRollback,
            Self::UnsafeSideEffect => SecurityErrorCode::UnsafeSideEffect,
            Self::InvalidPolicy(_) | Self::UnsafeDefault => SecurityErrorCode::Internal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv6Addr};

    fn evidence() -> AuthenticationEvidence {
        AuthenticationEvidence {
            principal_id: ActorId::new(),
            tenant_id: TenantId::new(),
            device_id: Some(DeviceId::new()),
            auth_method: AuthMethod::BearerToken,
            assurance: AssuranceLevel::MultiFactor,
            issuer: "https://identity.example".to_owned(),
            audience: "aequora".to_owned(),
            authenticated_at_unix_ms: 100,
            expires_at_unix_ms: 1_000,
            device_state: Some(DeviceState::Active),
        }
    }

    #[test]
    fn safe_profiles_validate() {
        assert!(
            SecurityPolicy::standard("issuer", "audience")
                .validate()
                .is_ok()
        );
        assert!(
            SecurityPolicy::enterprise("issuer", "audience")
                .validate()
                .is_ok()
        );
        assert!(
            SecurityPolicy::high_assurance("issuer", "audience")
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn authentication_and_tenant_binding_fail_closed() {
        let policy = SecurityPolicy::standard("https://identity.example", "aequora");
        let context = policy
            .auth
            .authenticate(evidence(), 200)
            .unwrap_or_else(|error| panic!("unexpected: {error}"));
        assert_eq!(
            context.bind_tenant(TenantId::new()),
            Err(SecurityError::TenantMismatch)
        );

        let mut revoked = evidence();
        revoked.device_state = Some(DeviceState::Revoked);
        assert_eq!(
            policy.auth.authenticate(revoked, 200),
            Err(SecurityError::DeviceRevoked)
        );
    }

    #[test]
    fn operation_payload_substitution_is_rejected() {
        let auth = evidence();
        let operation_id = OperationId::new();
        let first = OperationBinding::new(
            operation_id,
            auth.tenant_id,
            auth.principal_id,
            auth.device_id,
            b"first",
        );
        let changed = OperationBinding::new(
            operation_id,
            auth.tenant_id,
            auth.principal_id,
            auth.device_id,
            b"changed",
        );
        let mut guard =
            OperationReplayGuard::new(2).unwrap_or_else(|error| panic!("unexpected: {error}"));
        assert_eq!(guard.observe(first), Ok(ReplayDisposition::FirstSeen));
        assert_eq!(guard.observe(first), Ok(ReplayDisposition::Duplicate));
        assert_eq!(guard.observe(changed), Err(SecurityError::PayloadMismatch));
    }

    #[test]
    fn boundary_plus_one_and_compression_bomb_are_rejected() {
        let limits = SecurityPolicy::standard("issuer", "audience").protocol;
        let shape = InputShape {
            frame_bytes: limits.max_frame_bytes + 1,
            ..InputShape::default()
        };
        assert_eq!(
            limits.validate_input(shape),
            Err(SecurityError::InputLimitExceeded)
        );
        assert_eq!(
            limits.validate_compression(1, limits.max_compression_ratio + 1),
            Err(SecurityError::CompressionLimitExceeded)
        );
    }

    #[test]
    fn ssrf_blocks_private_metadata_redirect_and_rebinding() {
        let policy = SecurityPolicy::standard("issuer", "audience").egress;
        for address in [
            IpAddr::from([127, 0, 0, 1]),
            IpAddr::from([10, 0, 0, 1]),
            IpAddr::from([172, 16, 0, 1]),
            IpAddr::from([192, 168, 0, 1]),
            IpAddr::from([169, 254, 169, 254]),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            "fc00::1"
                .parse::<IpAddr>()
                .unwrap_or(IpAddr::V6(Ipv6Addr::LOCALHOST)),
            "::ffff:127.0.0.1"
                .parse::<IpAddr>()
                .unwrap_or(IpAddr::V6(Ipv6Addr::LOCALHOST)),
        ] {
            assert_eq!(
                policy.validate_target("https://example.test/hook", &[address], 0),
                Err(SecurityError::SsrfBlocked)
            );
        }
        let public = IpAddr::from([93, 184, 216, 34]);
        let target = policy
            .validate_target("https://example.com/hook", &[public], 0)
            .unwrap_or_else(|error| panic!("unexpected: {error}"));
        assert_eq!(
            target.revalidate_connection(policy, IpAddr::from([127, 0, 0, 1])),
            Err(SecurityError::SsrfBlocked)
        );
        assert_eq!(
            policy.validate_target("https://example.com/next", &[public], 1),
            Err(SecurityError::SsrfBlocked)
        );
    }

    #[test]
    fn archive_traversal_links_and_bombs_are_rejected() {
        let limits = SecurityPolicy::standard("issuer", "audience").uploads;
        let traversal = ArchiveEntry {
            relative_path: "../../etc/passwd".to_owned(),
            kind: ArchiveEntryKind::RegularFile,
            compressed_bytes: 1,
            expanded_bytes: 1,
        };
        assert_eq!(
            limits.validate_archive(&[traversal]),
            Err(SecurityError::UnsafeArchivePath)
        );
        let link = ArchiveEntry {
            relative_path: "link".to_owned(),
            kind: ArchiveEntryKind::SymbolicLink,
            compressed_bytes: 1,
            expanded_bytes: 1,
        };
        assert_eq!(
            limits.validate_archive(&[link]),
            Err(SecurityError::UnsafeArchiveEntry)
        );
        let bomb = ArchiveEntry {
            relative_path: "bomb.bin".to_owned(),
            kind: ArchiveEntryKind::RegularFile,
            compressed_bytes: 1,
            expanded_bytes: limits.max_archive_ratio + 1,
        };
        assert_eq!(
            limits.validate_archive(&[bomb]),
            Err(SecurityError::ArchiveLimitExceeded)
        );
    }

    #[test]
    fn secret_output_is_always_redacted() {
        let secret = SecretString::new("bearer-super-secret".to_owned());
        assert_eq!(format!("{secret}"), "[REDACTED]");
        assert_eq!(format!("{secret:?}"), "SecretString([REDACTED])");
        assert_eq!(secret.expose_secret(), "bearer-super-secret");
    }

    #[test]
    fn authority_and_side_effect_guards_fail_closed() {
        let epoch_two = AuthorityEpoch::new(2).unwrap_or(AuthorityEpoch::INITIAL);
        let mut guard = AuthorityEpochGuard::new(epoch_two);
        assert_eq!(
            guard.observe(AuthorityEpoch::INITIAL),
            Err(SecurityError::AuthorityRollback)
        );
        let unsafe_effect = SideEffectSafety {
            risk: SideEffectRisk::Financial,
            idempotency_key: None,
            reconciliation_kind: None,
        };
        assert_eq!(
            unsafe_effect.validate(),
            Err(SecurityError::UnsafeSideEffect)
        );
    }

    #[test]
    fn invariant_registry_is_unique_and_complete() {
        let ids = SECURITY_INVARIANTS
            .iter()
            .map(|entry| entry.id)
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), 10);
        assert_eq!(SECURITY_ASSETS.len(), 11);
        assert_eq!(ATTACKER_CLASSES.len(), 10);
        assert_eq!(TRUST_BOUNDARIES.len(), 10);
    }
}
