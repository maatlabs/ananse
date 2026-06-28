//! Static stack-to-register lift for the Ananse zkVM.
//!
//! Ananse proves WebAssembly directly with a register-shaped AIR. This crate is the
//! analysis that makes that possible without compiling WASM away: it turns a
//! validated [`Module`] into a [`LiftedProgram`] that records, for every program
//! point, the operand-stack height, the depth-indexed register operands each
//! instruction reads and writes, the per-function register-file width, and the
//! control-flow successor map.
//!
//! WebAssembly validation already fixes a single operand-stack height at every
//! reachable program point. The lift re-derives that height by abstract
//! interpretation over the structured control flow and addresses the operand
//! stack, locals, and globals as a single static register file (see [`Reg`]).
//! Because register identity is positional and every predecessor of a control-flow
//! join agrees on height, merges need no value muxing---only the next program
//! point is data-dependent, surfaced through [`Successors`].

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod analysis;
mod error;
mod program;

use ananse_decoder::Module;
pub use error::LiftError;
pub use program::{InstrSchedule, LiftedFunction, LiftedProgram, Reg, Successors};

/// Result alias for lift operations.
pub type Result<T> = core::result::Result<T, LiftError>;

/// Lifts a validated [`Module`] to its static register form.
///
/// Returns one [`LiftedFunction`] per defined (non-imported) function, in
/// code-section order. Because the input is an already-validated [`Module`], an
/// error indicates an internal inconsistency or a function outside Ananse's integer
/// subset of WASM rather than malformed user input.
pub fn lift(module: &Module) -> Result<LiftedProgram> {
    Ok(LiftedProgram {
        functions: analysis::lift_bytes(module.bytes())?,
    })
}
