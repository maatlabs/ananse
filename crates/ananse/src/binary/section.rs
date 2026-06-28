use num_derive::FromPrimitive;

use super::{FunctionLocal, Instruction};

/// WebAssembly binary section identifiers.
#[derive(Debug, PartialEq, Eq, FromPrimitive)]
pub enum SectionCode {
    /// Custom section for metadata.
    Custom = 0x00,
    /// Type section containing function signatures.
    Type = 0x01,
    /// Import section for external dependencies.
    Import = 0x02,
    /// Function section mapping functions to types.
    Function = 0x03,
    /// Memory section declaring linear memories.
    Memory = 0x05,
    /// Export section for public interfaces.
    Export = 0x07,
    /// Code section containing function bodies.
    Code = 0x0a,
    /// Data section for memory initialization.
    Data = 0x0b,
}

/// A parsed function body from the code section.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// Local variable declarations.
    pub locals: Vec<FunctionLocal>,
    /// The function's instruction sequence.
    pub code: Vec<Instruction>,
}
