# edlib-rs

Rust bindings to [edlib](https://github.com/tmokveld/edlib)

- **`edlib-rs`**: safe byte-oriented alignment, owned results, and CIGAR formatting.
- **`edlib-sys`**: raw bindings and the bundled C++ build.

Supports global, prefix, and infix alignment; distance limits; custom byte
equalities; and alignment paths. Results are `Send + Sync` and free their native
buffers on drop. Use the Git dependency below; the crates are not published to crates.io.

## Build

Requires Rust **1.85.1+** and a **C++14 compiler**. Default builds do not need
libclang, CMake, or a system edlib installation.

```sh
git clone --recurse-submodules https://github.com/tmokveld/edlib-rs.git
cd edlib-rs
cargo test --workspace
```

## Usage

Add the Git dependency to your `Cargo.toml`:

```toml
[dependencies]
edlib-rs = { git = "https://github.com/tmokveld/edlib-rs.git" }
```

Cargo fetches the edlib submodule automatically. For reproducible builds, add
`rev = "<commit-sha>"` to pin a specific revision.

```rust
use edlib_rs::aligner::{align, AlignConfig, AlignmentTask, CigarFormat};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AlignConfig {
        task: AlignmentTask::Path,
        ..Default::default()
    };
    let result = align(b"ACGT", b"AGGT", &config)?;

    assert_eq!(result.edit_distance(), Some(1));
    assert_eq!(result.cigar(CigarFormat::Extended).unwrap().to_string(), "1=1X2=");
    Ok(())
}
```

Defaults are global alignment, distance and end locations, and no distance limit.
`edit_distance()` returns `None` when the configured limit is exceeded.
`locations()` exposes half-open target spans when start locations were requested.
In infix mode, an empty query matches every target boundary. A requested
zero-length path is returned as `Some(&[])` and formats as an empty CIGAR string.
Raw bindings are available through `edlib_sys` or `edlib_rs::edlib`.

## Regenerating bindings

Bindings are committed, so libclang is only required with the optional `bindgen`
feature. Regenerate on a **non-Windows host**, since MSVC uses different enum
signedness:

```sh
UPDATE_BINDINGS=1 cargo test -p edlib-sys --features bindgen
```

Without `UPDATE_BINDINGS`, that command checks for drift without updating the
committed file. Run `cargo doc --open` for the Rust interface documentation.
