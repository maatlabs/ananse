//! The unified access-log trace: the executor -> prover artifact.
//!
//! Every operator's effect on the machine is a sequence of reads and writes over
//! one flat address space that holds the operand stack, locals, globals, and linear
//! memory alike. The trace records those accesses on a fixed-width value bus in
//! execution order and, alongside, the same accesses sorted by address then time.
//! Proving the two are a permutation of one another, and that the sorted view is
//! internally consistent (every read returns the value the previous access to its
//! address left), is the single argument that ties the whole machine together.

use ananse_executor::{OpCode, RegAccess, StepRecord, Transition, Word};
use ananse_lift::{LiftedFunction, LiftedProgram, Register, Successors};
use p3_field::{Field, PrimeCharacteristicRing, PrimeField64};
use p3_goldilocks::Goldilocks as Felt;

use crate::layout::{
    self, BUS_SLOTS, BW_A_BASE, BW_B_BASE, BW_NIBBLES, BW_P_BASE, COL_CLK, COL_HEIGHT, COL_IMM,
    COL_PC, GAP_BYTES, LIMB_BYTES, PC_DATA_A, PC_DATA_B, POPCNT_BYTE_BASE, POPCNT_BYTES,
    POPCNT_PC_BASE, RC_CMP_DHI, RC_CMP_DLO, RC_SIGN_A, RC_SIGN_B, RC_WRITE_HI, RC_WRITE_LO,
    REGISTER_REGION, SELECTOR_BASE, bus_slot, rc_gap, slot, sorted, sorted_slot, wit, witness,
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
            func,
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

    pub fn records(&self) -> &[StepRecord] {
        &self.records
    }

    pub fn columns(&self) -> &[Vec<Felt>] {
        &self.columns
    }

    pub fn column_at(&self, index: usize) -> Option<&[Felt]> {
        self.columns.get(index).map(Vec::as_slice)
    }

    pub fn width(&self) -> usize {
        self.columns.len()
    }

    pub fn length(&self) -> usize {
        self.length
    }

    pub fn steps(&self) -> usize {
        self.steps
    }

    pub fn locals(&self) -> u32 {
        self.locals
    }

    pub fn stack_base(&self) -> u32 {
        self.locals.saturating_add(self.globals)
    }

    pub fn initial_state(&self) -> &[(u64, Felt, Felt)] {
        &self.initial
    }
}

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
    func: &LiftedFunction,
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

        fill_comparison(&mut columns, row, record);
        fill_bitwise(&mut columns, row, record);
        fill_popcount(&mut columns, row, record);
        fill_pcdata(&mut columns, row, record, func);
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
    write_bytes(columns, base, row, value.as_canonical_u64(), LIMB_BYTES);
}

fn write_bytes(columns: &mut [Vec<Felt>], base: usize, row: usize, value: u64, count: usize) {
    for byte in 0..count {
        columns[base + byte][row] = Felt::new((value >> (8 * byte)) & 0xff);
    }
}

fn fill_comparison(columns: &mut [Vec<Felt>], row: usize, record: &StepRecord) {
    let limbs = |word: Word| {
        let (lo, hi) = word.to_limbs();
        (lo.as_canonical_u64(), hi.as_canonical_u64())
    };
    match record.opcode {
        op if is_binary_compare(op) => {
            let (Some(rhs), Some(lhs)) = (record.reads.first(), record.reads.get(1)) else {
                return;
            };
            let (llo, lhi) = limbs(lhs.value);
            let (rlo, rhi) = limbs(rhs.value);
            fill_borrow(columns, row, (llo, lhi), (rlo, rhi));
            fill_is_zero(columns, row, (llo, lhi), (rlo, rhi));
            if is_signed_compare(op) {
                let wide = is_i64_compare(op);
                fill_sign(
                    columns,
                    RC_SIGN_A,
                    witness::SIGN_A,
                    row,
                    if wide { lhi } else { llo },
                );
                fill_sign(
                    columns,
                    RC_SIGN_B,
                    witness::SIGN_B,
                    row,
                    if wide { rhi } else { rlo },
                );
            }
        }
        OpCode::I32Eqz | OpCode::I64Eqz | OpCode::Select | OpCode::If | OpCode::BrIf => {
            if let Some(operand) = record.reads.first() {
                fill_is_zero(columns, row, limbs(operand.value), (0, 0));
            }
        }
        OpCode::I64ExtendI32S => {
            if let Some(source) = record.reads.first() {
                let (slo, _) = limbs(source.value);
                fill_sign(columns, RC_SIGN_A, witness::SIGN_A, row, slo);
            }
        }
        _ => {}
    }
}

fn fill_pcdata(columns: &mut [Vec<Felt>], row: usize, record: &StepRecord, func: &LiftedFunction) {
    match record.opcode {
        OpCode::I32Const | OpCode::I64Const => {
            if let Some(write) = record.writes.first() {
                let (lo, hi) = write.value.to_limbs();
                columns[PC_DATA_A][row] = lo;
                columns[PC_DATA_B][row] = hi;
            }
        }
        OpCode::If | OpCode::BrIf => {
            if let Some(Successors::Branch { taken, not_taken }) = func
                .instrs
                .get(record.pc as usize)
                .map(|instr| &instr.successors)
            {
                columns[PC_DATA_A][row] = Felt::new(u64::from(*taken));
                columns[PC_DATA_B][row] = Felt::new(u64::from(*not_taken));
            }
        }
        _ => {}
    }
}

fn fill_borrow(columns: &mut [Vec<Felt>], row: usize, lhs: (u64, u64), rhs: (u64, u64)) {
    let two32 = 1u64 << 32;
    let (dlo, borrow_lo) = if lhs.0 >= rhs.0 {
        (lhs.0 - rhs.0, 0)
    } else {
        (lhs.0 + two32 - rhs.0, 1)
    };
    let net = i128::from(lhs.1) - i128::from(rhs.1) - i128::from(borrow_lo);
    let (dhi, borrow_hi) = if net >= 0 {
        (net as u64, 0)
    } else {
        ((net + i128::from(two32)) as u64, 1)
    };
    write_bytes(columns, RC_CMP_DLO, row, dlo, LIMB_BYTES);
    write_bytes(columns, RC_CMP_DHI, row, dhi, LIMB_BYTES);
    columns[wit(witness::BORROW_LO)][row] = Felt::new(borrow_lo);
    columns[wit(witness::BORROW_HI)][row] = Felt::new(borrow_hi);
}

fn fill_is_zero(columns: &mut [Vec<Felt>], row: usize, a: (u64, u64), b: (u64, u64)) {
    let g =
        (Felt::new(a.0) - Felt::new(b.0)) + (Felt::new(a.1) - Felt::new(b.1)) * Felt::new(1 << 32);
    let equal = g == Felt::ZERO;
    columns[wit(witness::EQUAL)][row] = boolean(equal);
    columns[wit(witness::INV)][row] = if equal { Felt::ZERO } else { g.inverse() };
}

fn fill_sign(columns: &mut [Vec<Felt>], rc_base: usize, sign_col: usize, row: usize, limb: u64) {
    let sign = (limb >> 31) & 1;
    let rest = limb & 0x7FFF_FFFF;
    write_bytes(columns, rc_base, row, rest, LIMB_BYTES);
    columns[rc_base + LIMB_BYTES][row] = Felt::new(2 * ((rest >> 24) & 0xff));
    columns[wit(sign_col)][row] = Felt::new(sign);
}

fn is_binary_compare(op: OpCode) -> bool {
    matches!(
        op,
        OpCode::I32Eq
            | OpCode::I32Ne
            | OpCode::I32LtS
            | OpCode::I32LtU
            | OpCode::I32GtS
            | OpCode::I32GtU
            | OpCode::I32LeS
            | OpCode::I32LeU
            | OpCode::I32GeS
            | OpCode::I32GeU
            | OpCode::I64Eq
            | OpCode::I64Ne
            | OpCode::I64LtS
            | OpCode::I64LtU
            | OpCode::I64GtS
            | OpCode::I64GtU
            | OpCode::I64LeS
            | OpCode::I64LeU
            | OpCode::I64GeS
            | OpCode::I64GeU
    )
}

fn is_signed_compare(op: OpCode) -> bool {
    matches!(
        op,
        OpCode::I32LtS
            | OpCode::I32GtS
            | OpCode::I32LeS
            | OpCode::I32GeS
            | OpCode::I64LtS
            | OpCode::I64GtS
            | OpCode::I64LeS
            | OpCode::I64GeS
    )
}

fn is_i64_compare(op: OpCode) -> bool {
    matches!(
        op,
        OpCode::I64Eq
            | OpCode::I64Ne
            | OpCode::I64LtS
            | OpCode::I64LtU
            | OpCode::I64GtS
            | OpCode::I64GtU
            | OpCode::I64LeS
            | OpCode::I64LeU
            | OpCode::I64GeS
            | OpCode::I64GeU
    )
}

fn fill_bitwise(columns: &mut [Vec<Felt>], row: usize, record: &StepRecord) {
    if !is_bitwise(record.opcode) {
        return;
    }
    let (Some(rhs), Some(lhs)) = (record.reads.first(), record.reads.get(1)) else {
        return;
    };
    let bits = |word: Word| {
        let (lo, hi) = word.to_limbs();
        lo.as_canonical_u64() | (hi.as_canonical_u64() << 32)
    };
    let (left, right) = (bits(lhs.value), bits(rhs.value));
    for i in 0..BW_NIBBLES {
        let a = (left >> (4 * i)) & 0xf;
        let b = (right >> (4 * i)) & 0xf;
        columns[BW_A_BASE + i][row] = Felt::new(a);
        columns[BW_B_BASE + i][row] = Felt::new(b);
        columns[BW_P_BASE + i][row] = Felt::new(a & b);
    }
}

fn is_bitwise(op: OpCode) -> bool {
    matches!(
        op,
        OpCode::I32And
            | OpCode::I32Or
            | OpCode::I32Xor
            | OpCode::I64And
            | OpCode::I64Or
            | OpCode::I64Xor
    )
}

fn fill_popcount(columns: &mut [Vec<Felt>], row: usize, record: &StepRecord) {
    if !matches!(record.opcode, OpCode::I32Popcnt | OpCode::I64Popcnt) {
        return;
    }
    let Some(operand) = record.reads.first() else {
        return;
    };
    let (lo, hi) = operand.value.to_limbs();
    let bits = lo.as_canonical_u64() | (hi.as_canonical_u64() << 32);
    for i in 0..POPCNT_BYTES {
        let byte = (bits >> (8 * i)) & 0xff;
        columns[POPCNT_BYTE_BASE + i][row] = Felt::new(byte);
        columns[POPCNT_PC_BASE + i][row] = Felt::new(u64::from((byte as u32).count_ones()));
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
