//! Read-only server boundary for bounded incident evidence collection.

use aequora_diagnostics::{
    DefaultDiagnosticSanitizer, DiagnosticError, DiagnosticProvider, DiagnosticRequest,
    DiagnosticSanitizer, DiagnosticSection, DiagnosticSectionId,
};
use aequora_types::TenantId;

pub trait DiagnosticAccessPolicy: Send + Sync {
    /// Authorizes a forensic read for exactly one tenant.
    ///
    /// # Errors
    ///
    /// Returns a policy error when the principal cannot read the requested tenant evidence.
    fn authorize_tenant(
        &self,
        principal_id: &str,
        tenant_id: TenantId,
    ) -> Result<(), DiagnosticError>;
}

pub struct ServerDiagnosticCollector<P, A> {
    provider: P,
    access: A,
    production: bool,
}

impl<P, A> ServerDiagnosticCollector<P, A>
where
    P: DiagnosticProvider,
    A: DiagnosticAccessPolicy,
{
    #[must_use]
    pub const fn new(provider: P, access: A, production: bool) -> Self {
        Self {
            provider,
            access,
            production,
        }
    }

    /// Collects and sanitizes one section from an already bounded request.
    ///
    /// # Errors
    ///
    /// Returns an authorization, policy, provider, limit, or sanitization error.
    pub async fn collect_section(
        &self,
        principal_id: &str,
        request: &DiagnosticRequest,
        section: DiagnosticSectionId,
    ) -> Result<DiagnosticSection, DiagnosticError> {
        request.validate(self.production)?;
        self.access
            .authorize_tenant(principal_id, request.tenant_id)?;
        if !request.requested_sections.contains(&section) {
            return Err(DiagnosticError::Provider(
                "section was not included in the approved plan".to_owned(),
            ));
        }
        let collected = self.provider.diagnostics(request, section).await?;
        DefaultDiagnosticSanitizer.sanitize(collected, &request.policy)
    }
}
