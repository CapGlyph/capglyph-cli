//! Core primitives grouped for future `sigil-core` extraction.
//!
//! This module re-exports the four foundational primitives that will be moved
//! into the `sigil-core` crate in Phase 1 (DEC-0003). The re-export keeps the
//! public API backwards compatible: `crate::geometry`, `crate::signal`,
//! `crate::keying`, and `crate::spread_spectrum` remain valid via `pub mod`
//! in `lib.rs`, while new internal code should prefer `crate::core::*`.
//!
//! ```rust,ignore
//! // old (still works)
//! use sigil::geometry::GeometryFile;
//! // new (preferred inside the crate)
//! use sigil::core::geometry::GeometryFile;
//! ```
//!
//! No new crates, no behavior change — only internal boundaries. The `Carrier`
//! trait lives in `crate::carrier` (re-exported here as `crate::core::carrier`
//! for consumers that already import via `core`).

// Re-export the core modules as `crate::core::*` for future `sigil-core` move.
pub use crate::ecc;
pub use crate::framing;
pub use crate::geometry;
pub use crate::interleave;
pub use crate::keying;
pub use crate::signal;
pub use crate::spread_spectrum;

// Re-export the carrier trait so `crate::core::carrier::Carrier` and
// `crate::carrier::Carrier` are the same type. This makes the future
// `sigil-core` extraction mechanical: move `core.rs` + `carrier.rs` into the
// new crate and keep `pub use sigil_core::carrier::Carrier` in the facade.
pub use crate::carrier;
