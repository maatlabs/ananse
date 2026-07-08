//! WebAssembly (WASM) module decoder and validator for the Ananse zkVM.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod error;
mod image;
mod module;
mod opcode;
mod types;

pub use error::DecodeError;
pub use image::{
    Image, PAGE_SIZE, ValTy, function_constants, function_opcodes, global_initializers,
};
pub use module::{Module, WASI_MODULE};
pub use opcode::OpCode;
pub use types::{ExportEntry, ExportKind, ImportEntry, Word, WordType};

/// Result of WASM module decode/validate operations.
pub type Result<T> = core::result::Result<T, DecodeError>;
