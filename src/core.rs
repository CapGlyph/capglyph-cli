//! Core primitives grouped for future `capglyph-core` extraction.
//!
//! This module re-exports the foundational primitives that have been moved
//! into the `capglyph-core` crate (CTX-0022, legacy `sigil-core`). The re-export
//! keeps the public API backwards compatible: `crate::geometry`, `crate::signal`,
//! etc. remain valid via `pub use capglyph_core::*` in `lib.rs`, while new
//! internal code should prefer `capglyph_core::*` directly or `crate::core::*`.

pub use capglyph_core::carrier;
pub use capglyph_core::ecc;
pub use capglyph_core::framing;
pub use capglyph_core::geometry;
pub use capglyph_core::interleave;
pub use capglyph_core::keying;
pub use capglyph_core::placement;
pub use capglyph_core::registration;
pub use capglyph_core::signal;
pub use capglyph_core::spread_spectrum;
// Compat: keep `sigil_core` path working
pub use capglyph_core as sigil_core_alias;
