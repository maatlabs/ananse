//! WASM module decoder for Maat zkVM.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod error;
mod module;

pub use error::DecodeError;
pub use module::{ExportEntry, ExportKind, ImportEntry, Module, WASI_MODULE};

pub type Result<T> = core::result::Result<T, DecodeError>;
