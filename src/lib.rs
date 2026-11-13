//! Safe, byte-oriented sequence alignment using bundled edlib.
//!
//! See [`aligner::align`] for the safe interface. Native result memory is owned
//! by [`aligner::Alignment`] and released on drop. Results are `Send + Sync`.
//!
//! # Usage
//!
//! Defaults are global alignment, distance and end locations, and no distance limit.
//! Set [`aligner::AlignConfig::task`] to request start locations or an alignment path.
//!
//! Add this Git dependency to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! edlib-rs = { git = "https://github.com/tmokveld/edlib-rs.git" }
//! ```
//!
//! Cargo fetches the edlib submodule automatically. Add `rev = "<commit-sha>"`
//! to pin a specific revision.
//!
//! See the [README](https://github.com/tmokveld/edlib-rs#build) for build and
//! binding-generation instructions.

pub mod aligner;

/// Raw edlib bindings. Callers must uphold the native ownership contracts.
pub mod edlib {
    pub use edlib_sys::*;
}
