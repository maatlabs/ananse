use ananse_lift::LiftError;
use wasmparser::BinaryReaderError;

pub(crate) fn malformed(e: BinaryReaderError) -> ExecuteError {
    ExecuteError::MalformedModule {
        offset: e.offset(),
        message: e.to_string(),
    }
}

pub(crate) fn inconsistent(message: &str) -> ExecuteError {
    ExecuteError::MalformedModule {
        offset: 0,
        message: message.into(),
    }
}

/// An error produced while executing a validated module.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExecuteError {
    /// The static register schedule could not be built for the module.
    #[error("failed to lift module: {0}")]
    Lift(#[from] LiftError),

    /// The module's bytes could not be parsed into an executable image.
    #[error("malformed module at offset {offset}: {message}")]
    MalformedModule {
        /// Byte offset at which parsing failed.
        offset: usize,
        /// Human-readable detail from the underlying parser.
        message: String,
    },

    /// An operator the executor does not implement was reached.
    #[error("unsupported operator at offset {offset}: {message}")]
    UnsupportedOperator {
        /// Byte offset of the rejected operator.
        offset: usize,
        /// Which operator was rejected.
        message: String,
    },

    /// No runnable entry point matched the requested [`Entry`](crate::Entry).
    #[error("entry point not found")]
    UndefinedEntry,

    /// The supplied argument count did not match the entry function's arity.
    #[error("entry expects {expected} arguments but {actual} were supplied")]
    ArgumentCountMismatch {
        /// Parameters the entry function declares.
        expected: usize,
        /// Arguments supplied to [`execute`](crate::execute).
        actual: usize,
    },

    /// Live execution disagreed with the static schedule at a program point.
    #[error(
        "function {func_index} pc {pc}: schedule height {schedule} disagrees with execution height {actual}"
    )]
    ScheduleMismatch {
        /// The offending function's index in the module function index space.
        func_index: u32,
        /// The program point at which the heights disagreed.
        pc: u32,
        /// Operand-stack height the schedule predicts entering the instruction.
        schedule: u32,
        /// Operand-stack height execution actually reached.
        actual: u32,
    },

    /// The program trapped.
    #[error("trap: {0}")]
    Trap(#[from] Trap),

    /// An imported host function failed.
    #[error("host call to {module}::{name} failed: {message}")]
    Host {
        /// The failed import's module namespace.
        module: String,
        /// The failed import's name.
        name: String,
        /// Human-readable detail from the host.
        message: String,
    },
}

/// A WebAssembly trap: a runtime fault that halts the program.
///
/// Traps are a legitimate outcome the proof system reasons about,
/// so they are not an executor defect.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Trap {
    /// An `unreachable` instruction executed.
    #[error("unreachable executed")]
    Unreachable,
    /// An integer division by zero.
    #[error("integer divide by zero")]
    DivideByZero,
    /// A signed division overflow (`MIN / -1`).
    #[error("integer overflow")]
    IntegerOverflow,
    /// A load or store outside the bounds of linear memory.
    #[error("out-of-bounds memory access")]
    MemoryOutOfBounds,
    /// The call stack exceeded its depth limit.
    #[error("call stack exhausted")]
    CallStackExhausted,
}
