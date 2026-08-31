// Library crate — all modules are public so integration tests can import them.
#[cfg(not(target_arch = "wasm32"))]
pub mod batch;
#[cfg(feature = "c2pa")]
pub mod c2pa;
#[cfg(feature = "c2pa")]
pub mod c2pa_cli;
pub mod cli;
#[cfg(not(target_arch = "wasm32"))]
pub mod conformance;
pub mod dct;
pub mod dwt;
pub mod dwt_embed;
pub mod embed;
pub mod extract;
#[cfg(not(target_arch = "wasm32"))]
pub mod info;
#[cfg(feature = "learned")]
pub mod learned;
#[cfg(not(target_arch = "wasm32"))]
pub mod pointer;
#[cfg(not(target_arch = "wasm32"))]
pub mod strip;
pub mod verify;
pub mod wasm_api;
// ── CTX-0018: carrier trait + core group ──
pub mod carrier;
pub mod core;
// ── CTX-0022: capglyph-core extraction (re-exports) ──
pub use capglyph_core::ecc;
pub use capglyph_core::framing;
pub use capglyph_core::geometry;
pub use capglyph_core::interleave;
pub use capglyph_core::keying;
pub use capglyph_core::placement;
pub use capglyph_core::registration;
pub use capglyph_core::signal;
pub use capglyph_core::spread_spectrum;
// Compat aliases for `sigil_core` import path (kept for downstream code)
pub use capglyph_core as sigil_core;
