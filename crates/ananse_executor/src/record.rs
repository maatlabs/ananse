use ananse_decoder::{Image, Module, OpCode, Word, WordType};
use ananse_lift::Register;

use crate::{ExecuteError, Result};

/// Where execution begins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// Run the exported `_start`, else the first exported function, else the
    /// first defined function; a module with no defined function runs nothing.
    Auto,
    /// Run the function at this index in the module function index space.
    Function(u32),
    /// Run the function exported under this name.
    Export(String),
}

impl Entry {
    pub fn params(&self, module: &Module) -> Result<Vec<WordType>> {
        let image = Image::parse(module.bytes())?;
        let Some(func_index) = self.resolve(&image)? else {
            return Ok(Vec::new());
        };
        let type_idx = *image
            .func_types
            .get(func_index as usize)
            .ok_or(ExecuteError::UndefinedEntry)?;
        let ty = image
            .types
            .get(type_idx as usize)
            .ok_or(ExecuteError::UndefinedEntry)?;
        Ok(ty.params.clone())
    }

    pub fn resolve(&self, image: &Image) -> Result<Option<u32>> {
        match self {
            Self::Function(idx) => Ok(Some(*idx)),
            Self::Export(name) => image
                .func_exports
                .iter()
                .find(|(export, _)| export == name)
                .map(|(_, idx)| Some(*idx))
                .ok_or(ExecuteError::UndefinedEntry),
            Self::Auto => Ok(image
                .func_exports
                .iter()
                .find(|(name, _)| name == "_start")
                .or_else(|| image.func_exports.first())
                .map(|(_, idx)| *idx)
                .or_else(|| (!image.funcs.is_empty()).then_some(image.num_imported))),
        }
    }
}

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

/// The record-bearing result of executing one operator.
pub(crate) struct Outcome {
    pub(crate) opcode: OpCode,
    pub(crate) transition: Transition,
    pub(crate) mem: Vec<MemAccess>,
    pub(crate) control: Control,
}

impl Outcome {
    pub(crate) fn advance(opcode: OpCode, next: usize) -> Self {
        Outcome {
            opcode,
            transition: Transition::Next(next as u32),
            mem: Vec::new(),
            control: Control::Advance(next),
        }
    }
}

/// What executing a single operator produced.
pub(crate) enum Control {
    Advance(usize),
    Return(Vec<Word>),
    Exit(i32),
}

/// What a function activation produced.
pub(crate) enum Flow {
    Return(Vec<Word>),
    Exit(i32),
}

/// A runtime control frame, tracked only for the data a branch needs: the values
/// it carries and where it lands.
pub(crate) struct RtFrame {
    /// Whether the frame is a `loop` (its branch target is its own header).
    pub(crate) is_loop: bool,
    /// Values a branch to this label carries: the loop's input arity, or a
    /// forward block's result arity.
    pub(crate) branch_arity: u32,
    /// Program point of the frame's matching `end`.
    pub(crate) end_pc: u32,
}
