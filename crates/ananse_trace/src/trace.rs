//! The register-column trace: the executor -> prover artifact.

use ananse_executor::{StepRecord, Transition};
use ananse_lift::{LiftedFunction, LiftedProgram, Register};
use maat_field::{Felt, FieldElement};

use crate::layout::{
    COL_MEM_ADDR, COL_MEM_IS_WRITE, COL_MEM_VAL_HI, COL_MEM_VAL_LO, COL_PC, SELECTOR_BASE,
    register_limb_columns, trace_width,
};
use crate::memory::{self, MemoryAccess};
use crate::selector::{SEL_PADDING, opcode_index};
use crate::{Result, TraceError};

/// Winterfell's minimum trace length.
const MIN_TRACE_ROWS: usize = 8;

/// Register-file dimensions of the executing frame.
#[derive(Clone, Copy)]
struct ExecFrame {
    locals: usize,
    globals: usize,
    width: usize,
}

impl ExecFrame {
    fn of(func: &LiftedFunction) -> Self {
        Self {
            locals: func.locals_count as usize,
            globals: func.globals_count as usize,
            width: func.reg_file_width as usize,
        }
    }
}

/// A register-shaped execution trace: the append-only record stream paired with
/// the column-major field matrix the STARK prover commits to.
#[derive(Debug, Clone)]
pub struct Trace {
    records: Vec<StepRecord>,
    columns: Vec<Vec<Felt>>,
    access_log: Vec<MemoryAccess>,
    register_width: usize,
    locals_count: usize,
    globals_count: usize,
    steps: usize,
    length: usize,
}

impl Trace {
    /// Builds a trace from a single frame's execution: the lifted `program` that
    /// scheduled it and the `records` it emitted, in execution order.
    pub fn build(program: &LiftedProgram, records: Vec<StepRecord>) -> Result<Self> {
        let Some(first) = records.first() else {
            return Err(TraceError::EmptyExecution);
        };
        let func_index = first.func_index;
        if records
            .iter()
            .any(|r| r.func_index != func_index || matches!(r.transition, Transition::Call { .. }))
        {
            return Err(TraceError::UnsupportedCall);
        }

        let func = program
            .functions
            .iter()
            .find(|f| f.func_index == func_index)
            .ok_or(TraceError::InconsistentSchedule { func_index })?;
        let exec_frame = ExecFrame::of(func);

        let initial = initial_register_file(&records, exec_frame)?;
        let (columns, steps, length) = build_columns(&records, exec_frame, initial)?;

        let access_log = memory::access_log(&records);
        memory::validate(&access_log)?;

        Ok(Self {
            records,
            columns,
            access_log,
            register_width: exec_frame.width,
            locals_count: exec_frame.locals,
            globals_count: exec_frame.globals,
            steps,
            length,
        })
    }

    /// The record stream the trace was built from, in execution order.
    pub fn records(&self) -> &[StepRecord] {
        &self.records
    }

    /// The column-major trace matrix: one inner vector per column, each of length
    /// [`length`](Self::length).
    pub fn columns(&self) -> &[Vec<Felt>] {
        &self.columns
    }

    /// The column at `index`, or `None` if it is out of range.
    pub fn column_at(&self, index: usize) -> Option<&[Felt]> {
        self.columns.get(index).map(Vec::as_slice)
    }

    /// The address-sorted linear-memory access log the prover permutes against
    /// the execution-order memory columns.
    pub fn access_log(&self) -> &[MemoryAccess] {
        &self.access_log
    }

    /// Number of columns in the trace, `trace_width(register_width)`.
    pub fn width(&self) -> usize {
        self.columns.len()
    }

    /// Number of rows after power-of-two padding.
    pub fn length(&self) -> usize {
        self.length
    }

    /// Number of executed operators (unpadded rows).
    pub fn steps(&self) -> usize {
        self.steps
    }

    /// Width of the register bank, `locals + globals + max operand-stack height`.
    pub fn register_width(&self) -> usize {
        self.register_width
    }

    /// The `(lo, hi)` limb columns holding `register`, or `None` if it is outside
    /// this frame's register file.
    pub fn register_columns(&self, register: Register) -> Option<(usize, usize)> {
        let exec_frame = ExecFrame {
            locals: self.locals_count,
            globals: self.globals_count,
            width: self.register_width,
        };
        resolve_register(register, exec_frame)
            .ok()
            .map(register_limb_columns)
    }
}

/// Materializes the column-major matrix, simulating the register file forward and
/// cross-checking every read against the reconstructed value.
fn build_columns(
    records: &[StepRecord],
    exec_frame: ExecFrame,
    mut regfile: Vec<(Felt, Felt)>,
) -> Result<(Vec<Vec<Felt>>, usize, usize)> {
    let steps = records.len();
    let length = steps.max(MIN_TRACE_ROWS).next_power_of_two();
    let width = trace_width(exec_frame.width);
    let mut columns = vec![vec![Felt::ZERO; length]; width];

    for (row, record) in records.iter().enumerate() {
        columns[COL_PC][row] = Felt::new(u64::from(record.pc));
        columns[SELECTOR_BASE + opcode_index(record.opcode)][row] = Felt::ONE;
        for (offset, &(lo, hi)) in regfile.iter().enumerate() {
            let (lo_col, hi_col) = register_limb_columns(offset);
            columns[lo_col][row] = lo;
            columns[hi_col][row] = hi;
        }
        for read in &record.reads {
            let offset = resolve_register(read.reg, exec_frame)?;
            if regfile[offset] != read.value.to_limbs() {
                return Err(TraceError::RegisterInconsistency {
                    step: row,
                    register: read.reg,
                });
            }
        }
        if let Some(access) = record.memory.first() {
            columns[COL_MEM_ADDR][row] = Felt::new(access.address);
            columns[COL_MEM_VAL_LO][row] = Felt::new(access.value & 0xFFFF_FFFF);
            columns[COL_MEM_VAL_HI][row] = Felt::new(access.value >> 32);
            columns[COL_MEM_IS_WRITE][row] = if access.store { Felt::ONE } else { Felt::ZERO };
        }
        for write in &record.writes {
            let offset = resolve_register(write.reg, exec_frame)?;
            regfile[offset] = write.value.to_limbs();
        }
    }

    // Padding rows hold the final register file and the last program counter, and
    // one-hot the padding selector so the halt state persists across the tail.
    let last_pc = records.last().map_or(0, |record| record.pc);
    columns[COL_PC][steps..].fill(Felt::new(u64::from(last_pc)));
    columns[SELECTOR_BASE + SEL_PADDING][steps..].fill(Felt::ONE);
    for (offset, &(lo, hi)) in regfile.iter().enumerate() {
        let (lo_col, hi_col) = register_limb_columns(offset);
        columns[lo_col][steps..].fill(lo);
        columns[hi_col][steps..].fill(hi);
    }

    Ok((columns, steps, length))
}

/// Resolves a register operand to its offset within the register bank, mirroring
/// the lift's three-bank layout: locals, then globals, then the operand stack.
fn resolve_register(register: Register, exec_frame: ExecFrame) -> Result<usize> {
    let offset = match register {
        Register::Local(index) => Some(index as usize),
        Register::Global(index) => exec_frame.locals.checked_add(index as usize),
        Register::Stack(depth) => exec_frame
            .locals
            .checked_add(exec_frame.globals)
            .and_then(|base| base.checked_add(depth as usize)),
    };
    offset
        .filter(|&offset| offset < exec_frame.width)
        .ok_or(TraceError::RegisterOutOfRange {
            register,
            width: exec_frame.width,
        })
}

/// Recovers the register file entering the first step: a register's value before
/// its first write is the value its first pre-write read observed (else zero, an
/// unconstrained slot the AIR never reads).
fn initial_register_file(
    records: &[StepRecord],
    exec_frame: ExecFrame,
) -> Result<Vec<(Felt, Felt)>> {
    let mut initial = vec![(Felt::ZERO, Felt::ZERO); exec_frame.width];
    let mut written = vec![false; exec_frame.width];
    let mut known = vec![false; exec_frame.width];
    for record in records {
        for read in &record.reads {
            let offset = resolve_register(read.reg, exec_frame)?;
            if !written[offset] && !known[offset] {
                initial[offset] = read.value.to_limbs();
                known[offset] = true;
            }
        }
        for write in &record.writes {
            let offset = resolve_register(write.reg, exec_frame)?;
            written[offset] = true;
        }
    }
    Ok(initial)
}
