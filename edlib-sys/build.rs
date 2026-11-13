fn main() {
    let header = "edlib/edlib/include/edlib.h";
    let source = "edlib/edlib/src/edlib.cpp";
    let shim_header = "src/shim.h";
    let shim_source = "src/shim.cpp";
    assert!(
        std::path::Path::new(header).exists(),
        "edlib submodule is missing; run git submodule update --init --recursive"
    );
    for path in [header, source, shim_header, shim_source] {
        println!("cargo:rerun-if-changed={path}");
    }

    // Edlib is one translation unit; cc also selects the target C++ runtime.
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++14")
        .include("edlib/edlib/include")
        .file(source)
        .file(shim_source)
        .opt_level(3);
    if build.get_compiler().is_like_msvc() {
        // Enable C++ unwinding, but explicitly disable /EHc: edlibAlign is
        // extern "C" and can throw. Otherwise the shim's noexcept termination
        // check may be omitted. /EHsc- means /EHs with /EHc disabled.
        build.flag("/EHsc-");
    }
    build.compile("edlib");

    #[cfg(feature = "bindgen")]
    generate_bindings(shim_header);
}

/// Regenerates the bindings that are otherwise read from `src/bindings.rs`.
#[cfg(feature = "bindgen")]
fn generate_bindings(header: &str) {
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    bindgen::Builder::default()
        .header(header)
        .clang_arg("-Iedlib/edlib/include")
        .rust_target(bindgen::RustTarget::stable(85, 1).unwrap())
        .rust_edition(bindgen::RustEdition::Edition2024)
        .allowlist_function("edlib.*")
        .allowlist_type("Edlib.*")
        .allowlist_var("EDLIB_.*")
        // These can throw std::bad_alloc; C-unwind makes such an unwind defined.
        .override_abi(bindgen::Abi::CUnwind, "edlibAlign|edlibAlignmentToCigar")
        // Size assertions would pin the committed file to one pointer width.
        .layout_tests(false)
        // Otherwise C-unwind overrides get a host-specific `link_name` (`_edlibAlign` on macOS).
        .trust_clang_mangling(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate edlib bindings; install libclang")
        .write_to_file(out.join("bindings.rs"))
        .expect("Unable to write edlib bindings");
}
