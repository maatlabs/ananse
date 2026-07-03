use alloc::vec::Vec;

/// The result of lifting a validated module: one [`LiftedFunction`] per defined
/// (non-imported) function, in code-section order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiftedProgram {
    /// Lifted functions in code-section order.
    pub functions: Vec<LiftedFunction>,
}

/// A lifted WebAssembly function: its register-file dimensions and per-program-point schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiftedFunction {
    pub func_index: u32,
    pub locals_count: u32,
    pub globals_count: u32,
    pub max_stack_height: u32,
    pub reg_file_width: u32,
    pub instrs: Vec<InstructionSchedule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionSchedule {
    pub pc: u32,
    pub height_in: u32,
    pub reads: Vec<Register>,
    pub writes: Vec<Register>,
    pub successors: Successors,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Register {
    Local(u32),
    Global(u32),
    Stack(u32),
}

/// The in-function control-flow successor(s) of a lifted instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Successors {
    /// Control falls through to the next program point (`pc + 1`).
    Fallthrough,
    /// Unconditional transfer to a single target program point.
    Jump(u32),
    /// Two-way branch. `taken` is entered when the condition holds, `not_taken`
    /// otherwise. Produced by `if` (taken = then-arm) and `br_if` (taken = the
    /// branch target).
    Branch {
        /// Target entered when the condition is non-zero.
        taken: u32,
        /// Target entered when the condition is zero.
        not_taken: u32,
    },
    /// Multi-way branch (`br_table`): one target per table entry plus a default
    /// target for indices beyond the table.
    Table {
        /// Targets selected by the branch index, in table order.
        targets: Vec<u32>,
        /// Target selected when the index is out of range.
        default: u32,
    },
    /// The instruction returns from the function (`return`, a branch to the
    /// outermost label, or the function's final `end`).
    Return,
    /// The instruction traps unconditionally (`unreachable`).
    Trap,
}
