mod label;
mod memory;
mod registers;
mod stack;

use ananse_decoder::Word;
pub use label::{Label, Labels};
pub use memory::Memory;
pub use registers::Registers;
pub use stack::OperandStack;

/// The state/outcome of an execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionResult {
    /// The entry function's return values, empty if the program exited.
    pub returns: Vec<Word>,
    /// The status code if the program halted through `proc_exit`.
    pub exit: Option<i32>,
    /// The number of instructions executed (records emitted).
    pub steps: u64,
}
