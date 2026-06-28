//! Shared utilities for MVM integration tests.

use std::path::{Path, PathBuf};

use wasmparser::WasmFeatures;

pub const WAT_FILES: &[&str] = &[
    "fibonacci.wat",
    "func_add.wat",
    "func_call.wat",
    "func_local.wat",
    "func_lts.wat",
    "func_sub.wat",
    "hello_world.wat",
    "i32_const.wat",
    "i32_store.wat",
    "local_set.wat",
    "memory.wat",
];

/// Control-flow shapes the integer fixtures do not exercise: blocks with results,
/// loop back-edges, forward and multi-way branches, `if`/`else`, and dead code
/// after `return`, `br`, and `unreachable`.
pub const WAT_SNIPPETS: &[(&str, &str)] = &[
    (
        "block_result",
        "(module (func (result i32) (block (result i32) (i32.const 1))))",
    ),
    (
        "loop_br_backedge",
        "(module (func (param i32) (loop $l (br_if $l (local.get 0)))))",
    ),
    ("br_forward", "(module (func (block (br 0))))"),
    (
        "br_table",
        "(module (func (param i32) (block (block (br_table 0 1 (local.get 0))))))",
    ),
    (
        "if_else",
        "(module (func (param i32) (result i32)
            (if (result i32) (local.get 0) (then (i32.const 1)) (else (i32.const 2)))))",
    ),
    (
        "if_no_else",
        "(module (func (param i32) (if (local.get 0) (then (nop)))))",
    ),
    (
        "dead_after_return",
        "(module (func (result i32) (i32.const 1) (return) (i32.const 2) (drop) (i32.const 3)))",
    ),
    (
        "dead_after_br",
        "(module (func (block (br 0) (i32.const 7) (drop))))",
    ),
    (
        "unreachable_op",
        "(module (func (result i32) (unreachable)))",
    ),
];

pub const FIXTURES_DIR: &str = "../fixtures";

/// The decoder's restricted feature set, replicated so the validator oracle sees
/// exactly the subset the lift is built for.
pub fn wasm_features() -> WasmFeatures {
    let mut f = WasmFeatures::empty();
    f.insert(WasmFeatures::MUTABLE_GLOBAL);
    f
}

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
