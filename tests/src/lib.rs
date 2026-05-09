//! Shared utilities for MVM integration tests.

use std::path::{Path, PathBuf};

const FIXTURES_DIR: &str = "../fixtures";

pub fn fixture_path(name: &str) -> PathBuf {
    Path::new(FIXTURES_DIR).join(name)
}

pub fn wat_from_file(name: &str) -> Vec<u8> {
    let path = fixture_path(name);
    let wat =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    wat::parse_str(&wat).unwrap_or_else(|e| panic!("assemble {}: {e}", path.display()))
}

pub fn wat_from_str(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("WAT assembles to WASM")
}
