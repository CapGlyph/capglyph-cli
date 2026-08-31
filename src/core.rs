//! Core primitives grouped for future `sigil-core` extraction.
//!
//! This module re-exports the foundational primitives that have been moved
//! into the `sigil-core` crate (CTX-0022). The re-export keeps the public API
//! backwards compatible: `crate::geometry`, `crate::signal`, etc. remain valid
//! via `pub use sigil_core::*` in `lib.rs`, while new internal code should
//! prefer `sigil_core::*` directly or `crate::core::*`.

pub use sigil_core::carrier;
pub use sigil_core::ecc;
pub use sigil_core::framing;
pub use sigil_core::geometry;
pub use sigil_core::interleave;
pub use sigil_core::keying;
pub use sigil_core::placement;
pub use sigil_core::registration;
pub use sigil_core::signal;
pub use sigil_core::spread_spectrum;
