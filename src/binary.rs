pub mod instruction;
pub mod module;
pub mod opcode;
pub mod section;
pub mod types;

pub use instruction::Instruction;
pub use module::Module;
pub use opcode::Opcode;
pub use section::{Function, SectionCode};
pub use types::{
    Data, Export, ExportDesc, FuncType, FunctionLocal, Import, ImportDesc, Limits, Memory,
    ValueType,
};
