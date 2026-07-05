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
    #[error("failed to lift module: {0}")]
    Lift(#[from] LiftError),

    #[error("malformed module at offset {offset}: {message}")]
    MalformedModule { offset: usize, message: String },

    #[error("unsupported operator at offset {offset}: {message}")]
    UnsupportedOperator { offset: usize, message: String },

    /// No runnable entry point matched the requested [`Entry`](crate::Entry).
    #[error("entry point not found")]
    UndefinedEntry,

    #[error("entry expects {expected} arguments but {actual} were supplied")]
    ArgumentCountMismatch { expected: usize, actual: usize },

    #[error(
        "function {func_index} pc {pc}: schedule height {schedule} disagrees with execution height {actual}"
    )]
    ScheduleMismatch {
        func_index: u32,
        pc: u32,
        schedule: u32,
        actual: u32,
    },

    /// The program trapped.
    #[error("trap: {0}")]
    Trap(#[from] Trap),

    #[error("host call to {module}::{name} failed: {message}")]
    Host {
        module: String,
        name: String,
        message: String,
    },
}

/// A WebAssembly trap: a runtime fault that halts the program.
///
/// Traps are a legitimate outcome the proof system reasons about,
/// so they are not an executor defect.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Trap {
    #[error("unreachable executed")]
    Unreachable,

    #[error("integer divide by zero")]
    DivideByZero,

    #[error("integer overflow")]
    IntegerOverflow,

    #[error("out-of-bounds memory access")]
    MemoryOutOfBounds,

    #[error("call stack exhausted")]
    CallStackExhausted,
}
