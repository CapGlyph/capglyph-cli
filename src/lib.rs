// Library crate — all modules are public so integration tests can import them.
#[cfg(not(target_arch = "wasm32"))]
pub mod batch;
#[cfg(feature = "c2pa")]
pub mod c2pa;
#[cfg(feature = "c2pa")]
pub mod c2pa_cli;
pub mod cli;
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
pub mod strip;
pub mod verify;
pub mod wasm_api;
// ── CTX-0018: carrier trait + core group ──
pub mod carrier;
pub mod core;
// ── CTX-0022: sigil-core extraction (re-exports) ──
pub use sigil_core::ecc;
pub use sigil_core::framing;
pub use sigil_core::geometry;
pub use sigil_core::interleave;
pub use sigil_core::keying;
pub use sigil_core::placement;
pub use sigil_core::registration;
pub use sigil_core::signal;
pub use sigil_core::spread_spectrum;
