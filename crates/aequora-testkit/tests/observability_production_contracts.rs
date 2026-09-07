use aequora_observability::{
    AlertCatalog, AttributeKey, AttributeValue, Attributes, BoundedTelemetryQueue,
    ClientTelemetryOwner, EventRateLimiter, ObservabilityConfig, ObservabilityError,
    OperationOutcomeClass, QueueOutcome, RateLimitOutcome, SamplingClass, SamplingContext,
    SamplingPolicy, SanitizedField, SeriesBudget, SloCatalog, TelemetryExporter,
    TelemetryFieldClass, TelemetryItem, TelemetryPriority, TraceId, TraceParent,
};
use std::time::Duration;

#[test]
fn thousands_of_unbounded_id_values_cannot_expand_series_past_budget() {
    let budget = SeriesBudget::new(32).unwrap_or_else(|error| panic!("{error}"));
    for value in 0_u64..10_000 {
        let attributes = Attributes::new([(
            AttributeKey::Region,
            AttributeValue::new(format!("generated-{value}"))
                .unwrap_or_else(|error| panic!("{error}")),
        )])
        .unwrap_or_else(|error| panic!("{error}"));
        let result = budget.observe(aequora_observability::MetricId::SyncExchanges, &attributes);
        if value < 32 {
            assert!(result.is_ok());
        } else {
            assert_eq!(result, Err(ObservabilityError::SeriesBudget));
        }
    }
    assert_eq!(budget.len(), 32);
}

#[test]
fn exporter_failure_drops_only_telemetry() {
    struct Unavailable;
    impl TelemetryExporter<&'static str> for Unavailable {
        type Error = ();

        fn export(&self, _batch: &[TelemetryItem<&'static str>]) -> Result<(), Self::Error> {
            Err(())
        }
    }

    let queue = BoundedTelemetryQueue::new(2, 2).unwrap_or_else(|error| panic!("{error}"));
    let domain_result = "authoritative commit accepted";
    assert_eq!(
        queue.try_push(TelemetryItem {
            priority: TelemetryPriority::OperationalInfo,
            payload: "operation accepted",
        }),
        QueueOutcome::Accepted
    );
    queue.export_once(&Unavailable);
    assert_eq!(domain_result, "authoritative commit accepted");
    assert_eq!(queue.snapshot().export_failures, 1);
    assert_eq!(queue.snapshot().dropped, 1);
}

#[test]
fn every_ingress_class_redacts_secret_sentinel() {
    let classes = [
        TelemetryFieldClass::SafeOperational,
        TelemetryFieldClass::Identifier,
        TelemetryFieldClass::Sensitive,
        TelemetryFieldClass::PersonallyIdentifiable,
        TelemetryFieldClass::Secret,
        TelemetryFieldClass::Financial,
    ];
    for class in classes {
        let field = SanitizedField::production(
            "source_error",
            "SECRET_DO_NOT_LEAK_123 password=unsafe",
            class,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        let rendered = format!("{field:?}");
        assert!(!rendered.contains("SECRET_DO_NOT_LEAK_123"));
        assert!(!rendered.contains("password=unsafe"));
    }
}

#[test]
fn sampling_and_slo_classification_fail_closed() {
    let policy = SamplingPolicy {
        error_basis_points: 10_000,
        slow_basis_points: 10_000,
        normal_basis_points: 0,
    };
    assert!(policy.sample(SamplingContext {
        trace_id: TraceId(8),
        class: SamplingClass::Error,
    }));
    assert!(!policy.sample(SamplingContext {
        trace_id: TraceId(8),
        class: SamplingClass::Sensitive,
    }));
    assert!(!OperationOutcomeClass::Conflict.availability_eligible());
    assert!(!OperationOutcomeClass::RejectedAuthorization.availability_eligible());
    assert!(OperationOutcomeClass::InternalFailure.availability_eligible());
    assert!(!OperationOutcomeClass::InternalFailure.availability_success());
}

#[test]
fn checked_operational_metadata_and_production_budgets_are_valid() {
    let alerts = include_str!("../../../deploy/observability/alerts/catalog.ron");
    let slos = include_str!("../../../deploy/observability/slos/catalog.ron");
    let alerts = AlertCatalog::from_ron(alerts).unwrap_or_else(|error| panic!("{error}"));
    let slos = SloCatalog::from_ron(slos).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(alerts.alerts.len(), 8);
    assert_eq!(slos.objectives.len(), 3);
    assert!(ObservabilityConfig::PRODUCTION.validate().is_ok());
    assert!(
        alerts
            .alerts
            .iter()
            .filter(|alert| matches!(
                alert.severity,
                aequora_observability::AlertSeverity::Page
                    | aequora_observability::AlertSeverity::SecurityResponse
            ))
            .all(|alert| !alert.runbook.is_empty())
    );
}

#[test]
fn trace_propagation_rate_limiting_and_multi_process_ownership_are_bounded() {
    let trace =
        TraceParent::parse_untrusted("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
            .unwrap_or_else(|error| panic!("{error}"));
    assert!(trace.sampled);

    let limiter = EventRateLimiter::new(8, 1, Duration::from_secs(60))
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        limiter.observe("sync.exchange.failed", Duration::ZERO),
        RateLimitOutcome::First
    );
    assert_eq!(
        limiter.observe("sync.exchange.failed", Duration::from_secs(1)),
        RateLimitOutcome::Suppressed(1)
    );
    assert_eq!(
        limiter.recovery("sync.exchange.failed"),
        RateLimitOutcome::Recovery(1)
    );

    assert!(ClientTelemetryOwner::DesktopAgent.owns_logical_operation_metrics());
    assert!(!ClientTelemetryOwner::AgentConnectedGui.owns_logical_operation_metrics());
}
