use ananse_decoder::{OpCode, Word};
use ananse_lift::Register;

/// A sink for the [`StepRecord`] stream an execution emits.
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

/// The observable effect of executing one WebAssembly operator.
///
/// One record is emitted per executed operator, in execution order.
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

/// A resolved register access: the static register operand the schedule named
/// and the concrete value execution observed there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegAccess {
    /// The depth-indexed register operand from the static schedule.
    pub reg: Register,
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
