//! Locally supplied inputs for tests that the public tree cannot run alone.
//!
//! Tests that call [`read`] are marked `#[ignore = "requires ..."]` and run with
//! `cargo test -- --ignored` once the named file is in place.

use std::path::Path;

pub(crate) fn read(path: &str) -> Vec<u8> {
    let full = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    std::fs::read(&full)
        .unwrap_or_else(|error| panic!("required test input {path} is unavailable: {error}"))
}
