use aequora_benchkit::{
    BenchmarkError, BenchmarkTarget, CapacityEvidence, CapacityInputs, EvidenceClass,
    ProductionTargetGuard, estimate_capacity,
};
use aequora_loadgen::{AdmissionOutcome, LoadGenerator};
use aequora_workload::WorkloadSpec;

const WORKLOADS: [(&str, &str); 8] = [
    (
        "generic_small",
        include_str!("../../../workloads/part47/generic_small.ron"),
    ),
    (
        "generic_medium",
        include_str!("../../../workloads/part47/generic_medium.ron"),
    ),
    (
        "school_standard",
        include_str!("../../../workloads/part47/school_standard.ron"),
    ),
    (
        "finance_append_only",
        include_str!("../../../workloads/part47/finance_append_only.ron"),
    ),
    (
        "mobile_offline",
        include_str!("../../../workloads/part47/mobile_offline.ron"),
    ),
    (
        "reconnect_storm",
        include_str!("../../../workloads/part47/reconnect_storm.ron"),
    ),
    (
        "hot_tenant",
        include_str!("../../../workloads/part47/hot_tenant.ron"),
    ),
    (
        "bootstrap_large",
        include_str!("../../../workloads/part47/bootstrap_large.ron"),
    ),
];

#[test]
fn recommended_workloads_are_parseable_bounded_and_uniquely_fingerprinted()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fingerprints = std::collections::BTreeSet::new();
    for (_name, source) in WORKLOADS {
        let workload: WorkloadSpec = ron::from_str(source)?;
        workload.validate()?;
        assert!(fingerprints.insert(workload.fingerprint()?));
    }
    Ok(())
}

#[test]
fn overload_is_rejected_before_queue_growth() -> Result<(), Box<dyn std::error::Error>> {
    let mut workload: WorkloadSpec = ron::from_str(WORKLOADS[0].1)?;
    workload.maximum_queue_depth = 2;
    let mut load = LoadGenerator::new(workload)?;
    assert_eq!(load.offer(), AdmissionOutcome::Admitted);
    assert_eq!(load.offer(), AdmissionOutcome::Admitted);
    for _ in 0..10_000 {
        assert_eq!(load.offer(), AdmissionOutcome::RejectedAtCapacity);
    }
    assert_eq!(load.queue_depth(), 2);
    assert_eq!(load.counters().rejected, 10_000);
    Ok(())
}

#[test]
fn unknown_capacity_stays_uncertified_and_has_no_invented_size()
-> Result<(), Box<dyn std::error::Error>> {
    let workload: WorkloadSpec = ron::from_str(WORKLOADS[2].1)?;
    let estimate = estimate_capacity(
        &workload,
        &CapacityInputs {
            active_tenants: 100,
            active_clients: 25_000,
            operations_per_client_hour: 12,
            peak_multiplier_basis_points: 60_000,
            batch_size: 32,
            safety_headroom_basis_points: 7_000,
            api_node_count: 2,
            survive_one_node_loss: true,
            journal_bytes_per_operation: 0,
            ledger_bytes_per_operation: 0,
            audit_bytes_per_operation: 0,
        },
        &CapacityEvidence {
            classification: EvidenceClass::Unknown,
            sustainable_goodput_per_second: None,
            tested_peak_operations_per_second: None,
            source_benchmark: None,
            evidence_bundle: None,
        },
    )?;
    assert_eq!(estimate.classification, EvidenceClass::Unknown);
    assert!(estimate.safe_capacity_per_node.is_none());
    assert!(estimate.required_api_nodes.is_none());
    assert!(!estimate.certified);
    Ok(())
}

#[test]
fn accidental_production_load_fails_closed() {
    let guard = ProductionTargetGuard {
        target: BenchmarkTarget::Production,
        safeguards: std::collections::BTreeSet::new(),
        rate_cap_operations_per_second: None,
    };
    assert_eq!(
        guard.authorize(),
        Err(BenchmarkError::ProductionTargetDenied)
    );
}
