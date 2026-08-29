//! Database-neutral compatibility primitives for incrementally adopting Aequora.
//!
//! Raw legacy formats terminate in reader and mapper boundaries. The core models provenance,
//! durable bridge commits, explicit aggregate ownership, side-effect-free shadowing, and verified
//! cutover; physical database adapters implement the traits in this crate.

#![allow(clippy::missing_errors_doc)]

pub mod bridge;
pub mod cutover;
pub mod id_map;
pub mod mapping;
pub mod ownership;
pub mod shadow;
pub mod system;

pub use bridge::*;
pub use cutover::*;
pub use id_map::*;
pub use mapping::*;
pub use ownership::*;
pub use shadow::*;
pub use system::*;
