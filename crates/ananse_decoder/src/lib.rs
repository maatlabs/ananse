//! WebAssembly (WASM) module decoder and validator for the Ananse zkVM.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod error;
mod module;
mod opcode;
mod types;

pub use error::DecodeError;
pub use module::{Module, ModuleInfo};
pub use opcode::OpCode;
pub use types::{ExportEntry, ExportKind, Image, ImportEntry, Word, WordType};

/// Result of WASM module decode/validate operations.
pub type Result<T> = core::result::Result<T, DecodeError>;

/// WebAssembly instructions.
pub type Instruction<'a> = wasmparser::Operator<'a>;

/// The prime field known as Goldilocks, defined as `F_p` where p = 2^64 - 2^32 + 1.
pub type Felt = p3_goldilocks::Goldilocks;

/// Largest addressable size of a 32-bit linear memory, in pages.
pub const WASM32_PAGE_SIZE: usize = 65536;

/// The supported WebAssembly System Interface module namespace.
pub const WASI_MODULE: &str = "wasi_snapshot_preview1";

/// Returns the corresponding [`OpCode`] of every [Instruction] in the
/// body of a defined function with index `func_index`.
pub fn function_opcodes(module: &Module, func_index: u32) -> Result<Vec<OpCode>> {
    let image = Image::parse(module.bytes())?;
    let func = image
        .funcs
        .iter()
        .find(|f| f.func_index == func_index)
        .ok_or_else(|| DecodeError::internal("no defined function for the requested index"))?;
    func.instructions
        .iter()
        .map(OpCode::from_instruction)
        .collect()
}

pub fn global_initializers(module: &Module) -> Result<Vec<Word>> {
    Ok(Image::parse(module.bytes())?.globals)
}

pub fn function_constants(module: &Module, func_index: u32) -> Result<Vec<Option<u64>>> {
    let image = Image::parse(module.bytes())?;
    let func = image
        .funcs
        .iter()
        .find(|f| f.func_index == func_index)
        .ok_or_else(|| DecodeError::internal("no defined function for the requested index"))?;
    Ok(func.instructions.iter().map(instruction_constant).collect())
}

fn instruction_constant(i: &Instruction) -> Option<u64> {
    match i {
        Instruction::I32Const { value } => Some(u64::from(*value as u32)),
        Instruction::I64Const { value } => Some(*value as u64),
        _ => None,
    }
}
