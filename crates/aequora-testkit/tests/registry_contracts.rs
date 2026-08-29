use aequora_admin::{AdminActionKind, AdminReasonCode, PermissionId};
use aequora_codec::MessageKind;
use aequora_compat::ids as compatibility_ids;
use aequora_registry_generated::{RegistryDomain, ids, resolve};
use aequora_types::OperationalErrorCode;

#[test]
fn wire_and_capability_constants_match_generated_registry() {
    assert_eq!(
        u32::from(MessageKind::SyncRequest as u8),
        ids::message::SYNC_REQUEST.0
    );
    assert_eq!(
        u32::from(MessageKind::TransportError as u8),
        ids::message::TRANSPORT_ERROR.0
    );
    assert_eq!(
        compatibility_ids::POSTCARD_V1.0,
        ids::capability::POSTCARD_V1.0
    );
    assert_eq!(
        compatibility_ids::AUTHORITY_EPOCH_V1.0,
        ids::capability::AUTHORITY_EPOCH_V1.0
    );
    assert_eq!(
        compatibility_ids::NEGOTIATION_V1.0,
        ids::capability::NEGOTIATION_V1.0
    );
}

#[test]
fn control_plane_ids_match_generated_registry() {
    assert_eq!(
        u32::from(PermissionId::AuthorityForcePromote as u16),
        ids::permission::AUTHORITY_FORCE_PROMOTE.0
    );
    assert_eq!(
        u32::from(PermissionId::ConsumersReset as u16),
        ids::permission::CONSUMERS_RESET.0
    );
    assert_eq!(
        u32::from(AdminActionKind::EmergencyStopWrites as u16),
        ids::admin_action::EMERGENCY_STOP_WRITES.0
    );
    assert_eq!(
        u32::from(AdminReasonCode::SecurityCompromise as u16),
        ids::reason::SECURITY_COMPROMISE.0
    );
}

#[test]
fn operational_errors_are_explicit_and_canonically_resolvable() {
    for code in [
        OperationalErrorCode::Overloaded,
        OperationalErrorCode::Authentication,
        OperationalErrorCode::PayloadLimit,
        OperationalErrorCode::Authority,
    ] {
        assert!(resolve(RegistryDomain::Error, code.registry_id()).is_some());
    }
}
