//! Shared utilities for Ananse integration tests.

use std::path::{Path, PathBuf};

use ananse_air::{AnanseAir, Ext, NUM_CHALLENGES, build_permutation_trace, program_rom};
use ananse_decoder::{ImportEntry, Module};
use ananse_executor::{
    Entry, ExecuteError, Host, HostAction, Word, execute, function_opcodes, global_initializers,
};
use ananse_lift::lift;
use ananse_trace::Trace;
use p3_air::{Air, BaseAir, DebugConstraintBuilder};
use p3_field::PrimeCharacteristicRing;
use p3_goldilocks::Goldilocks as Felt;
use p3_matrix::Matrix;
use p3_matrix::dense::{RowMajorMatrix, RowMajorMatrixView};
use p3_matrix::stack::ViewPair;
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

pub const SINGLE_FRAME_FIXTURES: &[&str] = &[
    "i32_const.wat",
    "func_add.wat",
    "func_sub.wat",
    "func_lts.wat",
    "func_local.wat",
    "local_set.wat",
    "i32_store.wat",
];

pub fn trace_of(bytes: &[u8]) -> Trace {
    let module = Module::decode(bytes).expect("decode");
    let program = lift(&module).expect("lift");
    let globals = global_initializers(&module).expect("globals");
    let mut host = TestHost::default();
    let mut records = Vec::new();
    execute(&module, &Entry::Auto, &[], &mut host, &mut records).expect("execute");
    Trace::build(&program, records, &[], &globals).expect("build")
}

pub fn trace_and_rom(bytes: &[u8]) -> (Trace, Vec<Felt>) {
    let module = Module::decode(bytes).expect("decode");
    let program = lift(&module).expect("lift");
    let globals = global_initializers(&module).expect("globals");
    let mut host = TestHost::default();
    let mut records = Vec::new();
    execute(&module, &Entry::Auto, &[], &mut host, &mut records).expect("execute");
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
    let trace = Trace::build(&program, records, &[], &globals).expect("build");
    (trace, rom)
}

pub fn trace_and_rom_entry(bytes: &[u8], entry: &Entry, args: &[Word]) -> (Trace, Vec<Felt>) {
    let module = Module::decode(bytes).expect("decode");
    let program = lift(&module).expect("lift");
    let globals = global_initializers(&module).expect("globals");
    let mut host = TestHost::default();
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
    let trace = Trace::build(&program, records, args, &globals).expect("build");
    (trace, rom)
}

#[derive(Default)]
pub struct TestHost {
    pub journal: Vec<u8>,
}

impl Host for TestHost {
    fn call(
        &mut self,
        import: &ImportEntry,
        args: &[Word],
        memory: &mut [u8],
    ) -> Result<HostAction, ExecuteError> {
        match import.name.as_str() {
            "fd_write" => {
                let iovs = as_u32(args[1]) as usize;
                let count = as_u32(args[2]);
                let nwritten = as_u32(args[3]) as usize;
                let mut total: u32 = 0;
                for i in 0..count {
                    let base = iovs + (i as usize) * 8;
                    let ptr = read_u32(memory, base) as usize;
                    let len = read_u32(memory, base + 4);
                    self.journal
                        .extend_from_slice(&memory[ptr..ptr + len as usize]);
                    total += len;
                }
                write_u32(memory, nwritten, total);
                Ok(HostAction::Return(vec![Word::I32(0)]))
            }
            "proc_exit" => Ok(HostAction::Exit(as_u32(args[0]) as i32)),
            _ => Err(ExecuteError::Host {
                module: import.module.clone(),
                name: import.name.clone(),
                message: "unexpected import".into(),
            }),
        }
    }
}

fn as_u32(word: Word) -> u32 {
    match word {
        Word::I32(v) => v,
        Word::I64(v) => v as u32,
    }
}

fn read_u32(memory: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([memory[at], memory[at + 1], memory[at + 2], memory[at + 3]])
}

fn write_u32(memory: &mut [u8], at: usize, value: u32) {
    memory[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

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

pub fn air_for(trace: &Trace, rom: &[Felt]) -> AnanseAir {
    AnanseAir::new(
        rom,
        trace.length(),
        trace.stack_base(),
        trace.initial_state(),
    )
}

/// Fixed stand-ins for the Fiat--Shamir permutation challenges the prover draws,
/// letting the control-flow lookup, consistency permutation, range-check byte-table
/// lookup, and boundary lookup be exercised without a prover, in [`NUM_CHALLENGES`]
/// order: the control-flow folding challenge, the consistency permutation's
/// denominator and access-folding challenges, the byte-table challenge, and the
/// boundary lookup's denominator.
pub fn mock_challenges() -> [Ext; NUM_CHALLENGES] {
    [
        Ext::from(Felt::new(0x9e37_79b9_7f4a_7c15)),
        Ext::from(Felt::new(0xff51_afd7_ed55_8ccd)),
        Ext::from(Felt::new(0xc4ce_b9fe_1a85_ec53)),
        Ext::from(Felt::new(0xbf58_476d_1ce4_e5b9)),
        Ext::from(Felt::new(0x94d0_49bb_1331_11eb)),
    ]
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
    initial: &[(u64, Felt, Felt)],
    challenges: [Ext; NUM_CHALLENGES],
) -> RowMajorMatrix<Ext> {
    build_permutation_trace(main, rom, initial, challenges).expect("permutation trace")
}

/// Evaluates every AIR constraint on each row of `main` paired with the permutation
/// trace `perm` under the permutation `challenges`, returning the indices of rows
/// carrying at least one violation.
pub fn failing_rows(
    air: &AnanseAir,
    main: &RowMajorMatrix<Felt>,
    perm: &RowMajorMatrix<Ext>,
    challenges: [Ext; NUM_CHALLENGES],
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
