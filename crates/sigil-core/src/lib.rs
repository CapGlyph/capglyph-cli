//! sigil-core — pure codec and carrier primitives shared with sigil-wasm and sigild.
//!
//! This crate contains no `clap`/`glob`/`tracing-subscriber`/`c2pa`/`trustmark`
//! dependencies and is `wasm32-unknown-unknown` clean (`cargo tree --target wasm32-unknown-unknown`
//! must not contain `clap`/`glob`).

pub mod carrier;
pub mod ecc;
pub mod framing;
pub mod geometry;
pub mod interleave;
pub mod keying;
pub mod placement;
pub mod registration;
pub mod signal;
pub mod spread_spectrum;

// Re-export placement at crate root for convenience.
pub use placement::Placement;
