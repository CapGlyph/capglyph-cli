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
pub mod geometry;
#[cfg(not(target_arch = "wasm32"))]
pub mod info;
pub mod keying;
#[cfg(feature = "learned")]
pub mod learned;
pub mod signal;
pub mod spread_spectrum;
#[cfg(not(target_arch = "wasm32"))]
pub mod strip;
pub mod verify;
pub mod wasm_api;
// ── Refactor CTX-0018: carrier trait + core group (single crate, no new crates) ──
pub mod carrier;
pub mod core;
// ── CTX-0020: framing + ECC + interleave + soft-bits ──
pub mod ecc;
pub mod framing;
pub mod interleave;
