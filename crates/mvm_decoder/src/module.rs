use alloc::string::{String, ToString};
use alloc::vec::Vec;

use wasmparser::{
    BinaryReaderError, ExternalKind, Import, Imports, Operator, Parser, Payload, TypeRef,
    ValidPayload, Validator, WasmFeatures,
};

use crate::{DecodeError, Result};

pub const WASI_MODULE: &str = "wasi_snapshot_preview1";

const ALLOWED_WASI_FUNCS: &[&str] = &["fd_write", "proc_exit"];

/// A validated WebAssembly module. Holds the raw bytes plus extracted metadata.
#[derive(Debug, Clone)]
pub struct Module {
    bytes: Vec<u8>,
    imports: Vec<ImportEntry>,
    exports: Vec<ExportEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportEntry {
    pub module: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportEntry {
    pub name: String,
    pub kind: ExportKind,
    pub index: u32,
}

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

fn features() -> WasmFeatures {
    let mut f = WasmFeatures::empty();
    f.insert(WasmFeatures::FLOATS);
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
    let mut imported_func_count: u32 = 0;
    let mut local_func_idx: u32 = 0;

    for payload in Parser::new(0).parse_all(bytes) {
        let payload = payload.map_err(invalid_binary)?;
        match payload {
            Payload::ImportSection(reader) => {
                for group in reader {
                    let group = group.map_err(invalid_binary)?;
                    add_import_group(group, &mut imports, &mut imported_func_count)?;
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
            Payload::CodeSectionEntry(body) => {
                let func_idx =
                    imported_func_count
                        .checked_add(local_func_idx)
                        .ok_or_else(|| DecodeError::ValidationFailed {
                            offset: 0,
                            message: "function index overflows u32".to_string(),
                        })?;
                let mut op_reader = body.get_operators_reader().map_err(invalid_binary)?;
                let mut instr_idx: u32 = 0;
                while !op_reader.eof() {
                    let op = op_reader.read().map_err(invalid_binary)?;
                    if let Some(name) = float_op_name(&op) {
                        return Err(DecodeError::FloatsDisabled {
                            func_idx,
                            instr_idx,
                            opcode: name,
                        });
                    }
                    instr_idx =
                        instr_idx
                            .checked_add(1)
                            .ok_or_else(|| DecodeError::ValidationFailed {
                                offset: 0,
                                message: "instruction index overflows u32".to_string(),
                            })?;
                }
                local_func_idx =
                    local_func_idx
                        .checked_add(1)
                        .ok_or_else(|| DecodeError::ValidationFailed {
                            offset: 0,
                            message: "local function count overflows u32".to_string(),
                        })?;
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

fn add_import_group(
    group: Imports<'_>,
    imports: &mut Vec<ImportEntry>,
    imported_func_count: &mut u32,
) -> Result<()> {
    match group {
        Imports::Single(_, import) => add_import(import, imports, imported_func_count),

        Imports::Compact1 { .. } | Imports::Compact2 { .. } => Err(DecodeError::ValidationFailed {
            offset: 0,
            message: "compact-imports proposal is not supported".to_string(),
        }),
    }
}

fn add_import(
    import: Import<'_>,
    imports: &mut Vec<ImportEntry>,
    imported_func_count: &mut u32,
) -> Result<()> {
    enforce_import_allowlist(import.module, import.name)?;
    imports.push(ImportEntry {
        module: import.module.to_string(),
        name: import.name.to_string(),
    });
    if matches!(import.ty, TypeRef::Func(_) | TypeRef::FuncExact(_)) {
        *imported_func_count =
            imported_func_count
                .checked_add(1)
                .ok_or_else(|| DecodeError::ValidationFailed {
                    offset: 0,
                    message: "imported function count overflows u32".to_string(),
                })?;
    }
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

fn float_op_name(op: &Operator) -> Option<&'static str> {
    use wasmparser::Operator::*;
    Some(match op {
        F32Load { .. } => "f32.load",
        F64Load { .. } => "f64.load",
        F32Store { .. } => "f32.store",
        F64Store { .. } => "f64.store",
        F32Const { .. } => "f32.const",
        F64Const { .. } => "f64.const",
        F32Eq => "f32.eq",
        F32Ne => "f32.ne",
        F32Lt => "f32.lt",
        F32Gt => "f32.gt",
        F32Le => "f32.le",
        F32Ge => "f32.ge",
        F64Eq => "f64.eq",
        F64Ne => "f64.ne",
        F64Lt => "f64.lt",
        F64Gt => "f64.gt",
        F64Le => "f64.le",
        F64Ge => "f64.ge",
        F32Abs => "f32.abs",
        F32Neg => "f32.neg",
        F32Ceil => "f32.ceil",
        F32Floor => "f32.floor",
        F32Trunc => "f32.trunc",
        F32Nearest => "f32.nearest",
        F32Sqrt => "f32.sqrt",
        F32Add => "f32.add",
        F32Sub => "f32.sub",
        F32Mul => "f32.mul",
        F32Div => "f32.div",
        F32Min => "f32.min",
        F32Max => "f32.max",
        F32Copysign => "f32.copysign",
        F64Abs => "f64.abs",
        F64Neg => "f64.neg",
        F64Ceil => "f64.ceil",
        F64Floor => "f64.floor",
        F64Trunc => "f64.trunc",
        F64Nearest => "f64.nearest",
        F64Sqrt => "f64.sqrt",
        F64Add => "f64.add",
        F64Sub => "f64.sub",
        F64Mul => "f64.mul",
        F64Div => "f64.div",
        F64Min => "f64.min",
        F64Max => "f64.max",
        F64Copysign => "f64.copysign",
        I32TruncF32S => "i32.trunc_f32_s",
        I32TruncF32U => "i32.trunc_f32_u",
        I32TruncF64S => "i32.trunc_f64_s",
        I32TruncF64U => "i32.trunc_f64_u",
        I64TruncF32S => "i64.trunc_f32_s",
        I64TruncF32U => "i64.trunc_f32_u",
        I64TruncF64S => "i64.trunc_f64_s",
        I64TruncF64U => "i64.trunc_f64_u",
        F32ConvertI32S => "f32.convert_i32_s",
        F32ConvertI32U => "f32.convert_i32_u",
        F32ConvertI64S => "f32.convert_i64_s",
        F32ConvertI64U => "f32.convert_i64_u",
        F32DemoteF64 => "f32.demote_f64",
        F64ConvertI32S => "f64.convert_i32_s",
        F64ConvertI32U => "f64.convert_i32_u",
        F64ConvertI64S => "f64.convert_i64_s",
        F64ConvertI64U => "f64.convert_i64_u",
        F64PromoteF32 => "f64.promote_f32",
        I32ReinterpretF32 => "i32.reinterpret_f32",
        I64ReinterpretF64 => "i64.reinterpret_f64",
        F32ReinterpretI32 => "f32.reinterpret_i32",
        F64ReinterpretI64 => "f64.reinterpret_i64",
        _ => return None,
    })
}
