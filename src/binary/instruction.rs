use super::Block;

/// A decoded WebAssembly instruction with its operands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    /// Conditional branch with block type.
    If(Block),
    /// End of block, loop, or function.
    End,
    /// Return from the current function.
    Return,
    /// Read local variable at the given index.
    LocalGet(u32),
    /// Write local variable at the given index.
    LocalSet(u32),
    /// Store 32-bit integer to memory with alignment and offset.
    I32Store {
        /// Memory alignment hint (log2 of byte alignment).
        align: u32,
        /// Byte offset added to the address.
        offset: u32,
    },
    /// Push a 32-bit integer constant onto the stack.
    I32Const(i32),
    /// Signed less-than (<) comparison for 32-bit integers.
    I32Lts,
    /// Add two 32-bit integers.
    I32Add,
    /// Subtract two 32-bit integers.
    I32Sub,
    /// Call a function by its index.
    Call(u32),
}
