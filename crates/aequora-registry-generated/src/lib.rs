//! Immutable, build-validated durable registry lookup tables.

pub use aequora_registry_types::{RegistryDomain, SupportStatus};

/// Compact runtime descriptor generated from canonical RON sources.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GeneratedEntry {
    pub domain: RegistryDomain,
    pub id: u32,
    pub name: &'static str,
    pub status: SupportStatus,
    pub schema_version: Option<u32>,
    pub owner: &'static str,
    pub description: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/registry.rs"));

#[must_use]
pub fn resolve(domain: RegistryDomain, id: u32) -> Option<&'static GeneratedEntry> {
    ENTRIES
        .binary_search_by_key(&(domain, id), |entry| (entry.domain, entry.id))
        .ok()
        .map(|index| &ENTRIES[index])
}

#[must_use]
pub fn resolve_name(domain: RegistryDomain, name: &str) -> Option<&'static GeneratedEntry> {
    ENTRIES
        .iter()
        .find(|entry| entry.domain == domain && entry.name == name)
}

/// Rejects unknown durable IDs and IDs whose lifecycle forbids new creation.
///
/// # Errors
///
/// Returns `Unknown` or `CreationForbidden` without guessing semantics.
pub fn admit_new(
    domain: RegistryDomain,
    id: u32,
) -> Result<&'static GeneratedEntry, AdmissionError> {
    admit_new_with_policy(domain, id, false)
}

/// Performs admission with an explicit opt-in for experimental durable history.
///
/// # Errors
///
/// Returns `Unknown` or `CreationForbidden` when policy does not admit the entry.
pub fn admit_new_with_policy(
    domain: RegistryDomain,
    id: u32,
    allow_experimental: bool,
) -> Result<&'static GeneratedEntry, AdmissionError> {
    let entry = resolve(domain, id).ok_or(AdmissionError::Unknown { domain, id })?;
    if !entry.status.allows_new_creation()
        || (entry.status == SupportStatus::Experimental && !allow_experimental)
    {
        return Err(AdmissionError::CreationForbidden {
            domain,
            id,
            status: entry.status,
        });
    }
    Ok(entry)
}

/// Resolves a historical retry without permitting a new logical effect. Removed and reserved
/// entries remain explainable but are not executable.
///
/// # Errors
///
/// Returns a fail-closed admission error for unknown or non-executable historical IDs.
pub fn admit_historical_retry(id: u32) -> Result<&'static GeneratedEntry, AdmissionError> {
    let entry = resolve(RegistryDomain::Operation, id).ok_or(AdmissionError::Unknown {
        domain: RegistryDomain::Operation,
        id,
    })?;
    if matches!(
        entry.status,
        SupportStatus::Reserved | SupportStatus::Removed
    ) {
        return Err(AdmissionError::CreationForbidden {
            domain: RegistryDomain::Operation,
            id,
            status: entry.status,
        });
    }
    Ok(entry)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionError {
    Unknown {
        domain: RegistryDomain,
        id: u32,
    },
    CreationForbidden {
        domain: RegistryDomain,
        id: u32,
        status: SupportStatus,
    },
}

/// Verifies startup handler/projector bindings against all compiled current and retry-only
/// contracts. Current consumers are required only when explicitly configured.
///
/// # Errors
///
/// Returns a fail-closed binding error for unknown handlers or missing required bindings.
pub fn validate_bindings(
    operation_handlers: &[u32],
    configured_consumer_projectors: &[u32],
) -> Result<(), BindingError> {
    for id in operation_handlers {
        if resolve(RegistryDomain::Operation, *id).is_none() {
            return Err(BindingError::UnknownHandler(*id));
        }
    }
    for entry in ENTRIES.iter().filter(|entry| {
        entry.domain == RegistryDomain::Operation
            && matches!(
                entry.status,
                SupportStatus::Current
                    | SupportStatus::Supported
                    | SupportStatus::Deprecated
                    | SupportStatus::RetryOnly
            )
    }) {
        if !operation_handlers.contains(&entry.id) {
            return Err(BindingError::MissingOperationHandler(entry.id));
        }
    }
    for id in configured_consumer_projectors {
        if resolve(RegistryDomain::Consumer, *id).is_none() {
            return Err(BindingError::UnknownConsumerProjector(*id));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingError {
    UnknownHandler(u32),
    MissingOperationHandler(u32),
    UnknownConsumerProjector(u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_registry_is_sorted_and_resolvable() {
        assert!(!ENTRIES.is_empty());
        assert!(
            ENTRIES.windows(2).all(|window| {
                (window[0].domain, window[0].id) < (window[1].domain, window[1].id)
            })
        );
        let first = ENTRIES[0];
        assert_eq!(resolve(first.domain, first.id), Some(&first));
        assert_eq!(resolve_name(first.domain, first.name), Some(&first));
    }

    #[test]
    fn unknown_ids_fail_closed() {
        assert!(matches!(
            admit_new(RegistryDomain::Operation, u32::MAX - 1),
            Err(AdmissionError::Unknown { .. })
        ));
    }

    #[test]
    fn retry_only_and_removed_lifecycles_forbid_new_creation() {
        assert!(!SupportStatus::RetryOnly.allows_new_creation());
        assert!(!SupportStatus::Removed.allows_new_creation());
        assert!(SupportStatus::RetryOnly.is_historical());
    }

    #[test]
    fn unknown_handler_fails_startup_validation() {
        assert_eq!(
            validate_bindings(&[42], &[]),
            Err(BindingError::UnknownHandler(42))
        );
    }
}
