use ananse_lift::Register;

use crate::Word;

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepRecord {
    pub func_index: u32,
    pub pc: u32,
    pub opcode: OpCode,
    pub reads: Vec<RegAccess>,
    pub writes: Vec<RegAccess>,
    pub memory: Vec<MemAccess>,
    /// The control-flow transition the operator took.
    pub transition: Transition,
}

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

/// A resolved register access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegAccess {
    pub reg: Register,
    pub value: Word,
}

/// A resolved linear-memory access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemAccess {
    pub address: u64,
    pub width: u8,
    pub value: u64,
    /// Whether the access is a store (`true`) or a load (`false`).
    pub store: bool,
}

/// The control-flow transition an executed instruction took.
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
