use aequora_executor::AuthContext;
use aequora_invariants::InvariantId;
use aequora_security::{
    ArchiveEntry, ArchiveEntryKind, AssuranceLevel, AuthMethod, AuthenticationEvidence,
    AuthorityEpochGuard, DeviceState, OperationBinding, OperationReplayGuard, ReplayDisposition,
    ResourceKind, SECURITY_INVARIANTS, SecurityError, SecurityPolicy, SideEffectRisk,
    SideEffectSafety, TenantResource,
};
use aequora_types::{ActorId, AuthorityEpoch, DeviceId, OperationId, TenantId};
use proptest::prelude::*;
use std::net::IpAddr;

fn evidence(actor: ActorId, tenant: TenantId, device: DeviceId) -> AuthenticationEvidence {
    AuthenticationEvidence {
        principal_id: actor,
        tenant_id: tenant,
        device_id: Some(device),
        auth_method: AuthMethod::BearerToken,
        assurance: AssuranceLevel::MultiFactor,
        issuer: "https://identity.example".to_owned(),
        audience: "aequora-sync".to_owned(),
        authenticated_at_unix_ms: 1_000,
        expires_at_unix_ms: 10_000,
        device_state: Some(DeviceState::Active),
    }
}

#[test]
fn validated_security_identity_is_the_execution_bridge() {
    let actor = ActorId::new();
    let tenant = TenantId::new();
    let device = DeviceId::new();
    let policy = SecurityPolicy::standard("https://identity.example", "aequora-sync");
    let validated = policy
        .auth
        .authenticate(evidence(actor, tenant, device), 2_000)
        .unwrap_or_else(|error| panic!("authentication failed: {error}"));
    let execution = AuthContext::from_validated_security(&validated)
        .unwrap_or_else(|error| panic!("execution binding failed: {error}"));
    assert_eq!(execution.actor_id, actor);
    assert_eq!(execution.tenant_id, tenant);
    assert_eq!(execution.device_id, device);
}

#[test]
fn every_known_resource_kind_is_tenant_bound_before_disclosure() {
    let tenant = TenantId::new();
    let other = TenantId::new();
    let policy = SecurityPolicy::standard("https://identity.example", "aequora-sync");
    let context = policy
        .auth
        .authenticate(evidence(ActorId::new(), tenant, DeviceId::new()), 2_000)
        .unwrap_or_else(|error| panic!("authentication failed: {error}"));
    let binding = context
        .bind_tenant(tenant)
        .unwrap_or_else(|error| panic!("tenant binding failed: {error}"));
    for kind in [
        ResourceKind::Entity,
        ResourceKind::Scope,
        ResourceKind::Blob,
        ResourceKind::Operation,
        ResourceKind::Snapshot,
        ResourceKind::Export,
    ] {
        assert_eq!(
            binding.authorize_resource(TenantResource {
                tenant_id: other,
                kind,
            }),
            Err(SecurityError::NotFoundOrForbidden)
        );
    }
}

#[test]
fn replay_rollback_archive_ssrf_and_side_effect_abuse_fail_closed() {
    let tenant = TenantId::new();
    let actor = ActorId::new();
    let device = DeviceId::new();
    let operation = OperationId::new();
    let original = OperationBinding::new(operation, tenant, actor, Some(device), b"charge:100");
    let substituted = OperationBinding::new(operation, tenant, actor, Some(device), b"charge:900");
    let mut replay =
        OperationReplayGuard::new(8).unwrap_or_else(|error| panic!("replay guard failed: {error}"));
    assert_eq!(replay.observe(original), Ok(ReplayDisposition::FirstSeen));
    assert_eq!(
        replay.observe(substituted),
        Err(SecurityError::PayloadMismatch)
    );

    let epoch_seven = AuthorityEpoch::new(7).unwrap_or(AuthorityEpoch::INITIAL);
    let epoch_six = AuthorityEpoch::new(6).unwrap_or(AuthorityEpoch::INITIAL);
    assert_eq!(
        AuthorityEpochGuard::new(epoch_seven).observe(epoch_six),
        Err(SecurityError::AuthorityRollback)
    );

    let policy = SecurityPolicy::standard("issuer", "audience");
    assert_eq!(
        policy.egress.validate_target(
            "https://metadata.example/",
            &[IpAddr::from([169, 254, 169, 254])],
            0
        ),
        Err(SecurityError::SsrfBlocked)
    );
    assert_eq!(
        policy.uploads.validate_archive(&[ArchiveEntry {
            relative_path: "../../tenant-b/export".to_owned(),
            kind: ArchiveEntryKind::RegularFile,
            compressed_bytes: 1,
            expanded_bytes: 1,
        }]),
        Err(SecurityError::UnsafeArchivePath)
    );
    assert_eq!(
        SideEffectSafety {
            risk: SideEffectRisk::Financial,
            idempotency_key: Some("provider-request-1".to_owned()),
            reconciliation_kind: None,
        }
        .validate(),
        Err(SecurityError::UnsafeSideEffect)
    );
}

#[test]
fn security_invariant_registries_agree() {
    for (suffix, security) in (1_u16..=10).zip(SECURITY_INVARIANTS) {
        let stable = format!("AEQ-INV-SEC{suffix:03}");
        let invariant: InvariantId = stable
            .parse()
            .unwrap_or_else(|error| panic!("registry lookup failed: {error}"));
        assert_eq!(invariant.as_str(), security.id);
        assert_eq!(invariant.entry().property_test, security.regression_test);
    }
}

proptest! {
    #[test]
    fn arbitrary_distinct_tenants_never_authorize(
        authenticated in any::<u128>(),
        claimed in any::<u128>(),
    ) {
        prop_assume!(authenticated != claimed);
        let authenticated = TenantId::from_uuid(uuid::Uuid::from_u128(authenticated));
        let claimed = TenantId::from_uuid(uuid::Uuid::from_u128(claimed));
        let policy = SecurityPolicy::standard("https://identity.example", "aequora-sync");
        let context = policy.auth.authenticate(
            evidence(ActorId::new(), authenticated, DeviceId::new()),
            2_000,
        ).unwrap_or_else(|error| panic!("authentication failed: {error}"));
        prop_assert_eq!(context.bind_tenant(claimed), Err(SecurityError::TenantMismatch));
    }
}
