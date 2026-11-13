//! Raw bindings to the bundled edlib C interface.
//!
//! Callers own native allocations: free alignment results with
//! `edlibFreeAlignResult` and CIGAR strings with the C allocator's `free`.
//!
//! Edlib's C++ code can throw `std::bad_alloc`. `edlibAlign` and
//! `edlibAlignmentToCigar` are declared `C-unwind`, so such an unwind is
//! defined behavior; `edlibAlignNoexcept` terminates the process instead.
//!
//! The bindings are committed in `src/bindings.rs`, so builds need no libclang.
//! After changing the headers, refresh them with
//! `UPDATE_BINDINGS=1 cargo test -p edlib-sys --features bindgen`.

#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]
#![allow(clippy::missing_safety_doc)]

#[cfg(feature = "bindgen")]
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
#[cfg(not(feature = "bindgen"))]
include!("bindings.rs");

#[cfg(all(test, feature = "bindgen"))]
mod tests {
    #[test]
    fn committed_bindings_match_generated() {
        let generated = include_str!(concat!(env!("OUT_DIR"), "/bindings.rs"));
        let committed = concat!(env!("CARGO_MANIFEST_DIR"), "/src/bindings.rs");
        if std::env::var_os("UPDATE_BINDINGS").is_some() {
            std::fs::write(committed, generated).unwrap();
        }
        assert!(
            std::fs::read_to_string(committed).unwrap() == generated,
            "src/bindings.rs is stale; rerun with UPDATE_BINDINGS=1. \
             Enum aliases are c_int on MSVC, so regenerate on a non-Windows host."
        );
    }
}
