use alloc::vec::Vec;

/// A register operand referenced by a lifted instruction.
///
/// Ananse lifts WebAssembly's operand stack, locals, and globals into a single
/// per-function register file whose addresses are fixed at analysis time. The
/// file is laid out in three contiguous banks, low address to high:
///
/// 1. locals `[0, locals_count)` -- function parameters followed by declared locals;
/// 2. globals `[locals_count, locals_count + globals_count)` -- module globals;
/// 3. operand stack `[locals_count + globals_count, ..)` -- slot at depth `d` maps to
///    register `locals_count + globals_count + d`.
///
/// A [`Register`] keeps the bank symbolic rather than pre-flattened to that absolute
/// index: the flattening is a trace-layout decision (globals are shared across
/// functions, locals and stack slots are per-call-frame), so the lift records
/// *what* each instruction touches and leaves the column assignment downstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Register {
    /// A local slot, addressed by its WebAssembly local index (parameters first,
    /// then declared locals).
    Local(u32),
    /// A module global, addressed by its WebAssembly global index.
    Global(u32),
    /// An operand-stack slot, addressed by its absolute depth (`0` is the bottom
    /// of the stack).
    Stack(u32),
}

/// The result of lifting a validated module: one [`LiftedFunction`] per defined
/// (non-imported) function, in code-section order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiftedProgram {
    /// Lifted functions in code-section order.
    pub functions: Vec<LiftedFunction>,
}

/// A lifted WebAssembly function: its register-file dimensions and per-program-point
/// schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiftedFunction {
    /// Index of this function in the module's function index space. Imported
    /// functions occupy the low indices, so this is at least the imported-function
    /// count.
    pub func_index: u32,
    /// Number of local slots: function parameters plus declared locals.
    pub locals_count: u32,
    /// Number of module globals addressable by this function.
    pub globals_count: u32,
    /// Maximum operand-stack height reached anywhere in the body.
    pub max_stack_height: u32,
    /// Width of the register file, `locals_count + globals_count + max_stack_height`.
    pub reg_file_width: u32,
    /// Per-program-point schedule in body order; `instrs[i].pc == i`. The index
    /// one past the last entry is the function-exit sentinel used by
    /// [`Successors`].
    pub instrs: Vec<InstrSchedule>,
}

/// The static schedule for a single WebAssembly instruction.
///
/// One [`InstrSchedule`] is emitted per operator in body order, so
/// `instrs[i].pc == i`. It records the operand-stack height entering the
/// instruction, the register operands it reads and writes, and where control
/// flows next -- everything the register-AIR needs that is fixed by the module's
/// static stack typing, with no runtime stack pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrSchedule {
    /// Program point: the operator's zero-based index within the function body.
    pub pc: u32,
    /// Operand-stack height immediately before the instruction executes.
    pub height_in: u32,
    /// Registers read, in the order the instruction consumes them: the top of
    /// the operand stack first. Empty for instructions in unreachable code.
    pub reads: Vec<Register>,
    /// Registers written. Empty for instructions in unreachable code.
    pub writes: Vec<Register>,
    /// In-function control-flow successor(s).
    pub successors: Successors,
}

/// The in-function control-flow successor(s) of a lifted instruction.
///
/// Targets are program points (instruction indices). A target `t` with
/// `t as usize == LiftedFunction::instrs.len()` is the function-exit sentinel
/// and denotes a return: it arises when a branch leaves the outermost label.
/// Terminal instructions carry [`Successors::Return`] or [`Successors::Trap`]
/// directly rather than the sentinel.
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
