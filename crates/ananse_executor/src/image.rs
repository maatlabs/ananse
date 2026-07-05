use ananse_decoder::{ImportEntry, Module};
use wasmparser::{
    BlockType, CompositeInnerType, ConstExpr, DataKind, FunctionBody, Imports, Operator, Parser,
    Payload, TypeRef, ValType,
};

use crate::{Entry, ExecuteError, OpCode, Result, Word, error as exec_error};

/// Bytes per WebAssembly memory page.
pub(crate) const PAGE_SIZE: usize = 65536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordType {
    I32,
    I64,
}

pub fn entry_parameters(module: &Module, entry: &Entry) -> Result<Vec<WordType>> {
    let image = Image::parse(module.bytes())?;
    let Some(func_index) = crate::interp::resolve_entry(&image, entry)? else {
        return Ok(Vec::new());
    };
    let type_idx = *image
        .func_types
        .get(func_index as usize)
        .ok_or(ExecuteError::UndefinedEntry)?;
    let ty = image
        .types
        .get(type_idx as usize)
        .ok_or(ExecuteError::UndefinedEntry)?;
    Ok(ty
        .params
        .iter()
        .map(|&param| match param {
            ValTy::I32 => WordType::I32,
            ValTy::I64 => WordType::I64,
        })
        .collect())
}

pub fn function_opcodes(module: &Module, func_index: u32) -> Result<Vec<OpCode>> {
    let image = Image::parse(module.bytes())?;
    let func = image
        .funcs
        .iter()
        .find(|f| f.func_index == func_index)
        .ok_or_else(|| exec_error::inconsistent("no defined function for the requested index"))?;
    func.ops.iter().map(operator_to_opcode).collect()
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
        .ok_or_else(|| exec_error::inconsistent("no defined function for the requested index"))?;
    Ok(func.ops.iter().map(operator_constant).collect())
}

fn operator_constant(op: &Operator) -> Option<u64> {
    match op {
        Operator::I32Const { value } => Some(u64::from(*value as u32)),
        Operator::I64Const { value } => Some(*value as u64),
        _ => None,
    }
}

fn operator_to_opcode(op: &Operator) -> Result<OpCode> {
    let opcode = match op {
        Operator::Unreachable => OpCode::Unreachable,
        Operator::Nop => OpCode::Nop,
        Operator::Block { .. } => OpCode::Block,
        Operator::Loop { .. } => OpCode::Loop,
        Operator::If { .. } => OpCode::If,
        Operator::Else => OpCode::Else,
        Operator::End => OpCode::End,
        Operator::Br { .. } => OpCode::Br,
        Operator::BrIf { .. } => OpCode::BrIf,
        Operator::BrTable { .. } => OpCode::BrTable,
        Operator::Return => OpCode::Return,
        Operator::Call { .. } => OpCode::Call,
        Operator::Drop => OpCode::Drop,
        Operator::Select => OpCode::Select,
        Operator::LocalGet { .. } => OpCode::LocalGet,
        Operator::LocalSet { .. } => OpCode::LocalSet,
        Operator::LocalTee { .. } => OpCode::LocalTee,
        Operator::GlobalGet { .. } => OpCode::GlobalGet,
        Operator::GlobalSet { .. } => OpCode::GlobalSet,
        Operator::I32Const { .. } => OpCode::I32Const,
        Operator::I64Const { .. } => OpCode::I64Const,
        Operator::I32Load { .. } => OpCode::I32Load,
        Operator::I64Load { .. } => OpCode::I64Load,
        Operator::I32Load8S { .. } => OpCode::I32Load8S,
        Operator::I32Load8U { .. } => OpCode::I32Load8U,
        Operator::I32Load16S { .. } => OpCode::I32Load16S,
        Operator::I32Load16U { .. } => OpCode::I32Load16U,
        Operator::I64Load8S { .. } => OpCode::I64Load8S,
        Operator::I64Load8U { .. } => OpCode::I64Load8U,
        Operator::I64Load16S { .. } => OpCode::I64Load16S,
        Operator::I64Load16U { .. } => OpCode::I64Load16U,
        Operator::I64Load32S { .. } => OpCode::I64Load32S,
        Operator::I64Load32U { .. } => OpCode::I64Load32U,
        Operator::I32Store { .. } => OpCode::I32Store,
        Operator::I64Store { .. } => OpCode::I64Store,
        Operator::I32Store8 { .. } => OpCode::I32Store8,
        Operator::I32Store16 { .. } => OpCode::I32Store16,
        Operator::I64Store8 { .. } => OpCode::I64Store8,
        Operator::I64Store16 { .. } => OpCode::I64Store16,
        Operator::I64Store32 { .. } => OpCode::I64Store32,
        Operator::MemorySize { .. } => OpCode::MemorySize,
        Operator::MemoryGrow { .. } => OpCode::MemoryGrow,
        Operator::I32Eqz => OpCode::I32Eqz,
        Operator::I32Eq => OpCode::I32Eq,
        Operator::I32Ne => OpCode::I32Ne,
        Operator::I32LtS => OpCode::I32LtS,
        Operator::I32LtU => OpCode::I32LtU,
        Operator::I32GtS => OpCode::I32GtS,
        Operator::I32GtU => OpCode::I32GtU,
        Operator::I32LeS => OpCode::I32LeS,
        Operator::I32LeU => OpCode::I32LeU,
        Operator::I32GeS => OpCode::I32GeS,
        Operator::I32GeU => OpCode::I32GeU,
        Operator::I64Eqz => OpCode::I64Eqz,
        Operator::I64Eq => OpCode::I64Eq,
        Operator::I64Ne => OpCode::I64Ne,
        Operator::I64LtS => OpCode::I64LtS,
        Operator::I64LtU => OpCode::I64LtU,
        Operator::I64GtS => OpCode::I64GtS,
        Operator::I64GtU => OpCode::I64GtU,
        Operator::I64LeS => OpCode::I64LeS,
        Operator::I64LeU => OpCode::I64LeU,
        Operator::I64GeS => OpCode::I64GeS,
        Operator::I64GeU => OpCode::I64GeU,
        Operator::I32Clz => OpCode::I32Clz,
        Operator::I32Ctz => OpCode::I32Ctz,
        Operator::I32Popcnt => OpCode::I32Popcnt,
        Operator::I32Add => OpCode::I32Add,
        Operator::I32Sub => OpCode::I32Sub,
        Operator::I32Mul => OpCode::I32Mul,
        Operator::I32DivS => OpCode::I32DivS,
        Operator::I32DivU => OpCode::I32DivU,
        Operator::I32RemS => OpCode::I32RemS,
        Operator::I32RemU => OpCode::I32RemU,
        Operator::I32And => OpCode::I32And,
        Operator::I32Or => OpCode::I32Or,
        Operator::I32Xor => OpCode::I32Xor,
        Operator::I32Shl => OpCode::I32Shl,
        Operator::I32ShrS => OpCode::I32ShrS,
        Operator::I32ShrU => OpCode::I32ShrU,
        Operator::I32Rotl => OpCode::I32Rotl,
        Operator::I32Rotr => OpCode::I32Rotr,
        Operator::I64Clz => OpCode::I64Clz,
        Operator::I64Ctz => OpCode::I64Ctz,
        Operator::I64Popcnt => OpCode::I64Popcnt,
        Operator::I64Add => OpCode::I64Add,
        Operator::I64Sub => OpCode::I64Sub,
        Operator::I64Mul => OpCode::I64Mul,
        Operator::I64DivS => OpCode::I64DivS,
        Operator::I64DivU => OpCode::I64DivU,
        Operator::I64RemS => OpCode::I64RemS,
        Operator::I64RemU => OpCode::I64RemU,
        Operator::I64And => OpCode::I64And,
        Operator::I64Or => OpCode::I64Or,
        Operator::I64Xor => OpCode::I64Xor,
        Operator::I64Shl => OpCode::I64Shl,
        Operator::I64ShrS => OpCode::I64ShrS,
        Operator::I64ShrU => OpCode::I64ShrU,
        Operator::I64Rotl => OpCode::I64Rotl,
        Operator::I64Rotr => OpCode::I64Rotr,
        Operator::I32WrapI64 => OpCode::I32WrapI64,
        Operator::I64ExtendI32S => OpCode::I64ExtendI32S,
        Operator::I64ExtendI32U => OpCode::I64ExtendI32U,
        other => {
            return Err(ExecuteError::UnsupportedOperator {
                offset: 0,
                message: format!("{other:?}"),
            });
        }
    };
    Ok(opcode)
}

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

pub(crate) struct FnType {
    pub(crate) params: Vec<ValTy>,
    pub(crate) results: u32,
}

/// A defined function's executable image.
pub(crate) struct FuncImage<'a> {
    pub(crate) func_index: u32,
    pub(crate) type_idx: u32,
    pub(crate) declared: Vec<ValTy>,
    pub(crate) ops: Vec<Operator<'a>>,
    /// `ends[pc]` is the `end` program point of the `block` / `loop` / `if` that
    /// opens at `pc`; `0` for every other program point.
    pub(crate) ends: Vec<u32>,
}

/// A value type in Ananse's integer subset.
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

fn eval_const(expr: &ConstExpr) -> Result<Word> {
    let mut reader = expr.get_operators_reader();
    let value = match reader.read().map_err(exec_error::malformed)? {
        Operator::I32Const { value } => Word::I32(value as u32),
        Operator::I64Const { value } => Word::I64(value as u64),
        _ => return Err(exec_error::inconsistent("unsupported constant initializer")),
    };
    Ok(value)
}
