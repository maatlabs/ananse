//! Register-column execution trace and linear-memory access log for the Ananse zkVM.
//!
//! Ananse proves WebAssembly directly against a register-shaped AIR. This crate is
//! the boundary between execution and proving: it turns the [`StepRecord`] stream
//! [`ananse_executor`] emits into the column-major field matrix the STARK prover
//! commits to.
//!
//! One executed instruction is one trace block---a single row, 1:1 with the source
//! bytecode. The operand stack, locals, globals, and linear memory are lifted into
//! one flat address space; each row records the accesses its instruction made on a
//! fixed-width value bus, in execution order, alongside the same accesses sorted by
//! address then time. A single argument---the sorted view is a permutation of the
//! bus and is internally read-consistent---replaces the per-bank permutations a
//! stack machine would need. See [`Trace`] for the row semantics and [`layout`] for
//! the column assignment.
//!
//! [`StepRecord`]: ananse_executor::StepRecord

#![forbid(unsafe_code)]

mod error;
pub mod layout;
pub mod selector;
mod trace;

pub use error::TraceError;
pub use trace::Trace;

/// Result of execution trace-building operations.
pub type Result<T> = core::result::Result<T, TraceError>;
