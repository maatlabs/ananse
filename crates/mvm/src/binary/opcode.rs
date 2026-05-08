use num_derive::FromPrimitive;

/// WebAssembly instruction opcodes.
#[derive(Debug, FromPrimitive, PartialEq)]
pub enum Opcode {
    /// Conditional branch.
    If = 0x04,
    /// End of block, loop, or function.
    End = 0x0B,
    /// Return from function.
    Return = 0x0F,
    /// Read a local variable.
    LocalGet = 0x20,
    /// Write a local variable.
    LocalSet = 0x21,
    /// Store 32-bit integer to memory.
    I32Store = 0x36,
    /// Push 32-bit integer constant.
    I32Const = 0x41,
    /// Signed less-than (<) comparison for 32-bit integers.
    I32LtS = 0x48,
    /// Add two 32-bit integers.
    I32Add = 0x6A,
    /// Subtract two 32-bit integers.
    I32Sub = 0x6B,
    /// Call a function by index.
    Call = 0x10,
}
