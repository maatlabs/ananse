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
use wasmparser::Operator;

/// Result of WASM module decode/validate operations.
pub type Result<T> = core::result::Result<T, DecodeError>;

/// Largest addressable size of a 32-bit linear memory, in pages.
pub const WASM_PAGE_SIZE: usize = 65536;

/// The supported WebAssembly System Interface module namespace.
pub const WASI_MODULE: &str = "wasi_snapshot_preview1";

/// The [`OpCode`] of every operator in a defined function's body, indexed by
/// program point.
pub fn function_opcodes(module: &Module, func_index: u32) -> Result<Vec<OpCode>> {
    let image = Image::parse(module.bytes())?;
    let func = image
        .funcs
        .iter()
        .find(|f| f.func_index == func_index)
        .ok_or_else(|| DecodeError::internal("no defined function for the requested index"))?;
    func.ops.iter().map(OpCode::from_operator).collect()
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
    Ok(func.ops.iter().map(operator_constant).collect())
}

fn operator_constant(op: &Operator) -> Option<u64> {
    match op {
        Operator::I32Const { value } => Some(u64::from(*value as u32)),
        Operator::I64Const { value } => Some(*value as u64),
        _ => None,
    }
}
