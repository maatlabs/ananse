#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FuncType {
    pub params: Vec<ValueType>,
    pub results: Vec<ValueType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueType {
    I32, // 0x7F
    I64, // 0x7E
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionLocal {
    pub type_count: u32,
    pub value_type: ValueType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportDesc {
    Func(u32),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Export {
    pub name: String,
    pub desc: ExportDesc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportDesc {
    Func(u32),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Import {
    pub module: String,
    pub field: String,
    pub desc: ImportDesc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Memory {
    pub limits: Limits,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    pub min: u32,
    pub max: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Data {
    pub memory_index: u32,
    pub offset: u32,
    pub init: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub block_type: BlockType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockType {
    Void,
    Value(Vec<ValueType>),
}

impl BlockType {
    pub fn result_count(&self) -> usize {
        match self {
            Self::Void => 0,
            Self::Value(vt) => vt.len(),
        }
    }
}
