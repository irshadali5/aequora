use aequora_config::Environment;
use aequora_deployment::{
    AuthorityBinding, AuthorityStoreKind, ConnectionBudget, ContinuityEvidence,
    DeploymentCapability, DeploymentDescriptor, DeploymentError, DeploymentId, DeploymentNode,
    EpochAction, NodeRole, OperationalMode, PublicDependency, ReadConsistency, ReadTarget,
    TimeoutBudget, TopologyKind, Version, promotion_epoch_action, route_read,
};
use aequora_types::{AuthorityEpoch, RegionId, TenantId};
use std::collections::BTreeSet;
use uuid::Uuid;

fn capabilities() -> BTreeSet<DeploymentCapability> {
    [
        DeploymentCapability::Tls,
        DeploymentCapability::Authentication,
        DeploymentCapability::Authorization,
        DeploymentCapability::LeastPrivilegeDatabaseRole,
        DeploymentCapability::SecretProvider,
        DeploymentCapability::DatabaseBackup,
        DeploymentCapability::DurableAudit,
        DeploymentCapability::Observability,
        DeploymentCapability::MigrationReadiness,
        DeploymentCapability::ImmutableRelease,
        DeploymentCapability::WorkerFencing,
    ]
    .into_iter()
    .collect()
}

fn node(name: &str, region: u16, roles: &[NodeRole]) -> DeploymentNode {
    DeploymentNode {
        node_id: name.to_owned(),
        region: RegionId::new(region),
        roles: roles.iter().copied().collect(),
        release: Version::new(1, 2, 3),
        config_generation: 7,
        config_digest: "a".repeat(64),
        registry_generation: 9,
        registry_digest: "b".repeat(64),
        minimum_protocol: 1,
        maximum_protocol: 2,
    }
}

fn global(topology: TopologyKind, nodes: Vec<DeploymentNode>) -> DeploymentDescriptor {
    DeploymentDescriptor {
        schema_version: 1,
        deployment_id: DeploymentId::from_uuid(Uuid::from_u128(42)),
        environment: Environment::Production,
        topology,
        authority_region: Some(RegionId::new(1)),
        mode: OperationalMode::Normal,
        nodes,
        authorities: vec![AuthorityBinding {
            tenant: None,
            writer_region: RegionId::new(1),
            epoch: AuthorityEpoch::INITIAL,
            store: AuthorityStoreKind::PostgreSql,
        }],
        capabilities: capabilities(),
        public_dependencies: BTreeSet::new(),
        connections: ConnectionBudget {
            api_nodes: 3,
            connections_per_api_node: 10,
            worker_connections: 4,
            operational_reserve: 6,
            database_capacity: 50,
        },
        timeouts: TimeoutBudget {
            database_ms: 1_000,
            server_ms: 2_000,
            proxy_ms: 3_000,
            client_ms: 4_000,
        },
    }
}

#[test]
fn horizontal_nodes_do_not_create_multiple_authority_timelines() {
    let descriptor = global(
        TopologyKind::SingleRegionHa,
        vec![
            node("api-a", 1, &[NodeRole::Api]),
            node("api-b", 1, &[NodeRole::Api]),
            node("api-c", 1, &[NodeRole::Api, NodeRole::Worker]),
        ],
    );
    assert!(descriptor.validate().is_ok());
    assert_eq!(descriptor.authorities.len(), 1);
    assert!(descriptor.doctor().is_ready());
}

#[test]
fn regional_reads_obey_explicit_cursor_consistency() {
    assert_eq!(
        route_read(ReadConsistency::AtLeast(11), 10),
        ReadTarget::WaitForReplica
    );
    assert_eq!(
        route_read(ReadConsistency::Session(11), 11),
        ReadTarget::RegionalReplica
    );
    assert_eq!(
        route_read(ReadConsistency::Authority, 99),
        ReadTarget::Authority
    );
}

#[test]
fn tenant_partitioning_has_one_binding_per_tenant() {
    let tenant = TenantId::from_uuid(Uuid::from_u128(7));
    let mut descriptor = global(
        TopologyKind::TenantPartitioned,
        vec![
            node("india", 1, &[NodeRole::Api]),
            node("eu", 2, &[NodeRole::Api]),
        ],
    );
    descriptor.authorities = vec![
        AuthorityBinding {
            tenant: Some(tenant),
            writer_region: RegionId::new(1),
            epoch: AuthorityEpoch::INITIAL,
            store: AuthorityStoreKind::PostgreSql,
        },
        AuthorityBinding {
            tenant: Some(tenant),
            writer_region: RegionId::new(2),
            epoch: AuthorityEpoch::INITIAL,
            store: AuthorityStoreKind::PostgreSql,
        },
    ];
    assert_eq!(
        descriptor.validate(),
        Err(DeploymentError::DuplicateAuthorityScope)
    );
}

#[test]
fn air_gap_and_restore_fail_closed() {
    let mut descriptor = global(
        TopologyKind::AirGapped,
        vec![node("internal", 1, &[NodeRole::Api])],
    );
    descriptor.connections.api_nodes = 1;
    descriptor
        .capabilities
        .insert(DeploymentCapability::OfflineReleaseVerification);
    descriptor
        .capabilities
        .insert(DeploymentCapability::InternalInfrastructure);
    descriptor
        .public_dependencies
        .insert(PublicDependency::UpdateService);
    assert_eq!(
        descriptor.validate(),
        Err(DeploymentError::PublicDependency)
    );
    assert_eq!(
        promotion_epoch_action(ContinuityEvidence::Uncertain),
        EpochAction::IncrementBeforeResume
    );
}
