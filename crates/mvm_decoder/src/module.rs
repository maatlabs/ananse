use alloc::string::{String, ToString};
use alloc::vec::Vec;

use wasmparser::{
    BinaryReaderError, ExternalKind, Import, Imports, Parser, Payload, ValidPayload, Validator,
    WasmFeatures,
};

use crate::{DecodeError, Result};

/// The only import module namespace MVM permits.
pub const WASI_MODULE: &str = "wasi_snapshot_preview1";

const ALLOWED_WASI_FUNCS: &[&str] = &["fd_write", "proc_exit"];

/// A validated WebAssembly module: the raw bytes plus extracted import/export
/// metadata. Successful construction guarantees the module lies within MVM's
/// integer-only WASM subset (floating-point, SIMD, threads, GC, reference
/// types, multi-memory, tail calls, and exceptions are all rejected) and that
/// it imports only the deterministic WASI functions MVM supports.
#[derive(Debug, Clone)]
pub struct Module {
    bytes: Vec<u8>,
    imports: Vec<ImportEntry>,
    exports: Vec<ExportEntry>,
}

/// A single `(module, name)` import declared by a [`Module`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportEntry {
    /// The import's module namespace (always [`WASI_MODULE`] for a validated
    /// [`Module`]).
    pub module: String,
    /// The imported item's name.
    pub name: String,
}

/// A single export declared by a [`Module`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportEntry {
    /// The export's name.
    pub name: String,
    /// The kind of item being exported.
    pub kind: ExportKind,
    /// The index of the exported item within its index space.
    pub index: u32,
}

/// The kind of item an [`ExportEntry`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    /// A function export.
    Function,
    /// A table export.
    Table,
    /// A linear-memory export.
    Memory,
    /// A global export.
    Global,
}

impl Module {
    /// Validates `bytes` against MVM's restricted feature set and extracts
    /// import/export metadata. Returns a [`DecodeError`] if the module is
    /// malformed, uses a feature outside the integer-only WASM subset, or
    /// imports anything other than the permitted WASI functions.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        validate_module(bytes)?;
        decode_internal(bytes)
    }

    /// The validated module's raw WebAssembly bytes.
    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The module's import declarations, in declaration order.
    #[inline]
    pub fn imports(&self) -> &[ImportEntry] {
        &self.imports
    }

    /// The module's export declarations, in declaration order.
    #[inline]
    pub fn exports(&self) -> &[ExportEntry] {
        &self.exports
    }
}

/// The WebAssembly feature set MVM accepts. `FLOATS` is deliberately left
/// disabled: with it off, the validator rejects every floating-point type and
/// instruction wherever it appears---function signatures, globals, locals, and
/// instruction bodies alike.
fn features() -> WasmFeatures {
    let mut f = WasmFeatures::empty();
    f.insert(WasmFeatures::MUTABLE_GLOBAL);
    f
}

fn invalid_binary(e: BinaryReaderError) -> DecodeError {
    DecodeError::InvalidBinary {
        offset: e.offset(),
        message: e.to_string(),
    }
}

fn validation_failed(e: BinaryReaderError) -> DecodeError {
    DecodeError::ValidationFailed {
        offset: e.offset(),
        message: e.to_string(),
    }
}

fn validate_module(bytes: &[u8]) -> Result<()> {
    let mut validator = Validator::new_with_features(features());
    for payload in Parser::new(0).parse_all(bytes) {
        let payload = payload.map_err(invalid_binary)?;
        let valid = validator.payload(&payload).map_err(validation_failed)?;
        if let ValidPayload::Func(func, body) = valid {
            let mut fv = func.into_validator(Default::default());
            fv.validate(&body).map_err(validation_failed)?;
        }
    }
    Ok(())
}

fn decode_internal(bytes: &[u8]) -> Result<Module> {
    let mut imports = Vec::new();
    let mut exports = Vec::new();

    for payload in Parser::new(0).parse_all(bytes) {
        let payload = payload.map_err(invalid_binary)?;
        match payload {
            Payload::ImportSection(reader) => {
                for group in reader {
                    let group = group.map_err(invalid_binary)?;
                    add_import_group(group, &mut imports)?;
                }
            }
            Payload::ExportSection(reader) => {
                for item in reader {
                    let export = item.map_err(invalid_binary)?;
                    if let Some(kind) = from_external_kind(export.kind) {
                        exports.push(ExportEntry {
                            name: export.name.to_string(),
                            kind,
                            index: export.index,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    Ok(Module {
        bytes: bytes.to_vec(),
        imports,
        exports,
    })
}

fn add_import_group(group: Imports<'_>, imports: &mut Vec<ImportEntry>) -> Result<()> {
    match group {
        Imports::Single(_, import) => add_import(import, imports),
        Imports::Compact1 { .. } | Imports::Compact2 { .. } => Err(DecodeError::ValidationFailed {
            offset: 0,
            message: "compact-imports proposal is not supported".to_string(),
        }),
    }
}

fn add_import(import: Import<'_>, imports: &mut Vec<ImportEntry>) -> Result<()> {
    enforce_import_allowlist(import.module, import.name)?;
    imports.push(ImportEntry {
        module: import.module.to_string(),
        name: import.name.to_string(),
    });
    Ok(())
}

fn from_external_kind(kind: ExternalKind) -> Option<ExportKind> {
    match kind {
        ExternalKind::Func => Some(ExportKind::Function),
        ExternalKind::Table => Some(ExportKind::Table),
        ExternalKind::Memory => Some(ExportKind::Memory),
        ExternalKind::Global => Some(ExportKind::Global),
        ExternalKind::Tag | ExternalKind::FuncExact => None,
    }
}

fn enforce_import_allowlist(module: &str, name: &str) -> Result<()> {
    if module != WASI_MODULE {
        return Err(DecodeError::ForbiddenImportModule {
            module: module.to_string(),
        });
    }
    if !ALLOWED_WASI_FUNCS.contains(&name) {
        return Err(DecodeError::ForbiddenWasiImport {
            module: module.to_string(),
            name: name.to_string(),
        });
    }
    Ok(())
}
