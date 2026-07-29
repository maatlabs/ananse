use wasmparser::{
    BlockType, CompositeInnerType, ConstExpr, DataKind, ExternalKind, FunctionBody, Imports,
    Parser, Payload, TypeRef, ValType,
};

use crate::{DecodeError, Felt, Instruction, Result, WASM32_PAGE_SIZE};

/// A WebAssembly integer value, held as its unsigned bit pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Word {
    /// A 32-bit value, stored as its unsigned bit pattern.
    I32(u32),
    /// A 64-bit value, stored as its unsigned bit pattern.
    I64(u64),
}

impl Word {
    /// Evaluates a constant initializer expression to a single value. Ananse's subset
    /// admits only `i32.const` / `i64.const` initializers.
    pub fn from_const_expr(expr: &ConstExpr) -> Result<Self> {
        let mut reader = expr.get_operators_reader();
        let value = match reader.read().map_err(DecodeError::invalid_binary)? {
            Instruction::I32Const { value } => Self::I32(value as u32),
            Instruction::I64Const { value } => Self::I64(value as u64),
            _ => {
                return Err(DecodeError::internal("unsupported constant initializer"));
            }
        };
        Ok(value)
    }

    /// This value's little-endian 32-bit limbs `(lo, hi)` as Goldilocks
    /// residues, with the value equal to `lo + hi * 2^32`.
    ///
    /// An `i32` occupies the low limb alone (`hi` is zero); an
    /// `i64` splits across both.
    pub fn to_limbs(self) -> (Felt, Felt) {
        let bits = match self {
            Self::I32(bits) => u64::from(bits),
            Self::I64(bits) => bits,
        };
        (Felt::new(bits & 0xFFFF_FFFF), Felt::new(bits >> 32))
    }

    /// Whether this value is non-zero, the WebAssembly truth value used by
    /// `if`, `br_if`, and `select`.
    pub fn is_true(self) -> bool {
        match self {
            Self::I32(bits) => bits != 0,
            Self::I64(bits) => bits != 0,
        }
    }

    /// The bit pattern widened to `u64` together with the value's bit width.
    pub fn raw(self) -> (u64, u32) {
        match self {
            Self::I32(bits) => (u64::from(bits), 32),
            Self::I64(bits) => (bits, 64),
        }
    }

    /// Returns the bit pattern of this value as `u32`.
    pub fn as_u32(self) -> u32 {
        match self {
            Self::I32(bits) => bits,
            Self::I64(bits) => bits as u32,
        }
    }
}

/// A value type in Ananse's integer subset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordType {
    I32,
    I64,
}

impl WordType {
    /// The zero value of this word type.
    pub fn zero(self) -> Word {
        match self {
            Self::I32 => Word::I32(0),
            Self::I64 => Word::I64(0),
        }
    }

    /// Convert from [`ValType`] to `Self`.
    pub fn from_val_ty(ty: ValType) -> Result<Self> {
        match ty {
            ValType::I32 => Ok(Self::I32),
            ValType::I64 => Ok(Self::I64),
            _ => Err(DecodeError::internal(
                "value type outside the integer subset",
            )),
        }
    }
}

/// Function type
pub struct FnType {
    /// List of parameters
    pub params: Vec<WordType>,
    /// Number of return values
    pub results: u32,
}

impl FnType {
    pub fn from_composite(c: &CompositeInnerType) -> Result<FnType> {
        match c {
            CompositeInnerType::Func(ft) => Ok(FnType {
                params: ft
                    .params()
                    .iter()
                    .copied()
                    .map(WordType::from_val_ty)
                    .collect::<Result<_>>()?,
                results: u32::try_from(ft.results().len())
                    .map_err(|_| DecodeError::internal("result count overflows"))?,
            }),
            _ => Ok(FnType {
                params: Vec::new(),
                results: 0,
            }),
        }
    }
}

/// A single `(module, name)` import declared by a [`Module`](crate::Module).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportEntry {
    /// The import's module namespace.
    pub module: String,
    /// The imported item's name.
    pub name: String,
}

/// A single export declared by a [`Module`](crate::Module).
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

/// An executable view of a validated module.
pub struct Image<'a> {
    pub types: Vec<FnType>,
    pub func_types: Vec<u32>,
    pub num_imported: u32,
    pub imports: Vec<ImportEntry>,
    pub globals: Vec<Word>,
    pub funcs: Vec<FuncImage<'a>>,
    pub func_exports: Vec<(String, u32)>,
    pub memory: Vec<u8>,
    pub max_pages: Option<u64>,
}

/// A defined function's executable image.
pub struct FuncImage<'a> {
    pub func_index: u32,
    pub type_idx: u32,
    pub declared: Vec<WordType>,
    pub instructions: Vec<Instruction<'a>>,
    /// `ends[pc]` is the `end` program point of the `block` / `loop` / `if` that
    /// opens at `pc`; `0` for every other program point.
    pub ends: Vec<u32>,
}

impl<'a> FuncImage<'a> {
    fn new(index: u32, func_types: &[u32], body: &FunctionBody<'a>) -> Result<Self> {
        let type_idx = *func_types
            .get(index as usize)
            .ok_or_else(|| DecodeError::internal("function references an undeclared type"))?;

        let declared = body
            .get_locals_reader()
            .map_err(DecodeError::invalid_binary)?
            .into_iter()
            .map(|local| {
                let (count, ty) = local.map_err(DecodeError::invalid_binary)?;
                Ok((0..count).map(move |_| WordType::from_val_ty(ty)))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Result<Vec<WordType>>>()?;

        let mut reader = body
            .get_operators_reader()
            .map_err(DecodeError::invalid_binary)?;
        let mut instructions = Vec::new();
        while !reader.eof() {
            instructions.push(reader.read().map_err(DecodeError::invalid_binary)?);
        }

        let ends = block_ends(&instructions)?;

        Ok(Self {
            func_index: index,
            type_idx,
            declared,
            instructions,
            ends,
        })
    }
}

impl<'a> Image<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
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
            match payload.map_err(DecodeError::invalid_binary)? {
                Payload::TypeSection(reader) => {
                    for rec in reader {
                        for sub in rec.map_err(DecodeError::invalid_binary)?.types() {
                            types.push(FnType::from_composite(&sub.composite_type.inner)?);
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
                            func_types.push(type_idx);
                            imports.push(ImportEntry {
                                module: import.module.into(),
                                name: import.name.into(),
                            });
                            num_imported = num_imported.checked_add(1).ok_or_else(|| {
                                DecodeError::internal("too many imported functions")
                            })?;
                        }
                    }
                }
                Payload::FunctionSection(reader) => {
                    for type_idx in reader {
                        func_types.push(type_idx.map_err(DecodeError::invalid_binary)?);
                    }
                }
                Payload::MemorySection(reader) => {
                    for mem in reader {
                        let mem = mem.map_err(DecodeError::invalid_binary)?;
                        let bytes = usize::try_from(mem.initial)
                            .ok()
                            .and_then(|pages| pages.checked_mul(WASM32_PAGE_SIZE))
                            .ok_or_else(|| {
                                DecodeError::internal("initial memory size overflows")
                            })?;
                        memory = vec![0u8; bytes];
                        max_pages = mem.maximum;
                    }
                }
                Payload::GlobalSection(reader) => {
                    for global in reader {
                        globals.push(Word::from_const_expr(
                            &global.map_err(DecodeError::invalid_binary)?.init_expr,
                        )?);
                    }
                }
                Payload::ExportSection(reader) => {
                    for export in reader {
                        let export = export.map_err(DecodeError::invalid_binary)?;
                        if matches!(export.kind, ExternalKind::Func) {
                            func_exports.push((export.name.into(), export.index));
                        }
                    }
                }
                Payload::DataSection(reader) => {
                    for data in reader {
                        let data = data.map_err(DecodeError::invalid_binary)?;
                        if let DataKind::Active { offset_expr, .. } = data.kind {
                            let offset = match Word::from_const_expr(&offset_expr)? {
                                Word::I32(v) => v as usize,
                                Word::I64(v) => usize::try_from(v)
                                    .map_err(|_| DecodeError::internal("data offset overflows"))?,
                            };
                            let end = offset
                                .checked_add(data.data.len())
                                .filter(|&end| end <= memory.len())
                                .ok_or(DecodeError::MemoryOutOfBounds)?;
                            memory[offset..end].copy_from_slice(data.data);
                        }
                    }
                }
                Payload::CodeSectionEntry(body) => {
                    let index = u32::try_from(funcs.len())
                        .ok()
                        .and_then(|i| num_imported.checked_add(i))
                        .ok_or_else(|| DecodeError::internal("too many functions"))?;
                    funcs.push(FuncImage::new(index, &func_types, &body)?);
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
    pub fn func_arity(&self, func_idx: u32) -> Option<(u32, u32)> {
        let ty = self
            .types
            .get(*self.func_types.get(func_idx as usize)? as usize)?;
        Some((u32::try_from(ty.params.len()).ok()?, ty.results))
    }

    /// The `(input arity, result arity)` of a block type.
    pub fn block_arity(&self, ty: &BlockType) -> Result<(u32, u32)> {
        match ty {
            BlockType::Empty => Ok((0, 0)),
            BlockType::Type(_) => Ok((0, 1)),
            BlockType::FuncType(idx) => {
                let ty = self
                    .types
                    .get(*idx as usize)
                    .ok_or_else(|| DecodeError::internal("block references an undeclared type"))?;
                Ok((
                    u32::try_from(ty.params.len())
                        .map_err(|_| DecodeError::internal("block parameter count overflows"))?,
                    ty.results,
                ))
            }
        }
    }
}

/// Maps each structured block opening to its matching `end` program point.
fn block_ends(instructions: &[Instruction]) -> Result<Vec<u32>> {
    let mut ends = vec![0u32; instructions.len()];
    let mut open: Vec<usize> = Vec::new();
    for (pc, inst) in instructions.iter().enumerate() {
        match inst {
            Instruction::Block { .. } | Instruction::Loop { .. } | Instruction::If { .. } => {
                open.push(pc)
            }
            Instruction::End => {
                if let Some(start) = open.pop() {
                    ends[start] = u32::try_from(pc)
                        .map_err(|_| DecodeError::internal("function body too large"))?;
                }
            }
            _ => {}
        }
    }
    Ok(ends)
}
