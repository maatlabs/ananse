use alloc::vec::Vec;

/// The result of lifting a validated module: one [`LiftedFunction`] per defined
/// (non-imported) function, in code-section order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiftedProgram {
    /// Lifted functions in code-section order.
    pub functions: Vec<LiftedFunction>,
}

/// A lifted WebAssembly function: its register-file dimensions and per-program-point
/// instruction schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiftedFunction {
    /// Index of this function in the module's function index space.
    pub func_index: u32,
    /// Number of local slots: function parameters plus declared locals.
    pub locals_count: u32,
    /// Number of module globals addressable by this function.
    pub globals_count: u32,
    /// Maximum operand-stack height reached anywhere in the body.
    pub max_stack_height: u32,
    /// Width of the register file, `locals_count + globals_count + max_stack_height`.
    pub reg_file_width: u32,
    /// Per-program-point instruction schedule in body order.
    pub schedules: Vec<Schedule>,
}

/// The static schedule for a single WebAssembly instruction.
///
/// A single instruction schedule is emitted per operator/instruction in body order,
/// so `schedules[i].pc == i`. It records the operand-stack height entering the
/// instruction, the register operands it reads and writes, and where control
/// flows next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    /// Program point: the instruction's zero-based index within the function body.
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

/// A register operand referenced by a lifted instruction.
///
/// The executor lifts WASM's operand stack, locals, and globals into a single
/// per-function register file whose addresses are fixed at analysis time. The
/// file is laid out in three contiguous banks, low address to high:
///
/// 1. locals `[0, locals_count)` --- function parameters followed by declared locals;
/// 2. globals `[locals_count, locals_count + globals_count)` --- module globals;
/// 3. operand stack `[locals_count + globals_count, ..)` --- slot at depth `d` maps to
///    register `locals_count + globals_count + d`.
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

/// A WebAssembly structured-control frame, tracked as the lift walks a body.
pub struct CtrlFrame {
    /// Input values a branch target carries to this label. Used by
    /// loops (the label is the loop header).
    pub in_arity: u32,
    /// Output values a branch target carries to this label. Used by
    /// non-loops (the label is past the matching `end`).
    pub out_arity: u32,
    /// Operand-stack height at frame entry, below the frame's inputs.
    pub floor: u32,
    /// Whether the current point in this frame is unreachable (dead code).
    pub unreachable: bool,
    /// `Some(header_pc)` for a `loop`, whose branch target is its own header;
    /// `None` for forward-closing frames.
    pub loop_header: Option<u32>,
    /// Forward branches awaiting this frame's continuation, patched at `end`.
    pub fixups: Vec<Fixup>,
    /// For an `if` frame, the `if` instruction whose `not_taken` target is still
    /// pending an `else` or `end`.
    pub if_instr: Option<usize>,
}

/// A forward branch whose target becomes known when its frame closes.
pub struct Fixup {
    /// Index of the instruction whose successor target must be patched.
    pub instr_index: usize,
    /// Which successor field of that instruction to patch.
    pub slot: SuccessorSlot,
}

pub enum SuccessorSlot {
    Jump,
    BranchTaken,
    TableEntry(usize),
    TableDefault,
}

/// The stack effect and register touches of a non-control value instruction.
pub struct ValueEffect {
    pub pops: u32,
    pub pushes: u32,
    pub bank_read: Option<Register>,
    pub bank_write: Option<Register>,
    /// `local.tee`: reads the operand-stack top and writes a local while leaving
    /// the top in place, so its stack slot is not rewritten.
    pub is_tee: bool,
}

impl ValueEffect {
    pub fn stack(pops: u32, pushes: u32) -> Self {
        Self {
            pops,
            pushes,
            bank_read: None,
            bank_write: None,
            is_tee: false,
        }
    }

    pub fn bank_read(reg: Register) -> Self {
        Self {
            pops: 0,
            pushes: 1,
            bank_read: Some(reg),
            bank_write: None,
            is_tee: false,
        }
    }

    pub fn bank_write(reg: Register) -> Self {
        Self {
            pops: 1,
            pushes: 0,
            bank_read: None,
            bank_write: Some(reg),
            is_tee: false,
        }
    }

    pub fn tee(reg: Register) -> Self {
        Self {
            pops: 1,
            pushes: 1,
            bank_read: None,
            bank_write: Some(reg),
            is_tee: true,
        }
    }
}
