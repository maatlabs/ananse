//! Errors while building a [`Trace`](crate::Trace).

use ananse_lift::Register;

/// A failure encountered while turning an execution's record stream into a trace.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TraceError {
    /// The execution produced no records; there is nothing to trace.
    #[error("execution produced no records")]
    EmptyExecution,

    /// The record stream crosses a defined-function call boundary.
    #[error("execution spans a defined-function call; cross-frame tracing is not yet supported")]
    UnsupportedCall,

    /// No lifted function matches the record stream's function index. Indicates
    /// the schedule and the records were produced from different modules.
    #[error("no lifted function for index {func_index}")]
    InconsistentSchedule {
        /// The function index carried by the records.
        func_index: u32,
    },

    /// A register operand resolves to an offset outside the function's register
    /// file. Indicates a lift/execution disagreement on the register-file width.
    #[error("register {register:?} lies outside the register file of width {width}")]
    RegisterOutOfRange {
        /// The offending register operand.
        register: Register,
        /// The register-file width the operand exceeded.
        width: usize,
    },

    /// A single operator performed more accesses than the value bus has slots.
    /// Every operator in Ananse's integer subset fits, so this signals a lift or
    /// executor defect rather than an input the bus is too narrow for.
    #[error("operator at step {step} performed {count} accesses, more than the value bus holds")]
    AccessOverflow {
        /// The step (row index) of the offending operator.
        step: usize,
        /// The number of accesses the operator performed.
        count: usize,
    },

    /// A read returned a value that disagrees with the most recent write to its
    /// address, breaking the unified access log's read-consistency. Covers operand
    /// stack, locals, globals, and linear memory alike, since one log serves them
    /// all.
    #[error("read at step {step} disagrees with the last write to address {address}")]
    AccessInconsistent {
        /// The address whose read-consistency failed.
        address: u64,
        /// The step (row index) of the disagreeing read.
        step: usize,
    },
}
