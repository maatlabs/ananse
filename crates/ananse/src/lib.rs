//! Ananse: a WebAssembly-native zero-knowledge virtual machine, as a library.
//!
//! `ananse` is the umbrella crate over the workspace. It re-exports every member
//! crate under a short, namespaced module ([`decoder`], [`lift`], [`executor`],
//! [`trace`], [`air`], [`wasi`]) so downstream tooling depends on a single
//! `ananse` crate rather than the individual `ananse_*` crates, and it adds a
//! [`Runtime`] facade that wraps the decode -> lift -> execute pipeline behind
//! one entry point. The [`prelude`] gathers the handful of items most callers
//! need:
//!
//! ```
//! use ananse::prelude::*;
//!
//! // A module that returns the constant 42 from its only export.
//! let wasm = wat::parse_str("(module (func (export \"answer\") (result i32) (i32.const 42)))")
//!     .expect("assemble");
//! let mut runtime = Runtime::instantiate(&wasm).expect("instantiate");
//! let execution = runtime
//!     .call(&Entry::Export("answer".into()), &[])
//!     .expect("run");
//! assert_eq!(execution.returns, vec![Word::I32(42)]);
//! ```
#![forbid(unsafe_code)]

pub use ananse_air as air;
pub use ananse_decoder as decoder;
use ananse_decoder::{DecodeError, Module};
pub use ananse_executor as executor;
use ananse_executor::{
    Entry, ExecuteError, Execution, Host, NoHost, Word, WordType, entry_parameters, execute,
};
pub use ananse_lift as lift;
pub use ananse_trace as trace;
pub use ananse_wasi as wasi;
use ananse_wasi::WasiSnapshotPreview1;

/// Result alias for [`Runtime`] operations.
pub type Result<T> = core::result::Result<T, Error>;

/// An error from decoding or executing a module through the [`Runtime`] facade.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The module failed to decode or validate.
    #[error(transparent)]
    Decode(#[from] DecodeError),
    /// Execution failed---a trap, an unsupported operator, or a host error.
    #[error(transparent)]
    Execute(#[from] ExecuteError),
}

/// A decoded module bound to the host its imports dispatch to.
///
/// The runtime decodes once at construction and runs an entry to completion on
/// [`call`](Self::call), so a caller never threads the module, host, and step
/// observer through [`executor::execute`] by hand.
/// The host is generic: [`instantiate`](Self::instantiate) rejects every import,
/// [`instantiate_with_wasi`](Self::instantiate_with_wasi) journals output through
/// the deterministic minimal WASI, and [`instantiate_with_host`](Self::instantiate_with_host)
/// accepts any [`Host`].
pub struct Runtime<H: Host> {
    module: Module,
    host: H,
}

impl Runtime<NoHost> {
    /// Decodes `bytes` into a runtime that rejects every import---for modules
    /// that import nothing.
    pub fn instantiate(bytes: &[u8]) -> Result<Self> {
        Self::instantiate_with_host(bytes, NoHost)
    }
}

impl Runtime<WasiSnapshotPreview1> {
    /// Decodes `bytes` into a runtime backed by the deterministic minimal WASI
    /// host, so a module calling `fd_write` / `proc_exit` journals its output
    /// (readable afterward through [`host`](Self::host)).
    pub fn instantiate_with_wasi(bytes: &[u8]) -> Result<Self> {
        Self::instantiate_with_host(bytes, WasiSnapshotPreview1::new())
    }
}

impl<H: Host> Runtime<H> {
    /// Decodes `bytes` into a runtime dispatching imports to `host`.
    pub fn instantiate_with_host(bytes: &[u8], host: H) -> Result<Self> {
        Ok(Self {
            module: Module::decode(bytes)?,
            host,
        })
    }

    /// The decoded module.
    pub fn module(&self) -> &Module {
        &self.module
    }

    /// The host imported functions dispatch to.
    pub fn host(&self) -> &H {
        &self.host
    }

    /// The parameter widths `entry` expects, in order --- the types a caller
    /// coerces its arguments to before [`call`](Self::call).
    pub fn parameters(&self, entry: &Entry) -> Result<Vec<WordType>> {
        Ok(entry_parameters(&self.module, entry)?)
    }

    /// Runs `entry` with `args` to completion, returning its results, exit
    /// status, and executed-step count.
    pub fn call(&mut self, entry: &Entry, args: &[Word]) -> Result<Execution> {
        Ok(execute(&self.module, entry, args, &mut self.host, &mut ())?)
    }
}

/// The items most callers need to decode, run, and inspect a module. Glob-import
/// with `use ananse::prelude::*;`.
pub mod prelude {
    pub use crate::decoder::Module;
    pub use crate::executor::{Entry, Execution, Word, WordType};
    pub use crate::wasi::WasiSnapshotPreview1;
    pub use crate::{Error, Result, Runtime};
}
