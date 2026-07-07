//! WebAssembly (WASM) module decoder and validator for the Ananse zkVM.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod error;
mod module;

pub use error::DecodeError;
pub use module::{ExportEntry, ExportKind, ImportEntry, Module, WASI_MODULE};

/// Result of WASM module decode/validate operations.
pub type Result<T> = core::result::Result<T, DecodeError>;
