/// Represents a WebAssembly function signature.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FuncType {
    /// Parameter types for the function.
    pub params: Vec<ValueType>,
    /// Result types returned by the function.
    pub results: Vec<ValueType>,
}

/// WebAssembly value types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueType {
    /// 32-bit integer (encoded as 0x7F).
    I32,
    /// 64-bit integer (encoded as 0x7E).
    I64,
}

impl TryFrom<u8> for ValueType {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x7F => Ok(Self::I32),
            0x7E => Ok(Self::I64),
            _ => Err(value),
        }
    }
}

/// Declares local variables within a function body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionLocal {
    /// Number of locals of this type.
    pub type_count: u32,
    /// The type of these local variables.
    pub value_type: ValueType,
}

/// Describes what kind of entity is being exported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportDesc {
    /// Export a function by its index.
    Func(u32),
}

/// A named export from a WebAssembly module.
#[derive(Debug, PartialEq, Eq)]
pub struct Export {
    /// The export name.
    pub name: String,
    /// The exported entity descriptor.
    pub desc: ExportDesc,
}

/// Describes what kind of entity is being imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportDesc {
    /// Import a function with the given type index.
    Func(u32),
}

/// An import declaration for a WebAssembly module.
#[derive(Debug, PartialEq, Eq)]
pub struct Import {
    /// The module name to import from.
    pub module: String,
    /// The field name within the module.
    pub field: String,
    /// The imported entity descriptor.
    pub desc: ImportDesc,
}

/// Declares a linear memory in the module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Memory {
    /// Size constraints for the memory.
    pub limits: Limits,
}

/// Size constraints for memories and tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Minimum size in pages (64 KiB each).
    pub min: u32,
    /// Optional maximum size in pages.
    pub max: Option<u32>,
}

/// A data segment for initializing linear memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Data {
    /// Index of the memory to initialize.
    pub memory_index: u32,
    /// Byte offset within the memory.
    pub offset: u32,
    /// The initialization data.
    pub init: Vec<u8>,
}

/// A structured control flow block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// The block's type signature.
    pub block_type: BlockType,
}

/// The type signature of a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockType {
    /// Block returns no values.
    Void,
    /// Block returns values of the specified types.
    Value(Vec<ValueType>),
}

impl BlockType {
    /// Returns the number of result values for this block type.
    pub fn result_count(&self) -> usize {
        match self {
            Self::Void => 0,
            Self::Value(vt) => vt.len(),
        }
    }
}
