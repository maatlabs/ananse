//! Register-schedule interpreter for the Ananse zkVM.
//!
//! Ananse proves WebAssembly directly against a register-shaped AIR. This crate
//! executes a validated [`Module`](ananse_decoder::Module) under the static
//! register schedule [`ananse_lift`] produces and emits, per executed operator, the
//! [`StepRecord`] every later AIR family consumes.

#![forbid(unsafe_code)]

mod error;
mod interp;
mod record;
mod value;

use ananse_decoder::ImportEntry;
pub use ananse_decoder::{OpCode, Word};
pub use error::{ExecuteError, Trap};
pub use interp::{Execution, execute};
pub use record::{Entry, MemAccess, RegAccess, StepObserver, StepRecord, Transition};

/// Result of module execution operations.
pub type Result<T> = core::result::Result<T, ExecuteError>;

/// The host environment imported functions are dispatched to.
pub trait Host {
    /// Dispatches a call to the imported function `import` with `args`,
    /// granting mutable access to linear `memory` for argument and result
    /// marshalling.
    fn call(
        &mut self,
        import: &ImportEntry,
        args: &[Word],
        memory: &mut [u8],
    ) -> Result<HostAction>;
}

/// What an imported host function returns to the executor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostAction {
    /// The call returned the given results to the operand stack.
    Return(Vec<Word>),
    /// The call halted the program with this exit status (`proc_exit`).
    Exit(i32),
}

/// A host that rejects every import. Suitable for modules with no imports.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoHost;

impl Host for NoHost {
    fn call(
        &mut self,
        import: &ImportEntry,
        _args: &[Word],
        _memory: &mut [u8],
    ) -> Result<HostAction> {
        Err(ExecuteError::Host {
            module: import.module.clone(),
            name: import.name.clone(),
            message: "no host environment was supplied".into(),
        })
    }
}
