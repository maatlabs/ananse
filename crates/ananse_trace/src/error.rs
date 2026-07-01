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

    /// A register operand resolves to a column outside the function's register
    /// file. Indicates a lift/execution disagreement on the register-file width.
    #[error("register {register:?} lies outside the register file of width {width}")]
    RegisterOutOfRange {
        /// The offending register operand.
        register: Register,
        /// The register-file width the operand exceeded.
        width: usize,
    },

    /// A read observed a register value that disagrees with the value the trace
    /// reconstructed for that column at that step.
    #[error("register {register:?} read at step {step} disagrees with the reconstructed value")]
    RegisterInconsistency {
        /// The step (row index) of the disagreeing read.
        step: usize,
        /// The register whose reconstructed value the read contradicts.
        register: Register,
    },

    /// A linear-memory read returned a value that disagrees with the most recent
    /// write to its address, breaking the access log's read-consistency.
    #[error("linear-memory read at step {step} disagrees with the last write to address {address}")]
    MemoryInconsistent {
        /// The address whose read-consistency failed.
        address: u64,
        /// The step (row index) of the disagreeing read.
        step: usize,
    },
}
