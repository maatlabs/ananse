use mvm_lift::Reg;

use crate::Word;

/// A sink for the [`StepRecord`] stream an execution emits.
///
/// The executor is generic over the observer so the same run drives the trace
/// builder in production and a recording collector in tests. Implementations
/// must not influence execution: the record stream is a function of the
/// `(module, entry, arguments, host)` inputs alone.
pub trait StepObserver {
    /// Receives the next executed operator's record.
    fn observe(&mut self, step: StepRecord);
}

/// Discards every record. Use when only the program's result is needed.
impl StepObserver for () {
    fn observe(&mut self, _step: StepRecord) {}
}

/// Collects the full record stream in execution order.
impl StepObserver for Vec<StepRecord> {
    fn observe(&mut self, step: StepRecord) {
        self.push(step);
    }
}

/// The observable effect of executing one WebAssembly operator: its program
/// point, identity, register and memory accesses, and the control-flow
/// transition it took. One [`StepRecord`] is emitted per executed operator, in
/// execution order, and is the unit every later AIR family consumes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepRecord {
    /// The executing function's index in the module function index space.
    pub func_index: u32,
    /// The operator's program point within its function body.
    pub pc: u32,
    /// The operator's identity.
    pub opcode: OpCode,
    /// Registers read, in the order the operator consumes them (top of the
    /// operand stack first), each resolved to its value.
    pub reads: Vec<RegAccess>,
    /// Registers written, each resolved to its post-execution value.
    pub writes: Vec<RegAccess>,
    /// Linear-memory accesses the operator performed.
    pub memory: Vec<MemAccess>,
    /// The control-flow transition the operator took.
    pub transition: Transition,
}

/// The identity of an executed WebAssembly operator, stripped of its immediate
/// operands. The immediate-bearing detail (a constant's value, a memory access's
/// address) is recovered from the [`StepRecord`]'s register and memory effects.
///
/// This is the per-opcode selector the register-AIR keys on: one variant per
/// operator in MVM's integer subset of WASM.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(missing_docs)]
pub enum OpCode {
    Unreachable,
    Nop,
    Block,
    Loop,
    If,
    Else,
    End,
    Br,
    BrIf,
    BrTable,
    Return,
    Call,
    Drop,
    Select,

    LocalGet,
    LocalSet,
    LocalTee,
    GlobalGet,
    GlobalSet,

    I32Const,
    I64Const,

    I32Load,
    I64Load,
    I32Load8S,
    I32Load8U,
    I32Load16S,
    I32Load16U,
    I64Load8S,
    I64Load8U,
    I64Load16S,
    I64Load16U,
    I64Load32S,
    I64Load32U,
    I32Store,
    I64Store,
    I32Store8,
    I32Store16,
    I64Store8,
    I64Store16,
    I64Store32,
    MemorySize,
    MemoryGrow,

    I32Eqz,
    I32Eq,
    I32Ne,
    I32LtS,
    I32LtU,
    I32GtS,
    I32GtU,
    I32LeS,
    I32LeU,
    I32GeS,
    I32GeU,
    I64Eqz,
    I64Eq,
    I64Ne,
    I64LtS,
    I64LtU,
    I64GtS,
    I64GtU,
    I64LeS,
    I64LeU,
    I64GeS,
    I64GeU,

    I32Clz,
    I32Ctz,
    I32Popcnt,
    I32Add,
    I32Sub,
    I32Mul,
    I32DivS,
    I32DivU,
    I32RemS,
    I32RemU,
    I32And,
    I32Or,
    I32Xor,
    I32Shl,
    I32ShrS,
    I32ShrU,
    I32Rotl,
    I32Rotr,
    I64Clz,
    I64Ctz,
    I64Popcnt,
    I64Add,
    I64Sub,
    I64Mul,
    I64DivS,
    I64DivU,
    I64RemS,
    I64RemU,
    I64And,
    I64Or,
    I64Xor,
    I64Shl,
    I64ShrS,
    I64ShrU,
    I64Rotl,
    I64Rotr,

    I32WrapI64,
    I64ExtendI32S,
    I64ExtendI32U,
}

/// A resolved register access: the static register operand the schedule named
/// and the concrete value execution observed there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegAccess {
    /// The depth-indexed register operand from the static schedule.
    pub reg: Reg,
    /// The value held in that register at this step.
    pub value: Word,
}

/// A resolved linear-memory access. Loads and stores are the only operations
/// that touch the address-sorted access log the trace permutes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemAccess {
    /// The effective byte address (base operand plus the static offset).
    pub address: u64,
    /// The access width in bytes (`1`, `2`, `4`, or `8`).
    pub width: u8,
    /// The little-endian integer value read or written, zero-extended to 64 bits.
    pub value: u64,
    /// Whether the access is a store (`true`) or a load (`false`).
    pub store: bool,
}

/// The control-flow transition an executed instruction took. The static schedule
/// enumerates the possible successors; this records the one execution selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    /// Control moved to a single program point (fall-through, a taken or
    /// untaken branch, or a multi-way branch's selected target).
    Next(u32),
    /// A direct call transferred control into a defined function, to resume at
    /// `return_pc` once the callee returns.
    Call {
        /// The callee's index in the module function index space.
        callee: u32,
        /// The caller program point to resume at after the callee returns.
        return_pc: u32,
    },
    /// The instruction returned from the function.
    Return,
    /// The instruction trapped.
    Trap,
    /// The program halted through a host `proc_exit` with this status code.
    Exit(i32),
}
