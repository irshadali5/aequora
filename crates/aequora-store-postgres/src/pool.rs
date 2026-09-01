//! Pool admission helpers.

/// Rejects work before pool acquisition when no connection may ever be opened.
#[must_use]
pub const fn has_capacity(max_connections: u32) -> bool {
    max_connections > 0
}
