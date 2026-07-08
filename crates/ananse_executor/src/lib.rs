//! Register-schedule interpreter for the Ananse zkVM.
//!
//! Ananse proves WebAssembly directly against a register-shaped AIR. This crate
//! executes a validated [`Module`](ananse_decoder::Module) under the static
//! register schedule [`ananse_lift`] produces and emits, per executed operator, the
//! [`StepRecord`] every later AIR family consumes.

#![forbid(unsafe_code)]

mod error;
mod host;
mod interp;
mod record;
mod value;

pub use ananse_decoder::{OpCode, Word};
pub use error::{ExecuteError, Trap};
pub use host::{Host, HostAction, NoHost};
pub use interp::{Entry, Execution, entry_parameters, execute};
pub use record::{MemAccess, RegAccess, StepObserver, StepRecord, Transition};

/// Result of module execution operations.
pub type Result<T> = core::result::Result<T, ExecuteError>;
