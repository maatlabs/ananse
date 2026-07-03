//! The unified access-log trace: the executor -> prover artifact.
//!
//! Every operator's effect on the machine is a sequence of reads and writes over
//! one flat address space that holds the operand stack, locals, globals, and linear
//! memory alike. The trace records those accesses on a fixed-width value bus in
//! execution order and, alongside, the same accesses sorted by address then time.
//! Proving the two are a permutation of one another, and that the sorted view is
//! internally consistent (every read returns the value the previous access to its
//! address left), is the single argument that ties the whole machine together.

use ananse_executor::{RegAccess, StepRecord, Transition, Word};
use ananse_lift::{LiftedFunction, LiftedProgram, Register, Successors};
use p3_field::{PrimeCharacteristicRing, PrimeField64};
use p3_goldilocks::Goldilocks as Felt;

use crate::layout::{
    self, BUS_SLOTS, COL_CLK, COL_HEIGHT, COL_IMM, COL_PC, GAP_BYTES, LIMB_BYTES, RC_WRITE_HI,
    RC_WRITE_LO, REGISTER_REGION, SELECTOR_BASE, bus_slot, rc_gap, slot, sorted, sorted_slot,
};
use crate::selector::{SEL_PADDING, opcode_index};
use crate::{Result, TraceError};

/// The smallest padded trace height, so the low-degree extension has room for the
/// FRI blowup and query positions on the shortest programs.
const MIN_TRACE_ROWS: usize = 8;

/// Register-file dimensions of the executing frame, used to resolve a register
/// operand to its address in the unified space.
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

/// One access on the unified log: an address, the timestamp at which it happened,
/// the accessed value's two limbs, and whether it was a write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Access {
    address: u64,
    timestamp: u64,
    lo: Felt,
    hi: Felt,
    is_write: bool,
}

/// A register-shaped execution trace: the append-only record stream paired with the
/// column-major field matrix the STARK prover commits to.
#[derive(Debug, Clone)]
pub struct Trace {
    records: Vec<StepRecord>,
    columns: Vec<Vec<Felt>>,
    locals: u32,
    globals: u32,
    initial: Vec<(u64, Felt, Felt)>,
    steps: usize,
    length: usize,
}

impl Trace {
    /// Builds a trace from a single frame's execution: the lifted `program` that
    /// scheduled it and the `records` it emitted, in execution order.
    pub fn build(
        program: &LiftedProgram,
        records: Vec<StepRecord>,
        args: &[Word],
        globals: &[Word],
    ) -> Result<Self> {
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

        let accesses = access_stream(&records, exec_frame)?;
        validate(&accesses)?;

        let halt_pc = func.instrs.len() as u32;
        let heights = func
            .instrs
            .iter()
            .map(|instr| instr.height_in)
            .collect::<Vec<_>>();
        let (columns, steps, length) = build_columns(
            &records,
            exec_frame,
            &accesses,
            &heights,
            halt_pc,
            edge_count(func),
        )?;

        Ok(Self {
            records,
            columns,
            locals: func.locals_count,
            globals: func.globals_count,
            initial: initial_state(exec_frame, args, globals),
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

    /// Number of columns in the trace, [`layout::main_width`].
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

    /// Number of local slots in the traced frame (parameters plus declared locals).
    pub fn locals(&self) -> u32 {
        self.locals
    }

    /// Register-file offset at which the operand stack begins: the locals followed by the globals.
    pub fn stack_base(&self) -> u32 {
        self.locals.saturating_add(self.globals)
    }

    /// The frame's public initial-state table as `(address, lo, hi)` triples, one per
    /// register-file cell in ascending address order.
    pub fn initial_state(&self) -> &[(u64, Felt, Felt)] {
        &self.initial
    }
}

/// The executing frame's public initial register image as `(address, lo, hi)` triples.
fn initial_state(frame: ExecFrame, args: &[Word], globals: &[Word]) -> Vec<(u64, Felt, Felt)> {
    (0..frame.width)
        .map(|offset| {
            let value = if offset < frame.locals {
                args.get(offset)
            } else if offset < frame.locals + frame.globals {
                globals.get(offset - frame.locals)
            } else {
                None
            };
            let (lo, hi) = value.map_or((Felt::ZERO, Felt::ZERO), |word| word.to_limbs());
            (REGISTER_REGION + offset as u64, lo, hi)
        })
        .collect()
}

fn access_stream(records: &[StepRecord], exec_frame: ExecFrame) -> Result<Vec<Access>> {
    records
        .iter()
        .enumerate()
        .map(|(step, record)| row_accesses(step, record, exec_frame))
        .collect::<Result<Vec<_>>>()
        .map(|rows| rows.into_iter().flatten().collect())
}

fn row_accesses(step: usize, record: &StepRecord, exec_frame: ExecFrame) -> Result<Vec<Access>> {
    let reg = |a: &RegAccess, is_write: bool| -> Result<Access> {
        let (lo, hi) = a.value.to_limbs();
        Ok(Access {
            address: register_address(a.reg, exec_frame)?,
            timestamp: 0,
            lo,
            hi,
            is_write,
        })
    };
    let reads = record.reads.iter().map(|a| reg(a, false));
    let mem = record.memory.iter().map(|m| {
        Ok(Access {
            address: m.address,
            timestamp: 0,
            lo: Felt::new(m.value & 0xFFFF_FFFF),
            hi: Felt::new(m.value >> 32),
            is_write: m.store,
        })
    });
    let writes = record.writes.iter().map(|a| reg(a, true));

    let mut accesses = reads.chain(mem).chain(writes).collect::<Result<Vec<_>>>()?;
    if accesses.len() > BUS_SLOTS {
        return Err(TraceError::AccessOverflow {
            step,
            count: accesses.len(),
        });
    }
    let base = (step * BUS_SLOTS) as u64;
    for (slot, access) in accesses.iter_mut().enumerate() {
        access.timestamp = base + slot as u64;
    }
    Ok(accesses)
}

fn register_address(register: Register, exec_frame: ExecFrame) -> Result<u64> {
    let offset = resolve_register(register, exec_frame)?;
    Ok(REGISTER_REGION + offset as u64)
}

fn immediate_offset(record: &StepRecord, exec_frame: ExecFrame) -> Result<u64> {
    record
        .reads
        .iter()
        .chain(&record.writes)
        .find(|a| matches!(a.reg, Register::Local(_) | Register::Global(_)))
        .map(|a| resolve_register(a.reg, exec_frame).map(|offset| offset as u64))
        .transpose()
        .map(|offset| offset.unwrap_or(0))
}

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

fn validate(accesses: &[Access]) -> Result<()> {
    let mut sorted = accesses.to_vec();
    sorted.sort_by_key(|a| (a.address, a.timestamp));
    for pair in sorted.windows(2) {
        let (prev, cur) = (pair[0], pair[1]);
        if cur.address == prev.address && !cur.is_write && (cur.lo, cur.hi) != (prev.lo, prev.hi) {
            return Err(TraceError::AccessInconsistent {
                address: cur.address,
                step: (cur.timestamp / BUS_SLOTS as u64) as usize,
            });
        }
    }
    Ok(())
}

fn build_columns(
    records: &[StepRecord],
    exec_frame: ExecFrame,
    accesses: &[Access],
    heights: &[u32],
    halt_pc: u32,
    edge_count: usize,
) -> Result<(Vec<Vec<Felt>>, usize, usize)> {
    let steps = records.len();
    let length = steps
        .max(edge_count)
        .max(layout::RANGE_TABLE_SIZE)
        .saturating_add(1)
        .max(MIN_TRACE_ROWS)
        .next_power_of_two();
    let mut columns = vec![vec![Felt::ZERO; length]; layout::main_width()];

    for (row, record) in records.iter().enumerate() {
        let height =
            heights
                .get(record.pc as usize)
                .copied()
                .ok_or(TraceError::InconsistentSchedule {
                    func_index: record.func_index,
                })?;
        columns[COL_PC][row] = Felt::new(u64::from(record.pc));
        columns[COL_CLK][row] = Felt::new(row as u64);
        columns[COL_HEIGHT][row] = Felt::new(u64::from(height));
        columns[COL_IMM][row] = Felt::new(immediate_offset(record, exec_frame)?);
        columns[SELECTOR_BASE + opcode_index(record.opcode)][row] = Felt::ONE;
        let row_accesses = row_accesses(row, record, exec_frame)?;
        for (index, access) in row_accesses.iter().enumerate() {
            write_bus_slot(&mut columns, bus_slot(index), row, *access);
        }

        if let Some(write) = row_accesses.iter().find(|access| access.is_write) {
            write_value_bytes(&mut columns, RC_WRITE_LO, row, write.lo);
            write_value_bytes(&mut columns, RC_WRITE_HI, row, write.hi);
        }
    }

    columns[COL_PC][steps..].fill(Felt::new(u64::from(halt_pc)));
    columns[SELECTOR_BASE + SEL_PADDING][steps..].fill(Felt::ONE);
    for (offset, clock) in columns[COL_CLK][steps..].iter_mut().enumerate() {
        *clock = Felt::new((steps + offset) as u64);
    }

    fill_sorted_log(&mut columns, accesses)?;

    Ok((columns, steps, length))
}

fn write_value_bytes(columns: &mut [Vec<Felt>], base: usize, row: usize, value: Felt) {
    let value = value.as_canonical_u64();
    for byte in 0..LIMB_BYTES {
        columns[base + byte][row] = Felt::new((value >> (8 * byte)) & 0xff);
    }
}

fn write_bus_slot(columns: &mut [Vec<Felt>], base: usize, row: usize, access: Access) {
    columns[base + slot::ADDR][row] = Felt::new(access.address);
    columns[base + slot::LO][row] = access.lo;
    columns[base + slot::HI][row] = access.hi;
    columns[base + slot::IS_WRITE][row] = boolean(access.is_write);
    columns[base + slot::ACTIVE][row] = Felt::ONE;
}

fn fill_sorted_log(columns: &mut [Vec<Felt>], accesses: &[Access]) -> Result<()> {
    let mut sorted = accesses.to_vec();
    sorted.sort_by_key(|a| (a.address, a.timestamp));

    for (position, access) in sorted.iter().enumerate() {
        let (row, slot_index) = (position / BUS_SLOTS, position % BUS_SLOTS);
        let base = sorted_slot(slot_index);
        columns[base + sorted::ADDR][row] = Felt::new(access.address);
        columns[base + sorted::TS][row] = Felt::new(access.timestamp);
        columns[base + sorted::LO][row] = access.lo;
        columns[base + sorted::HI][row] = access.hi;
        columns[base + sorted::IS_WRITE][row] = boolean(access.is_write);
        columns[base + sorted::ACTIVE][row] = Felt::ONE;

        let Some(&prev) = sorted
            .get(position.wrapping_sub(1))
            .filter(|_| position > 0)
        else {
            continue;
        };
        let same_addr = prev.address == access.address;
        columns[base + sorted::SAME_ADDR][row] = boolean(same_addr);

        let gap = if same_addr {
            access.timestamp.checked_sub(prev.timestamp)
        } else {
            access.address.checked_sub(prev.address)
        }
        .and_then(|diff| diff.checked_sub(1))
        .ok_or(TraceError::AccessLogNotStrictlyOrdered { position })?;
        let (gap_row, pair) = ((position - 1) / BUS_SLOTS, (position - 1) % BUS_SLOTS);
        let gap_base = rc_gap(pair);
        for byte in 0..GAP_BYTES {
            columns[gap_base + byte][gap_row] = Felt::new((gap >> (8 * byte)) & 0xff);
        }
    }
    Ok(())
}

/// A Boolean value; One if `flag`, zero otherwise.
fn boolean(flag: bool) -> Felt {
    if flag { Felt::ONE } else { Felt::ZERO }
}

fn edge_count(function: &LiftedFunction) -> usize {
    let targets = function
        .instrs
        .iter()
        .map(|sched| match &sched.successors {
            Successors::Fallthrough
            | Successors::Jump(_)
            | Successors::Return
            | Successors::Trap => 1,
            Successors::Branch { .. } => 2,
            Successors::Table { targets, .. } => targets.len().saturating_add(1),
        })
        .sum::<usize>();
    targets
        .saturating_add(function.instrs.len())
        .saturating_add(1)
}
