use alloc::vec::Vec;

use wasmparser::{
    CompositeInnerType, ExternalKind, FunctionBody, Import, Imports, Parser, Payload, TypeRef,
    ValidPayload, Validator, WasmFeatures,
};

use crate::{DecodeError, ExportEntry, ExportKind, ImportEntry, Result, WASI_MODULE};

const ALLOWED_WASI_FUNCS: &[&str] = &["fd_write", "proc_exit"];

/// WebAssembly proposals and features that are active during
/// validation and parsing of WebAssembly binaries.
fn wasm_features() -> WasmFeatures {
    let mut f = WasmFeatures::empty();
    f.insert(WasmFeatures::MUTABLE_GLOBAL);
    f
}

/// A validated WebAssembly module.
#[derive(Debug, Clone)]
pub struct Module {
    /// The raw bytes.
    bytes: Vec<u8>,
    /// Imported metadata.
    imports: Vec<ImportEntry>,
    /// Exported metadata.
    exports: Vec<ExportEntry>,
}

impl Module {
    /// Validates `bytes` against the WASM features that are enabled for validation,
    /// extracting import/export metadata.
    ///
    /// # Errors
    ///
    /// Returns a [`DecodeError`] if the module is invalid_binary, uses a feature outside the
    /// allowed subset, or imports anything other than the permitted WASI functions.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        validate(bytes)?;
        decode(bytes)
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

fn validate(bytes: &[u8]) -> Result<()> {
    let mut validator = Validator::new_with_features(wasm_features());
    for payload in Parser::new(0).parse_all(bytes) {
        let payload = payload.map_err(DecodeError::invalid_binary)?;
        let valid = validator
            .payload(&payload)
            .map_err(DecodeError::validation_failed)?;
        if let ValidPayload::Func(func, body) = valid {
            let mut fv = func.into_validator(Default::default());
            fv.validate(&body).map_err(DecodeError::validation_failed)?;
        }
    }
    Ok(())
}

fn decode(bytes: &[u8]) -> Result<Module> {
    let mut imports = Vec::new();
    let mut exports = Vec::new();

    for payload in Parser::new(0).parse_all(bytes) {
        let payload = payload.map_err(DecodeError::invalid_binary)?;
        match payload {
            Payload::ImportSection(reader) => {
                for group in reader {
                    let group = group.map_err(DecodeError::invalid_binary)?;
                    add_import_group(group, &mut imports)?;
                }
            }
            Payload::ExportSection(reader) => {
                for item in reader {
                    let export = item.map_err(DecodeError::invalid_binary)?;
                    if let Some(kind) = export_kind_from(export.kind) {
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

fn export_kind_from(kind: ExternalKind) -> Option<ExportKind> {
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

/// Module-level facts the per-function lift needs.
pub struct ModuleInfo<'a> {
    /// `(params, results)` arity per type index.
    pub type_arities: Vec<(u32, u32)>,
    /// Type index per function index (imported functions first).
    pub func_type_idx: Vec<u32>,
    /// Count of imported functions, which occupy the low function indices.
    pub num_imported_funcs: u32,
    /// Count of module globals.
    pub num_globals: u32,
    /// Defined function bodies, in code-section order.
    pub bodies: Vec<FunctionBody<'a>>,
}

impl<'a> ModuleInfo<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let mut type_arities = Vec::new();
        let mut func_type_idx = Vec::new();
        let mut num_imported_funcs = 0u32;
        let mut num_globals = 0u32;
        let mut bodies = Vec::new();

        for payload in Parser::new(0).parse_all(bytes) {
            match payload.map_err(DecodeError::invalid_binary)? {
                Payload::TypeSection(reader) => {
                    for rec in reader {
                        for sub in rec.map_err(DecodeError::invalid_binary)?.types() {
                            let arity = match &sub.composite_type.inner {
                                CompositeInnerType::Func(ft) => (
                                    u32::try_from(ft.params().len()).map_err(|_| {
                                        DecodeError::FunctionTooLarge { func_index: 0 }
                                    })?,
                                    u32::try_from(ft.results().len()).map_err(|_| {
                                        DecodeError::FunctionTooLarge { func_index: 0 }
                                    })?,
                                ),
                                _ => (0, 0),
                            };
                            type_arities.push(arity);
                        }
                    }
                }
                Payload::ImportSection(reader) => {
                    for group in reader {
                        if let Imports::Single(_, import) =
                            group.map_err(DecodeError::invalid_binary)?
                            && let TypeRef::Func(type_idx) | TypeRef::FuncExact(type_idx) =
                                import.ty
                        {
                            func_type_idx.push(type_idx);
                            num_imported_funcs = num_imported_funcs
                                .checked_add(1)
                                .ok_or(DecodeError::FunctionTooLarge { func_index: 0 })?;
                        }
                    }
                }
                Payload::FunctionSection(reader) => {
                    for type_idx in reader {
                        func_type_idx.push(type_idx.map_err(DecodeError::invalid_binary)?);
                    }
                }
                Payload::GlobalSection(reader) => num_globals = reader.count(),
                Payload::CodeSectionEntry(body) => bodies.push(body),
                _ => {}
            }
        }

        Ok(Self {
            type_arities,
            func_type_idx,
            num_imported_funcs,
            num_globals,
            bodies,
        })
    }

    /// `(params, results)` for a type index.
    pub fn type_arity(&self, type_idx: u32) -> Option<(u32, u32)> {
        self.type_arities.get(type_idx as usize).copied()
    }

    /// `(params, results)` for a function index, resolved through its type.
    pub fn func_arity(&self, func_idx: u32) -> Option<(u32, u32)> {
        let type_idx = *self.func_type_idx.get(func_idx as usize)?;
        self.type_arity(type_idx)
    }
}
