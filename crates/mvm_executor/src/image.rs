use mvm_decoder::ImportEntry;
use wasmparser::{
    BlockType, CompositeInnerType, ConstExpr, DataKind, FunctionBody, Imports, Operator, Parser,
    Payload, TypeRef, ValType,
};

use crate::{ExecuteError, Result, Word, error as exec_error};

/// Bytes per WebAssembly memory page.
pub(crate) const PAGE_SIZE: usize = 65536;

/// An executable view of a validated module.
pub(crate) struct Image<'a> {
    pub(crate) types: Vec<FnType>,
    pub(crate) func_types: Vec<u32>,
    pub(crate) num_imported: u32,
    pub(crate) imports: Vec<ImportEntry>,
    pub(crate) globals: Vec<Word>,
    pub(crate) funcs: Vec<FuncImage<'a>>,
    pub(crate) func_exports: Vec<(String, u32)>,
    pub(crate) memory: Vec<u8>,
    pub(crate) max_pages: Option<u64>,
}

/// A function signature reduced to what execution needs: parameter types and a
/// result count.
pub(crate) struct FnType {
    pub(crate) params: Vec<ValTy>,
    pub(crate) results: u32,
}

/// A defined function's executable image: its operators, the declared-local
/// types, and the program point of each structured block's matching `end`.
pub(crate) struct FuncImage<'a> {
    pub(crate) func_index: u32,
    pub(crate) type_idx: u32,
    pub(crate) declared: Vec<ValTy>,
    pub(crate) ops: Vec<Operator<'a>>,
    /// `ends[pc]` is the `end` program point of the `block` / `loop` / `if` that
    /// opens at `pc`; `0` for every other program point.
    pub(crate) ends: Vec<u32>,
}

/// A value type in MVM's integer subset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ValTy {
    I32,
    I64,
}

impl ValTy {
    /// The zero value of this type.
    pub(crate) fn zero(self) -> Word {
        match self {
            ValTy::I32 => Word::I32(0),
            ValTy::I64 => Word::I64(0),
        }
    }
}

fn val_ty(ty: ValType) -> Result<ValTy> {
    match ty {
        ValType::I32 => Ok(ValTy::I32),
        ValType::I64 => Ok(ValTy::I64),
        _ => Err(exec_error::inconsistent(
            "value type outside the integer subset",
        )),
    }
}

impl<'a> Image<'a> {
    pub(crate) fn parse(bytes: &'a [u8]) -> Result<Self> {
        let mut types = Vec::new();
        let mut func_types = Vec::new();
        let mut num_imported = 0u32;
        let mut imports = Vec::new();
        let mut globals = Vec::new();
        let mut funcs = Vec::new();
        let mut func_exports = Vec::new();
        let mut memory: Vec<u8> = Vec::new();
        let mut max_pages = None;

        for payload in Parser::new(0).parse_all(bytes) {
            match payload.map_err(exec_error::malformed)? {
                Payload::TypeSection(reader) => {
                    for rec in reader {
                        for sub in rec.map_err(exec_error::malformed)?.types() {
                            types.push(fn_type(&sub.composite_type.inner)?);
                        }
                    }
                }
                Payload::ImportSection(reader) => {
                    for group in reader {
                        if let Imports::Single(_, import) = group.map_err(exec_error::malformed)?
                            && let TypeRef::Func(type_idx) | TypeRef::FuncExact(type_idx) =
                                import.ty
                        {
                            func_types.push(type_idx);
                            imports.push(ImportEntry {
                                module: import.module.into(),
                                name: import.name.into(),
                            });
                            num_imported = num_imported.checked_add(1).ok_or_else(|| {
                                exec_error::inconsistent("too many imported functions")
                            })?;
                        }
                    }
                }
                Payload::FunctionSection(reader) => {
                    for type_idx in reader {
                        func_types.push(type_idx.map_err(exec_error::malformed)?);
                    }
                }
                Payload::MemorySection(reader) => {
                    for mem in reader {
                        let mem = mem.map_err(exec_error::malformed)?;
                        let bytes = usize::try_from(mem.initial)
                            .ok()
                            .and_then(|pages| pages.checked_mul(PAGE_SIZE))
                            .ok_or_else(|| {
                                exec_error::inconsistent("initial memory size overflows")
                            })?;
                        memory = vec![0u8; bytes];
                        max_pages = mem.maximum;
                    }
                }
                Payload::GlobalSection(reader) => {
                    for global in reader {
                        globals.push(eval_const(
                            &global.map_err(exec_error::malformed)?.init_expr,
                        )?);
                    }
                }
                Payload::ExportSection(reader) => {
                    for export in reader {
                        let export = export.map_err(exec_error::malformed)?;
                        if matches!(export.kind, wasmparser::ExternalKind::Func) {
                            func_exports.push((export.name.into(), export.index));
                        }
                    }
                }
                Payload::DataSection(reader) => {
                    for data in reader {
                        let data = data.map_err(exec_error::malformed)?;
                        if let DataKind::Active { offset_expr, .. } = data.kind {
                            let offset = match eval_const(&offset_expr)? {
                                Word::I32(v) => v as usize,
                                Word::I64(v) => usize::try_from(v).map_err(|_| {
                                    exec_error::inconsistent("data offset overflows")
                                })?,
                            };
                            let end = offset
                                .checked_add(data.data.len())
                                .filter(|&end| end <= memory.len())
                                .ok_or(ExecuteError::Trap(crate::error::Trap::MemoryOutOfBounds))?;
                            memory[offset..end].copy_from_slice(data.data);
                        }
                    }
                }
                Payload::CodeSectionEntry(body) => {
                    let index = u32::try_from(funcs.len())
                        .ok()
                        .and_then(|i| num_imported.checked_add(i))
                        .ok_or_else(|| exec_error::inconsistent("too many functions"))?;
                    funcs.push(func_image(index, &func_types, &body)?);
                }
                _ => {}
            }
        }

        Ok(Self {
            types,
            func_types,
            num_imported,
            imports,
            globals,
            funcs,
            func_exports,
            memory,
            max_pages,
        })
    }

    /// The `(parameter count, result count)` of a function by index.
    pub(crate) fn func_arity(&self, func_idx: u32) -> Option<(u32, u32)> {
        let ty = self
            .types
            .get(*self.func_types.get(func_idx as usize)? as usize)?;
        Some((u32::try_from(ty.params.len()).ok()?, ty.results))
    }

    /// The `(input arity, result arity)` of a block type.
    pub(crate) fn block_arity(&self, blockty: &BlockType) -> Result<(u32, u32)> {
        match blockty {
            BlockType::Empty => Ok((0, 0)),
            BlockType::Type(_) => Ok((0, 1)),
            BlockType::FuncType(idx) => {
                let ty = self.types.get(*idx as usize).ok_or_else(|| {
                    exec_error::inconsistent("block references an undeclared type")
                })?;
                Ok((
                    u32::try_from(ty.params.len())
                        .map_err(|_| exec_error::inconsistent("block parameter count overflows"))?,
                    ty.results,
                ))
            }
        }
    }
}

fn fn_type(inner: &CompositeInnerType) -> Result<FnType> {
    match inner {
        CompositeInnerType::Func(ft) => Ok(FnType {
            params: ft
                .params()
                .iter()
                .copied()
                .map(val_ty)
                .collect::<Result<_>>()?,
            results: u32::try_from(ft.results().len())
                .map_err(|_| exec_error::inconsistent("result count overflows"))?,
        }),
        _ => Ok(FnType {
            params: Vec::new(),
            results: 0,
        }),
    }
}

fn func_image<'a>(
    index: u32,
    func_types: &[u32],
    body: &FunctionBody<'a>,
) -> Result<FuncImage<'a>> {
    let type_idx = *func_types
        .get(index as usize)
        .ok_or_else(|| exec_error::inconsistent("function references an undeclared type"))?;

    let declared = body
        .get_locals_reader()
        .map_err(exec_error::malformed)?
        .into_iter()
        .map(|local| {
            let (count, ty) = local.map_err(exec_error::malformed)?;
            Ok((0..count).map(move |_| val_ty(ty)))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Result<Vec<ValTy>>>()?;

    let mut reader = body.get_operators_reader().map_err(exec_error::malformed)?;
    let mut ops = Vec::new();
    while !reader.eof() {
        ops.push(reader.read().map_err(exec_error::malformed)?);
    }

    let ends = block_ends(&ops)?;

    Ok(FuncImage {
        func_index: index,
        type_idx,
        declared,
        ops,
        ends,
    })
}

/// Maps each structured block opening to its matching `end` program point.
fn block_ends(ops: &[Operator]) -> Result<Vec<u32>> {
    let mut ends = vec![0u32; ops.len()];
    let mut open: Vec<usize> = Vec::new();
    for (pc, op) in ops.iter().enumerate() {
        match op {
            Operator::Block { .. } | Operator::Loop { .. } | Operator::If { .. } => open.push(pc),
            Operator::End => {
                if let Some(start) = open.pop() {
                    ends[start] = u32::try_from(pc)
                        .map_err(|_| exec_error::inconsistent("function body too large"))?;
                }
            }
            _ => {}
        }
    }
    Ok(ends)
}

/// Evaluates a constant initializer expression to a single value. MVM's subset
/// admits only `i32.const` / `i64.const` initializers.
fn eval_const(expr: &ConstExpr) -> Result<Word> {
    let mut reader = expr.get_operators_reader();
    let value = match reader.read().map_err(exec_error::malformed)? {
        Operator::I32Const { value } => Word::I32(value as u32),
        Operator::I64Const { value } => Word::I64(value as u64),
        _ => return Err(exec_error::inconsistent("unsupported constant initializer")),
    };
    Ok(value)
}
