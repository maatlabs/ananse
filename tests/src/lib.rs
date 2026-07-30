//! Shared utilities for Ananse integration tests.

use std::path::{Path, PathBuf};

use ananse_air::{
    AnanseAir, NUM_CHALLENGES, QuadExt, build_permutation_trace, program_data, program_rom,
};
use ananse_decoder::{
    Felt, Module, Word, function_constants, function_opcodes, global_initializers,
};
use ananse_executor::{Entry, execute};
use ananse_lift::lift;
use ananse_trace::Trace;
use ananse_wasi::WasiSnapshotPreview1;
use p3_air::{Air, BaseAir, DebugConstraintBuilder};
use p3_field::PrimeCharacteristicRing;
use p3_matrix::Matrix;
use p3_matrix::dense::{RowMajorMatrix, RowMajorMatrixView};
use p3_matrix::stack::ViewPair;
use wasmparser::WasmFeatures;

/// The curated showcase programs under `examples/` --- production-grade,
/// ZK-themed integer workloads exercised end to end by the executor suite.
pub const EXAMPLE_FILES: &[&str] = &[
    "fibonacci.wat",
    "factorial.wat",
    "gcd.wat",
    "fnv1a.wat",
    "modpow.wat",
    "crc32.wat",
    "merkle_path.wat",
];

pub const EXAMPLES_DIR: &str = "../examples";

/// The opcode-family test fixtures under `tests/fixtures/` --- small programs
/// each named for the operator family it exercises, consumed by the decoder,
/// lift, executor, trace, and AIR suites.
pub const FIXTURE_FILES: &[&str] = &[
    "bitwise.wat",
    "branch.wat",
    "cmp.wat",
    "const.wat",
    "conv.wat",
    "ext_s.wat",
    "func_add.wat",
    "func_call.wat",
    "func_local.wat",
    "func_lts.wat",
    "func_sub.wat",
    "global.wat",
    "hello_world.wat",
    "i32_const.wat",
    "i32_store.wat",
    "local_set.wat",
    "local_tee.wat",
    "memory.wat",
    "select.wat",
];

pub const SINGLE_FRAME_FIXTURES: &[&str] = &[
    "i32_const.wat",
    "func_add.wat",
    "func_sub.wat",
    "func_lts.wat",
    "func_local.wat",
    "local_set.wat",
    "local_tee.wat",
    "global.wat",
    "conv.wat",
    "cmp.wat",
    "ext_s.wat",
    "bitwise.wat",
    "select.wat",
    "branch.wat",
    "i32_store.wat",
];

pub const FIXTURE_DIR: &str = "fixtures";

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

pub fn trace_of(bytes: &[u8]) -> Trace {
    let module = Module::decode(bytes).expect("decode");
    let program = lift(&module).expect("lift");
    let globals = global_initializers(&module).expect("globals");
    let mut host = WasiSnapshotPreview1::new();
    let mut records = Vec::new();
    execute(&module, &Entry::Auto, &[], &mut host, &mut records).expect("execute");
    Trace::build(&program, records, &[], &globals).expect("build")
}

pub fn trace_and_rom(bytes: &[u8]) -> (Trace, Vec<Felt>, Vec<(u32, u32, u32)>) {
    trace_and_rom_entry(bytes, &Entry::Auto, &[])
}

pub fn trace_and_rom_entry(
    bytes: &[u8],
    entry: &Entry,
    args: &[Word],
) -> (Trace, Vec<Felt>, Vec<(u32, u32, u32)>) {
    let module = Module::decode(bytes).expect("decode");
    let program = lift(&module).expect("lift");
    let globals = global_initializers(&module).expect("globals");
    let mut host = WasiSnapshotPreview1::new();
    let mut records = Vec::new();
    execute(&module, entry, args, &mut host, &mut records).expect("execute");
    let func_index = records
        .first()
        .expect("execution produces records")
        .func_index;
    let function = program
        .functions
        .iter()
        .find(|f| f.func_index == func_index)
        .expect("executed function was lifted");
    let opcodes = function_opcodes(&module, func_index).expect("opcodes");
    let rom = program_rom(&opcodes, function).expect("program ROM");
    let constants = function_constants(&module, func_index).expect("constants");
    let data = program_data(&constants, function).expect("program data");
    let trace = Trace::build(&program, records, args, &globals).expect("build");
    (trace, rom, data)
}

pub fn air_for(trace: &Trace, rom: &[Felt], data: &[(u32, u32, u32)]) -> AnanseAir {
    AnanseAir::new(
        rom,
        data,
        trace.length(),
        trace.stack_base(),
        trace.initial_state(),
    )
}

pub fn main_matrix(columns: &[Vec<Felt>], length: usize) -> RowMajorMatrix<Felt> {
    let width = columns.len();
    let values = (0..length)
        .flat_map(|row| columns.iter().map(move |column| column[row]))
        .collect();
    RowMajorMatrix::new(values, width)
}

pub fn permutation_of(
    main: &RowMajorMatrix<Felt>,
    rom: &[Felt],
    data: &[(u32, u32, u32)],
    initial: &[(u64, Felt, Felt)],
    challenges: [QuadExt; NUM_CHALLENGES],
) -> RowMajorMatrix<QuadExt> {
    build_permutation_trace(main, rom, data, initial, challenges).expect("permutation trace")
}

/// Evaluates every AIR constraint on each row of `main` paired with the permutation
/// trace `perm` under the permutation `challenges`, returning the indices of rows
/// carrying at least one violation.
pub fn failing_rows(
    air: &AnanseAir,
    main: &RowMajorMatrix<Felt>,
    perm: &RowMajorMatrix<QuadExt>,
    challenges: [QuadExt; NUM_CHALLENGES],
) -> Vec<usize> {
    let height = main.height();
    let main_width = main.width();
    let perm_width = perm.width();
    (0..height)
        .filter(|&row| {
            let next = (row + 1) % height;
            let main_pair = ViewPair::new(
                RowMajorMatrixView::new_row(&main.values[row * main_width..(row + 1) * main_width]),
                RowMajorMatrixView::new_row(
                    &main.values[next * main_width..(next + 1) * main_width],
                ),
            );
            let perm_pair = ViewPair::new(
                RowMajorMatrixView::new_row(&perm.values[row * perm_width..(row + 1) * perm_width]),
                RowMajorMatrixView::new_row(
                    &perm.values[next * perm_width..(next + 1) * perm_width],
                ),
            );
            let preprocessed = ViewPair::new(
                RowMajorMatrixView::new(&[][..], 0),
                RowMajorMatrixView::new(&[][..], 0),
            );
            let periodic = air.periodic_values(row);
            let mut builder = DebugConstraintBuilder::new_with_permutation(
                row,
                main_pair,
                preprocessed,
                &[],
                Felt::from_bool(row == 0),
                Felt::from_bool(row == height - 1),
                Felt::from_bool(row != height - 1),
                perm_pair,
                &challenges,
                &[],
                &periodic,
            );
            air.eval(&mut builder);
            builder.has_failures()
        })
        .collect()
}

pub fn wasm_features() -> WasmFeatures {
    let mut f = WasmFeatures::empty();
    f.insert(WasmFeatures::MUTABLE_GLOBAL);
    f
}

pub fn example_path(name: &str) -> PathBuf {
    Path::new(EXAMPLES_DIR).join(name)
}

pub fn fixture_path(name: &str) -> PathBuf {
    Path::new(FIXTURE_DIR).join(name)
}

fn wat_from_path(path: &Path) -> Vec<u8> {
    let wat =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    wat::parse_str(&wat).unwrap_or_else(|e| panic!("assemble {}: {e}", path.display()))
}

/// Loads and assembles a showcase program from `examples/`.
pub fn wat_from_example(name: &str) -> Vec<u8> {
    wat_from_path(&example_path(name))
}

/// Loads and assembles a test fixture from `tests/fixtures/`.
pub fn wat_from_fixture(name: &str) -> Vec<u8> {
    wat_from_path(&fixture_path(name))
}

pub fn wat_from_str(wat: &str) -> Vec<u8> {
    wat::parse_str(wat).expect("WAT assembles to WASM")
}

/// Fixed stand-ins for the Fiat--Shamir permutation challenges the prover draws,
/// letting the control-flow lookup, consistency permutation, range-check byte-table
/// lookup, boundary lookup, bitwise AND-table lookup, popcount lookup, and pc-keyed
/// data-ROM lookup be exercised without a prover, in [`NUM_CHALLENGES`] order: the
/// control-flow folding challenge, the consistency permutation's denominator and
/// access-folding challenges, the byte-table challenge, the boundary lookup's
/// denominator, the AND-table's tuple-fold and denominator challenges, the popcount
/// table's pair-fold and denominator challenges, and the data-ROM's tuple-fold and
/// denominator challenges.
pub fn mock_challenges() -> [QuadExt; NUM_CHALLENGES] {
    [
        QuadExt::from(Felt::new(0x9e37_79b9_7f4a_7c15)),
        QuadExt::from(Felt::new(0xff51_afd7_ed55_8ccd)),
        QuadExt::from(Felt::new(0xc4ce_b9fe_1a85_ec53)),
        QuadExt::from(Felt::new(0xbf58_476d_1ce4_e5b9)),
        QuadExt::from(Felt::new(0x94d0_49bb_1331_11eb)),
        QuadExt::from(Felt::new(0x2545_f491_4f6c_dd1d)),
        QuadExt::from(Felt::new(0x1656_67b1_9e37_79f9)),
        QuadExt::from(Felt::new(0x6a09_e667_f3bc_c908)),
        QuadExt::from(Felt::new(0xb056_88c2_b3e6_c1f7)),
        QuadExt::from(Felt::new(0x3c6e_f372_fe94_f82b)),
        QuadExt::from(Felt::new(0xa54f_f53a_5f1d_36f1)),
    ]
}
