//! Schedule-driven interpreter and reference semantics for the Ananse zkVM.
//!
//! Ananse proves WebAssembly directly against a register-shaped AIR. This crate
//! executes a validated [`Module`] under the static register schedule
//! [`ananse_lift`] produces and emits, per executed instruction, the
//! [`StepRecord`] every later AIR family consumes.

#![forbid(unsafe_code)]

mod entry;
mod error;
mod interpreter;
mod runtime;
mod step;

use ananse_decoder::{Image, ImportEntry, Module};
pub use ananse_decoder::{OpCode, Word};
pub use entry::Entry;
pub use error::{ExecuteError, Trap};
pub use runtime::ExecutionResult;
pub use step::{MemAccess, RegAccess, StepObserver, StepRecord, Transition};

/// Result of module execution operations.
pub type Result<T> = core::result::Result<T, ExecuteError>;

/// Executes a validated module under its static register schedule.
///
/// The module is lifted to its register schedule and parsed into an executable
/// image, then interpreted: every instruction's register touches are resolved
/// against the live operand stack, locals, and globals and emitted as a
/// [`StepRecord`]. At each program point the operand-stack height execution
/// reaches is checked against the height the schedule predicts; a disagreement
/// is reported as [`ExecuteError::ScheduleMismatch`].
///
/// The record stream is a deterministic function of the
/// `(module, entry, args, host)` inputs.
pub fn execute<O: StepObserver, H: Host>(
    module: &Module,
    entry: &Entry,
    args: &[Word],
    host: &mut H,
    observer: &mut O,
) -> Result<ExecutionResult> {
    use interpreter::{Completion, Interpreter};
    use runtime::Memory;

    let program = ananse_lift::lift(module)?;
    let image = Image::parse(module.bytes())?;

    let Some(func_index) = entry.resolve(&image)? else {
        return Ok(ExecutionResult {
            returns: Vec::new(),
            exit: None,
            steps: 0,
        });
    };
    let defined = func_index
        .checked_sub(image.num_imported)
        .map(|d| d as usize)
        .filter(|&d| d < image.funcs.len())
        .ok_or(ExecuteError::UndefinedEntry)?;

    let (params, _) = image
        .func_arity(func_index)
        .ok_or(ExecuteError::UndefinedEntry)?;

    let call_args = crate::entry::validate_args(&image, func_index, params, args)?;

    let mut interpreter = Interpreter {
        globals: image.globals.clone(),
        memory: Memory::new(image.memory.clone(), image.max_pages),
        observer,
        host,
        steps: 0,
    };
    let flow = interpreter.eval(&image, &program, defined, call_args, 0)?;

    Ok(match flow {
        Completion::Return(returns) => ExecutionResult {
            returns,
            exit: None,
            steps: interpreter.steps,
        },
        Completion::Exit(code) => ExecutionResult {
            returns: Vec::new(),
            exit: Some(code),
            steps: interpreter.steps,
        },
    })
}

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
