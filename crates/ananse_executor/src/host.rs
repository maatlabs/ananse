use ananse_decoder::ImportEntry;

use crate::{ExecuteError, Result, Word};

/// The host environment imported functions are dispatched to.
///
/// Ananse's decoder admits only the `wasi_snapshot_preview1` `fd_write` and
/// `proc_exit` imports, both of which must be realized deterministically: no
/// wall-clock time, no randomness, no real file descriptors. The executor never
/// implements a host itself---it is supplied by the caller---so the WASI
/// surface lives outside the execution core.
pub trait Host {
    /// Dispatches a call to the imported function `import` with `args`,
    /// granting mutable access to linear `memory` for argument and result
    /// marshalling. Returning an [`ExecuteError`] aborts execution.
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
