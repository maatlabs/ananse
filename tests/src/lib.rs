//! Shared utilities for Ananse integration tests.

use std::path::{Path, PathBuf};

use ananse_air::{
    AUX_WIDTH, Air, AnanseAir, AnansePublicInputs, AuxRandElements, BatchingMethod,
    EvaluationFrame, FieldExtension, NUM_AUX_CONSTRAINTS, NUM_AUX_RANDS, ProofOptions, TraceInfo,
    build_aux_columns, periodic_table, program_rom,
};
use ananse_decoder::{ImportEntry, Module};
use ananse_executor::{Entry, ExecuteError, Host, HostAction, Word, execute, function_opcodes};
use ananse_lift::lift;
use ananse_trace::Trace;
use maat_field::{Felt, FieldElement};
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

/// Builds the two-segment register-shaped AIR for `trace` and its program ROM with
/// development-grade proof options, ready for direct constraint evaluation.
pub fn air_for(trace: &Trace, program: Vec<Felt>) -> AnanseAir {
    let trace_info = TraceInfo::new_multi_segment(
        trace.width(),
        AUX_WIDTH,
        NUM_AUX_RANDS,
        trace.length(),
        vec![],
    );
    let options = ProofOptions::new(
        27,
        8,
        0,
        FieldExtension::None,
        4,
        255,
        BatchingMethod::Algebraic,
        BatchingMethod::Algebraic,
    );
    AnanseAir::new(
        trace_info,
        AnansePublicInputs::new(program, trace.stack_base()),
        options,
    )
}

/// Builds the auxiliary LogUp columns for `main_columns` against `rom` under the
/// mock challenge `alpha`, then evaluates the auxiliary transition constraint on
/// every row, returning the rows whose residual does not vanish.
pub fn aux_violations(
    air: &AnanseAir,
    main_columns: &[Vec<Felt>],
    rom: &[Felt],
    length: usize,
    alpha: Felt,
) -> Vec<usize> {
    let aux = build_aux_columns(main_columns, rom, length, alpha).expect("aux columns");
    let table = periodic_table(rom, length);
    aux_residuals(air, main_columns, &aux, &table, length, alpha)
}

/// Evaluates the auxiliary transition constraint on every row against explicitly
/// supplied auxiliary `aux_columns` and periodic `table`, returning the rows whose
/// residual does not vanish. Pairing honest auxiliary columns with a forged main
/// trace is how a lookup tamper is caught: the edge recomputed from the main trace
/// stops matching the committed grand sum.
pub fn aux_residuals(
    air: &AnanseAir,
    main_columns: &[Vec<Felt>],
    aux_columns: &[Vec<Felt>],
    table: &[Felt],
    length: usize,
    alpha: Felt,
) -> Vec<usize> {
    let rands = AuxRandElements::new(vec![alpha]);
    (0..length.saturating_sub(1))
        .filter(|&row| {
            let main_current = main_columns.iter().map(|c| c[row]).collect::<Vec<Felt>>();
            let main_next = main_columns
                .iter()
                .map(|c| c[row + 1])
                .collect::<Vec<Felt>>();
            let aux_current = aux_columns.iter().map(|c| c[row]).collect::<Vec<Felt>>();
            let aux_next = aux_columns
                .iter()
                .map(|c| c[row + 1])
                .collect::<Vec<Felt>>();
            let main_frame = EvaluationFrame::from_rows(main_current, main_next);
            let aux_frame = EvaluationFrame::from_rows(aux_current, aux_next);
            let mut result = vec![Felt::ZERO; NUM_AUX_CONSTRAINTS];
            air.evaluate_aux_transition(
                &main_frame,
                &aux_frame,
                &[table[row]],
                &rands,
                &mut result,
            );
            result[0] != Felt::ZERO
        })
        .collect()
}

/// Evaluates every transition constraint of `air` across the column-major
/// `columns` (each of length `length`) and returns the `(row, constraint)` pairs
/// whose residual does not vanish. An empty result means the trace satisfies the
/// whole transition system; a non-empty one localizes each violation. Transitions
/// are checked on rows `0..length - 1`, matching the divisor that excludes the
/// wrap from the last row back to the first.
pub fn transition_violations(
    air: &AnanseAir,
    columns: &[Vec<Felt>],
    length: usize,
) -> Vec<(usize, usize)> {
    (0..length.saturating_sub(1))
        .flat_map(|row| {
            let current = columns
                .iter()
                .map(|column| column[row])
                .collect::<Vec<Felt>>();
            let next = columns
                .iter()
                .map(|column| column[row + 1])
                .collect::<Vec<Felt>>();
            let frame = EvaluationFrame::from_rows(current, next);
            let mut result = vec![Felt::ZERO; air.num_main_transition_constraints()];
            air.evaluate_transition(&frame, &[], &mut result);
            result
                .into_iter()
                .enumerate()
                .filter(|(_, residual)| *residual != Felt::ZERO)
                .map(move |(constraint, _)| (row, constraint))
                .collect::<Vec<_>>()
        })
        .collect()
}
