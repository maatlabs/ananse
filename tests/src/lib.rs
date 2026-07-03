//! Shared utilities for Ananse integration tests.

use std::path::{Path, PathBuf};

use ananse_air::{AnanseAir, Ext, NUM_CHALLENGES, build_permutation_trace, program_rom};
use ananse_decoder::{ImportEntry, Module};
use ananse_executor::{Entry, ExecuteError, Host, HostAction, Word, execute, function_opcodes};
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

/// Single-frame fixtures that execute from their automatic entry point and stay
/// within one call frame, performing only register and linear-memory accesses that
/// fit the value bus. `hello_world.wat` is excluded: its `fd_write` host call reads
/// four arguments and writes one result in a single operator, a variable-arity host
/// boundary handled alongside the call/return model rather than the register core.
pub const SINGLE_FRAME_FIXTURES: &[&str] = &[
    "i32_const.wat",
    "func_add.wat",
    "func_sub.wat",
    "func_lts.wat",
    "func_local.wat",
    "local_set.wat",
    "i32_store.wat",
];

/// Decodes, lifts, executes from the automatic entry point, and builds the trace.
pub fn trace_of(bytes: &[u8]) -> Trace {
    let module = Module::decode(bytes).expect("decode");
    let program = lift(&module).expect("lift");
    let mut host = TestHost::default();
    let mut records = Vec::new();
    execute(&module, &Entry::Auto, &[], &mut host, &mut records).expect("execute");
    Trace::build(&program, records).expect("build")
}

/// Decodes, lifts, executes, and builds both the trace and the packed program ROM
/// the control-flow lookup binds against---the pair the two-segment AIR needs.
pub fn trace_and_rom(bytes: &[u8]) -> (Trace, Vec<Felt>) {
    let module = Module::decode(bytes).expect("decode");
    let program = lift(&module).expect("lift");
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
    let trace = Trace::build(&program, records).expect("build");
    (trace, rom)
}

/// Decodes, lifts, executes from an explicit entry point with explicit arguments,
/// and builds both the trace and the packed program ROM. The `entry`/`args` form
/// lets a test drive concrete operand values through an opcode the automatic entry
/// point would otherwise run with zero-filled locals.
pub fn trace_and_rom_entry(bytes: &[u8], entry: &Entry, args: &[Word]) -> (Trace, Vec<Felt>) {
    let module = Module::decode(bytes).expect("decode");
    let program = lift(&module).expect("lift");
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
    let trace = Trace::build(&program, records).expect("build");
    (trace, rom)
}

/// A deterministic host realizing the two WASI imports Ananse admits: `fd_write`
/// appends each io-vector's bytes to a journal and reports the count written;
/// `proc_exit` halts with its status code.
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

/// Builds the register-shaped AIR for `trace` and its program ROM, ready for
/// row-by-row constraint evaluation.
pub fn air_for(trace: &Trace, rom: &[Felt]) -> AnanseAir {
    AnanseAir::new(rom, trace.length(), trace.stack_base())
}

/// Fixed stand-ins for the Fiat--Shamir permutation challenges the prover draws,
/// letting the control-flow lookup, consistency permutation, and range-check
/// byte-table lookup be exercised without a prover, in [`NUM_CHALLENGES`] order: the
/// control-flow folding challenge, the consistency permutation's denominator and
/// access-folding challenges, and the byte-table challenge.
pub fn mock_challenges() -> [Ext; NUM_CHALLENGES] {
    [
        Ext::from(Felt::new(0x9e37_79b9_7f4a_7c15)),
        Ext::from(Felt::new(0xff51_afd7_ed55_8ccd)),
        Ext::from(Felt::new(0xc4ce_b9fe_1a85_ec53)),
        Ext::from(Felt::new(0xbf58_476d_1ce4_e5b9)),
    ]
}

/// Packs the column-major trace into the row-major main matrix the constraint
/// evaluator reads.
pub fn main_matrix(columns: &[Vec<Felt>], length: usize) -> RowMajorMatrix<Felt> {
    let width = columns.len();
    let values = (0..length)
        .flat_map(|row| columns.iter().map(move |column| column[row]))
        .collect();
    RowMajorMatrix::new(values, width)
}

/// Builds the permutation trace binding `main`'s control flow to `rom` and its value
/// bus to its sorted access log, under the permutation `challenges`.
pub fn permutation_of(
    main: &RowMajorMatrix<Felt>,
    rom: &[Felt],
    challenges: [Ext; NUM_CHALLENGES],
) -> RowMajorMatrix<Ext> {
    build_permutation_trace(main, rom, challenges).expect("permutation trace")
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
