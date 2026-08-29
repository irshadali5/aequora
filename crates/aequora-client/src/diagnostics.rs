//! Bounded, durable-snapshot-friendly client diagnostic integration.

use aequora_diagnostics::{
    DiagnosticError, DiagnosticEvent, DiagnosticRing, DiagnosticRingLimits, RuntimeInventory,
};

#[derive(Clone, Debug)]
pub struct ClientDiagnosticSnapshot {
    pub inventory: RuntimeInventory,
    pub events: Vec<DiagnosticEvent>,
}

#[derive(Clone, Debug)]
pub struct ClientDiagnostics {
    inventory: RuntimeInventory,
    ring: DiagnosticRing,
}

impl ClientDiagnostics {
    /// Creates a client diagnostic ring after validating its inventory and hard bounds.
    ///
    /// # Errors
    ///
    /// Returns an inventory or limit error for an unsafe configuration.
    pub fn new(
        inventory: RuntimeInventory,
        limits: DiagnosticRingLimits,
    ) -> Result<Self, DiagnosticError> {
        inventory.validate()?;
        Ok(Self {
            inventory,
            ring: DiagnosticRing::new(limits)?,
        })
    }

    /// Adds one structured event while evicting older evidence within the configured bounds.
    ///
    /// # Errors
    ///
    /// Returns a validation or size error when the event itself is unsafe to retain.
    pub fn record(&mut self, event: DiagnosticEvent) -> Result<(), DiagnosticError> {
        self.ring.push(event)
    }

    #[must_use]
    pub fn snapshot(&self) -> ClientDiagnosticSnapshot {
        ClientDiagnosticSnapshot {
            inventory: self.inventory.clone(),
            events: self.ring.snapshot(),
        }
    }
}
