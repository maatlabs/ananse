//! One-hot opcode selectors: the mapping from a WebAssembly instruction to the
//! trace column that fires on its block.

use ananse_executor::OpCode;

/// Number of operators in Ananse's integer subset of WebAssembly, one per
/// [`OpCode`] variant. Each instruction owns a dedicated one-hot selector column so
/// the register-AIR can gate its per-opcode constraint on exactly its own blocks.
pub const NUM_OPCODES: usize = 103;

/// Selector index reserved for padding rows, placed immediately after the
/// per-opcode selectors.
pub const SEL_PADDING: usize = NUM_OPCODES;

/// Number of selector columns: one per instruction plus the padding selector.
pub const NUM_SELECTORS: usize = NUM_OPCODES + 1;

/// The one-hot selector index of an instruction, in `[0, NUM_OPCODES)`.
pub fn opcode_index(opcode: OpCode) -> usize {
    match opcode {
        OpCode::Unreachable => 0,
        OpCode::Nop => 1,
        OpCode::Block => 2,
        OpCode::Loop => 3,
        OpCode::If => 4,
        OpCode::Else => 5,
        OpCode::End => 6,
        OpCode::Br => 7,
        OpCode::BrIf => 8,
        OpCode::BrTable => 9,
        OpCode::Return => 10,
        OpCode::Call => 11,
        OpCode::Drop => 12,
        OpCode::Select => 13,

        OpCode::LocalGet => 14,
        OpCode::LocalSet => 15,
        OpCode::LocalTee => 16,
        OpCode::GlobalGet => 17,
        OpCode::GlobalSet => 18,

        OpCode::I32Const => 19,
        OpCode::I64Const => 20,

        OpCode::I32Load => 21,
        OpCode::I64Load => 22,
        OpCode::I32Load8S => 23,
        OpCode::I32Load8U => 24,
        OpCode::I32Load16S => 25,
        OpCode::I32Load16U => 26,
        OpCode::I64Load8S => 27,
        OpCode::I64Load8U => 28,
        OpCode::I64Load16S => 29,
        OpCode::I64Load16U => 30,
        OpCode::I64Load32S => 31,
        OpCode::I64Load32U => 32,
        OpCode::I32Store => 33,
        OpCode::I64Store => 34,
        OpCode::I32Store8 => 35,
        OpCode::I32Store16 => 36,
        OpCode::I64Store8 => 37,
        OpCode::I64Store16 => 38,
        OpCode::I64Store32 => 39,
        OpCode::MemorySize => 40,
        OpCode::MemoryGrow => 41,

        OpCode::I32Eqz => 42,
        OpCode::I32Eq => 43,
        OpCode::I32Ne => 44,
        OpCode::I32LtS => 45,
        OpCode::I32LtU => 46,
        OpCode::I32GtS => 47,
        OpCode::I32GtU => 48,
        OpCode::I32LeS => 49,
        OpCode::I32LeU => 50,
        OpCode::I32GeS => 51,
        OpCode::I32GeU => 52,
        OpCode::I64Eqz => 53,
        OpCode::I64Eq => 54,
        OpCode::I64Ne => 55,
        OpCode::I64LtS => 56,
        OpCode::I64LtU => 57,
        OpCode::I64GtS => 58,
        OpCode::I64GtU => 59,
        OpCode::I64LeS => 60,
        OpCode::I64LeU => 61,
        OpCode::I64GeS => 62,
        OpCode::I64GeU => 63,

        OpCode::I32Clz => 64,
        OpCode::I32Ctz => 65,
        OpCode::I32Popcnt => 66,
        OpCode::I32Add => 67,
        OpCode::I32Sub => 68,
        OpCode::I32Mul => 69,
        OpCode::I32DivS => 70,
        OpCode::I32DivU => 71,
        OpCode::I32RemS => 72,
        OpCode::I32RemU => 73,
        OpCode::I32And => 74,
        OpCode::I32Or => 75,
        OpCode::I32Xor => 76,
        OpCode::I32Shl => 77,
        OpCode::I32ShrS => 78,
        OpCode::I32ShrU => 79,
        OpCode::I32Rotl => 80,
        OpCode::I32Rotr => 81,
        OpCode::I64Clz => 82,
        OpCode::I64Ctz => 83,
        OpCode::I64Popcnt => 84,
        OpCode::I64Add => 85,
        OpCode::I64Sub => 86,
        OpCode::I64Mul => 87,
        OpCode::I64DivS => 88,
        OpCode::I64DivU => 89,
        OpCode::I64RemS => 90,
        OpCode::I64RemU => 91,
        OpCode::I64And => 92,
        OpCode::I64Or => 93,
        OpCode::I64Xor => 94,
        OpCode::I64Shl => 95,
        OpCode::I64ShrS => 96,
        OpCode::I64ShrU => 97,
        OpCode::I64Rotl => 98,
        OpCode::I64Rotr => 99,

        OpCode::I32WrapI64 => 100,
        OpCode::I64ExtendI32S => 101,
        OpCode::I64ExtendI32U => 102,
    }
}
