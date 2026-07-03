//! Errors while building a [`Trace`](crate::Trace).

use ananse_lift::Register;

/// A failure encountered while turning an execution's record stream into a trace.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TraceError {
    #[error("execution produced no records")]
    EmptyExecution,

    #[error("execution spans a defined-function call; cross-frame tracing is not yet supported")]
    UnsupportedCall,

    #[error("no lifted function for index {func_index}")]
    InconsistentSchedule { func_index: u32 },

    #[error("register {register:?} lies outside the register file of width {width}")]
    RegisterOutOfRange { register: Register, width: usize },

    #[error("operator at step {step} performed {count} accesses, more than the value bus holds")]
    AccessOverflow { step: usize, count: usize },

    #[error("read at step {step} disagrees with the last write to address {address}")]
    AccessInconsistent { address: u64, step: usize },

    #[error("sorted access log is not strictly ordered at position {position}")]
    AccessLogNotStrictlyOrdered { position: usize },
}
