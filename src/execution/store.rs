use std::collections::HashMap;

use crate::binary::instruction::Instruction;
use crate::binary::module::Module;
use crate::binary::types::{ExportDesc, FuncType, ImportDesc, ValueType};

/// Size of a WebAssembly memory page in bytes (64 KiB).
pub const PAGE_SIZE: u32 = 65536;

/// A function's code representation at runtime.
#[derive(Debug, Clone)]
pub struct Func {
    /// Local variable types.
    pub locals: Vec<ValueType>,
    /// The instruction sequence.
    pub body: Vec<Instruction>,
}

/// An internal (WASM-defined) function instance.
#[derive(Debug, Clone)]
pub struct InternalFuncInst {
    /// The function's type signature.
    pub func_type: FuncType,
    /// The function's code.
    pub code: Func,
}

/// An external (imported) function instance.
#[derive(Debug, Clone)]
pub struct ExternalFuncInst {
    /// The module name this function is imported from.
    pub module: String,
    /// The function name within the module.
    pub func: String,
    /// The function's type signature.
    pub func_type: FuncType,
}

/// A function instance, either internal or external.
#[derive(Debug, Clone)]
pub enum FuncInst {
    /// A function defined in the WASM module.
    Internal(InternalFuncInst),
    /// A function imported from the host.
    External(ExternalFuncInst),
}

/// An instantiated export.
#[derive(Debug)]
pub struct ExportInst {
    /// The export name.
    pub name: String,
    /// The exported entity descriptor.
    pub desc: ExportDesc,
}

/// An instantiated module with its exports.
#[derive(Debug, Default)]
pub struct ModuleInst {
    /// Map of export names to export instances.
    pub exports: HashMap<String, ExportInst>,
}

/// A linear memory instance.
#[derive(Debug, Default, Clone)]
pub struct MemoryInst {
    /// The memory contents.
    pub data: Vec<u8>,
    /// Optional maximum size in pages.
    pub max: Option<u32>,
}

/// The runtime store containing all instantiated module data.
#[derive(Debug, Default)]
pub struct Store {
    /// All function instances.
    pub funcs: Vec<FuncInst>,
    /// All memory instances.
    pub memories: Vec<MemoryInst>,
    /// The module instance with exports.
    pub module: ModuleInst,
}

impl Store {
    /// Creates a new store from a parsed module.
    ///
    /// # Errors
    ///
    /// Returns an error if the module references missing types or memories.
    pub fn new(module: Module) -> anyhow::Result<Self> {
        let func_type_idxs = module.function_section.clone().unwrap_or_default();

        let mut funcs = vec![];
        let mut memories = vec![];

        if let Some(ref import_section) = module.import_section {
            for import in import_section {
                let module_name = import.module.clone();
                let field = import.field.clone();
                let func_type = match import.desc {
                    ImportDesc::Func(type_idx) => {
                        let Some(ref func_types) = module.type_section else {
                            anyhow::bail!("not found type_section")
                        };

                        let Some(func_type) = func_types.get(type_idx as usize) else {
                            anyhow::bail!("not found func type in type_section")
                        };

                        func_type.clone()
                    }
                };

                let func = FuncInst::External(ExternalFuncInst {
                    module: module_name,
                    func: field,
                    func_type,
                });
                funcs.push(func);
            }
        }

        if let Some(ref code_section) = module.code_section {
            for (func_body, type_idx) in code_section.iter().zip(func_type_idxs.into_iter()) {
                let Some(ref func_types) = module.type_section else {
                    anyhow::bail!("not found type_section")
                };

                let Some(func_type) = func_types.get(type_idx as usize) else {
                    anyhow::bail!("not found func type in type_section")
                };

                let mut locals = Vec::with_capacity(func_body.locals.len());
                for local in func_body.locals.iter() {
                    for _ in 0..local.type_count {
                        locals.push(local.value_type.clone());
                    }
                }

                let func = FuncInst::Internal(InternalFuncInst {
                    func_type: func_type.clone(),
                    code: Func {
                        locals,
                        body: func_body.code.clone(),
                    },
                });
                funcs.push(func);
            }
        }

        let mut exports = HashMap::default();
        if let Some(ref sections) = module.export_section {
            for export in sections {
                let name = export.name.clone();
                let export_inst = ExportInst {
                    name: name.clone(),
                    desc: export.desc.clone(),
                };
                exports.insert(name, export_inst);
            }
        }

        if let Some(ref sections) = module.memory_section {
            for memory in sections {
                let min = memory.limits.min * PAGE_SIZE;
                let memory = MemoryInst {
                    data: vec![0; min as usize],
                    max: memory.limits.max,
                };
                memories.push(memory);
            }
        }

        if let Some(ref sections) = module.data_section {
            for data in sections {
                let memory = memories
                    .get_mut(data.memory_index as usize)
                    .ok_or(anyhow::anyhow!("not found memory"))?;

                let offset = data.offset as usize;
                let init = &data.init;

                if offset + init.len() > memory.data.len() {
                    anyhow::bail!("data is too large to fit in memory");
                }
                memory.data[offset..offset + init.len()].copy_from_slice(init);
            }
        }

        Ok(Self {
            funcs,
            memories,
            module: ModuleInst { exports },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_memory() -> anyhow::Result<()> {
        let wasm = wat::parse_file("fixtures/memory.wat")?;
        let module = Module::new(&wasm)?;
        let store = Store::new(module)?;
        assert_eq!(store.memories.len(), 1);
        assert_eq!(store.memories[0].data.len(), 65536);
        assert_eq!(&store.memories[0].data[0..5], b"hello");
        assert_eq!(&store.memories[0].data[5..10], b"world");
        Ok(())
    }
}
