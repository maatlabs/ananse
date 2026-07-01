//! Register-column execution trace for the Ananse zkVM.
//!
//! Ananse proves WebAssembly directly against a register-shaped AIR. This crate
//! is the boundary between execution and proving: it turns the [`StepRecord`]
//! stream [`ananse_executor`] emits into the column-major field matrix the STARK
//! prover commits to, plus the address-sorted linear-memory access log the one
//! permutation argument runs over.
//!
//! One executed operator is one trace block---a single row, 1:1 with the source
//! bytecode. The columns are the program counter, a one-hot opcode selector, the
//! depth-indexed register bank (operand stack, locals, and globals as static
//! columns, needing no permutation argument), and the linear-memory access
//! columns. See [`Trace`] for the row semantics and [`layout`] for the column
//! assignment.
//!
//! [`StepRecord`]: ananse_executor::StepRecord

#![forbid(unsafe_code)]

mod error;
pub mod layout;
mod memory;
pub mod selector;
mod trace;

pub use error::TraceError;
pub use memory::MemoryAccess;
pub use trace::Trace;

/// Result alias for trace-building operations.
pub type Result<T> = core::result::Result<T, TraceError>;
