//! Register-schedule interpreter for the Ananse zkVM.
//!
//! Ananse proves WebAssembly directly against a register-shaped AIR. This crate
//! executes a validated [`Module`](ananse_decoder::Module) under the static
//! register schedule [`ananse_lift`] produces and emits, per executed operator, the
//! [`StepRecord`] every later AIR family consumes.
//!
//! Execution reads opcode semantics and immediates from the module while taking
//! every register address and control-flow successor from the lift schedule, and
//! it checks the operand-stack height it reaches against the height the schedule
//! predicts at every program point. That check is the live cross-check of the
//! register lift: the schedule is sound only if it matches execution everywhere,
//! and a disagreement surfaces as [`ExecuteError::ScheduleMismatch`]. The record
//! stream is a deterministic function of the `(module, entry, arguments, host)`
//! inputs---there is no wall-clock time, randomness, or thread identity in the
//! execution path, and nondeterministic imports are already rejected at decode
//! time.
//!
//! Integer arithmetic in Ananse's own bookkeeping is checked; the WebAssembly
//! operators themselves use the specification's wrapping and trapping semantics.

#![forbid(unsafe_code)]

mod error;
mod host;
mod image;
mod interp;
mod record;
mod value;

pub use error::{ExecuteError, Trap};
pub use host::{Host, HostAction, NoHost};
pub use image::function_opcodes;
pub use interp::{Entry, Execution, execute};
pub use record::{MemAccess, OpCode, RegAccess, StepObserver, StepRecord, Transition};
pub use value::Word;

/// Result alias for execution operations.
pub type Result<T> = core::result::Result<T, ExecuteError>;
