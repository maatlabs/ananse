use alloc::string::{String, ToString};
use alloc::vec::Vec;

use wasmparser::{
    ExternalKind, Import, Imports, Parser, Payload, ValidPayload, Validator, WasmFeatures,
};

use crate::{DecodeError, Result, error as decode_error};

/// The only import module namespace Ananse permits.
pub const WASI_MODULE: &str = "wasi_snapshot_preview1";

const ALLOWED_WASI_FUNCS: &[&str] = &["fd_write", "proc_exit"];

/// A validated WebAssembly module.
#[derive(Debug, Clone)]
pub struct Module {
    bytes: Vec<u8>,
    imports: Vec<ImportEntry>,
    exports: Vec<ExportEntry>,
}

/// A single `(module, name)` import declared by a [`Module`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportEntry {
    pub module: String,
    pub name: String,
}

/// A single export declared by a [`Module`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportEntry {
    pub name: String,
    pub kind: ExportKind,
    pub index: u32,
}

/// The kind of item an [`ExportEntry`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Function,
    Table,
    Memory,
    Global,
}

impl Module {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        validate_module(bytes)?;
        decode_internal(bytes)
    }

    #[inline]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[inline]
    pub fn imports(&self) -> &[ImportEntry] {
        &self.imports
    }

    #[inline]
    pub fn exports(&self) -> &[ExportEntry] {
        &self.exports
    }
}

fn wasm_features() -> WasmFeatures {
    let mut f = WasmFeatures::empty();
    f.insert(WasmFeatures::MUTABLE_GLOBAL);
    f
}

fn validate_module(bytes: &[u8]) -> Result<()> {
    let mut validator = Validator::new_with_features(wasm_features());
    for payload in Parser::new(0).parse_all(bytes) {
        let payload = payload.map_err(decode_error::invalid_binary)?;
        let valid = validator
            .payload(&payload)
            .map_err(decode_error::validation_failed)?;
        if let ValidPayload::Func(func, body) = valid {
            let mut fv = func.into_validator(Default::default());
            fv.validate(&body)
                .map_err(decode_error::validation_failed)?;
        }
    }
    Ok(())
}

fn decode_internal(bytes: &[u8]) -> Result<Module> {
    let mut imports = Vec::new();
    let mut exports = Vec::new();

    for payload in Parser::new(0).parse_all(bytes) {
        let payload = payload.map_err(decode_error::invalid_binary)?;
        match payload {
            Payload::ImportSection(reader) => {
                for group in reader {
                    let group = group.map_err(decode_error::invalid_binary)?;
                    add_import_group(group, &mut imports)?;
                }
            }
            Payload::ExportSection(reader) => {
                for item in reader {
                    let export = item.map_err(decode_error::invalid_binary)?;
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
