//! Register-schedule interpreter for the Ananse zkVM.
//!
//! Ananse proves WebAssembly directly against a register-shaped AIR. This crate
//! executes a validated [`Module`](ananse_decoder::Module) under the static
//! register schedule [`ananse_lift`] produces and emits, per executed operator, the
//! [`StepRecord`] every later AIR family consumes.

#![forbid(unsafe_code)]

mod error;
mod host;
mod image;
mod interp;
mod record;
mod value;

pub use error::{ExecuteError, Trap};
pub use host::{Host, HostAction, NoHost};
pub use image::{
    WordType, entry_parameters, function_constants, function_opcodes, global_initializers,
};
pub use interp::{Entry, Execution, execute};
pub use record::{MemAccess, OpCode, RegAccess, StepObserver, StepRecord, Transition};
pub use value::Word;

/// Result of module execution operations.
pub type Result<T> = core::result::Result<T, ExecuteError>;
