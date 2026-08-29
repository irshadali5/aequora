use aequora_diagnostics::{
    ClassifiedValue, DataClassification, DiagnosticError, DiagnosticMode, DiagnosticPolicy,
    DiagnosticProvider, DiagnosticRecord, DiagnosticRequest, DiagnosticSection,
    DiagnosticSectionId, DiagnosticSelector, DiagnosticValue, EvidenceConfidence, IncidentId,
    TimeRange,
};
use aequora_server::diagnostics::{DiagnosticAccessPolicy, ServerDiagnosticCollector};
use aequora_types::TenantId;
use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet};

struct EvidenceProvider;

#[async_trait]
impl DiagnosticProvider for EvidenceProvider {
    async fn diagnostics(
        &self,
        _request: &DiagnosticRequest,
        section: DiagnosticSectionId,
    ) -> Result<DiagnosticSection, DiagnosticError> {
        Ok(DiagnosticSection {
            id: section,
            schema_version: 1,
            confidence: EvidenceConfidence::Authoritative,
            records: vec![DiagnosticRecord {
                kind: "authority-state".to_owned(),
                confidence: EvidenceConfidence::Authoritative,
                fields: BTreeMap::from([(
                    "credential".to_owned(),
                    ClassifiedValue {
                        classification: DataClassification::Secret,
                        value: DiagnosticValue::Text("must-not-leave-boundary".to_owned()),
                    },
                )]),
            }],
            truncated_records: 0,
        })
    }
}

struct TenantAccess {
    tenant_id: TenantId,
}

impl DiagnosticAccessPolicy for TenantAccess {
    fn authorize_tenant(
        &self,
        principal_id: &str,
        tenant_id: TenantId,
    ) -> Result<(), DiagnosticError> {
        if principal_id == "forensic-operator" && tenant_id == self.tenant_id {
            Ok(())
        } else {
            Err(DiagnosticError::Provider("tenant access denied".to_owned()))
        }
    }
}

fn request(tenant_id: TenantId) -> DiagnosticRequest {
    DiagnosticRequest {
        incident_id: IncidentId::new(),
        tenant_id,
        selectors: vec![DiagnosticSelector::TimeWindow(TimeRange {
            start_unix_ms: 1,
            end_unix_ms: 10,
        })],
        requested_sections: BTreeSet::from([DiagnosticSectionId::Authority]),
        boundary: None,
        policy: DiagnosticPolicy {
            mode: DiagnosticMode::SupportMinimal,
            ..DiagnosticPolicy::support_minimal()
        },
    }
}

#[tokio::test]
async fn server_collection_authorizes_tenant_and_sanitizes_before_export() {
    let tenant_id = TenantId::new();
    let collector =
        ServerDiagnosticCollector::new(EvidenceProvider, TenantAccess { tenant_id }, true);
    let request = request(tenant_id);

    assert!(
        collector
            .collect_section("wrong-principal", &request, DiagnosticSectionId::Authority)
            .await
            .is_err()
    );

    let section = collector
        .collect_section(
            "forensic-operator",
            &request,
            DiagnosticSectionId::Authority,
        )
        .await
        .unwrap_or_else(|error| panic!("authorized collection failed: {error}"));
    assert_eq!(
        section.records[0].fields["credential"].value,
        DiagnosticValue::Redacted
    );
}
