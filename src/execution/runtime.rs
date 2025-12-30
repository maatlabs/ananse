use super::{
    ExternalFuncInst, FuncInst, Import, InternalFuncInst, Label, LabelKind, Store, Value,
    WasiSnapshotPreview1,
};
use crate::binary::instruction::Instruction;
use crate::binary::module::Module;
use crate::binary::types::{ExportDesc, ValueType};

/// A call frame representing a function invocation.
#[derive(Default)]
pub struct Frame {
    /// Program counter (current instruction index).
    pub pc: isize,
    /// Stack pointer at frame entry.
    pub sp: usize,
    /// The function's instructions.
    pub insts: Vec<Instruction>,
    /// Number of return values expected.
    pub arity: usize,
    /// Active control flow labels.
    pub labels: Vec<Label>,
    /// Local variables including parameters.
    pub locals: Vec<Value>,
}

/// The WebAssembly runtime executor.
#[derive(Default)]
pub struct Runtime {
    /// The module store with functions and memory.
    pub store: Store,
    /// The operand stack.
    pub stack: Vec<Value>,
    /// The call stack of active frames.
    pub call_stack: Vec<Frame>,
    /// Registered import functions.
    pub import: Import,
    /// Optional WASI interface.
    pub wasi: Option<WasiSnapshotPreview1>,
}

impl Runtime {
    /// Creates a runtime from a WebAssembly binary.
    ///
    /// # Errors
    ///
    /// Returns an error if the binary cannot be parsed or instantiated.
    pub fn instantiate(wasm: impl AsRef<[u8]>) -> anyhow::Result<Self> {
        let module = Module::new(wasm.as_ref())?;
        let store = Store::new(module)?;
        Ok(Self {
            store,
            ..Default::default()
        })
    }

    /// Creates a runtime with WASI support from a WebAssembly binary.
    ///
    /// # Errors
    ///
    /// Returns an error if the binary cannot be parsed or instantiated.
    pub fn instantiate_with_wasi(
        wasm: impl AsRef<[u8]>,
        wasi: WasiSnapshotPreview1,
    ) -> anyhow::Result<Self> {
        let module = Module::new(wasm.as_ref())?;
        let store = Store::new(module)?;
        Ok(Self {
            store,
            wasi: Some(wasi),
            ..Default::default()
        })
    }

    /// Registers an import function.
    ///
    /// # Errors
    ///
    /// Currently always succeeds.
    pub fn add_import(
        &mut self,
        module_name: impl Into<String>,
        func_name: impl Into<String>,
        func: impl FnMut(&mut Store, Vec<Value>) -> anyhow::Result<Option<Value>> + 'static,
    ) -> anyhow::Result<()> {
        let import = self.import.entry(module_name.into()).or_default();
        import.insert(func_name.into(), Box::new(func));
        Ok(())
    }

    /// Calls an exported function by name with the given arguments.
    ///
    /// # Errors
    ///
    /// Returns an error if the function is not found or execution fails.
    pub fn call(
        &mut self,
        name: impl Into<String>,
        args: Vec<Value>,
    ) -> anyhow::Result<Option<Value>> {
        let idx = match self
            .store
            .module
            .exports
            .get(&name.into())
            .ok_or(anyhow::anyhow!("not found export function"))?
            .desc
        {
            ExportDesc::Func(idx) => idx as usize,
        };
        let Some(func_inst) = self.store.funcs.get(idx) else {
            anyhow::bail!("not found func");
        };
        for arg in args {
            self.stack.push(arg);
        }
        match func_inst {
            FuncInst::Internal(func) => self.invoke_internal(func.clone()),
            FuncInst::External(func) => self.invoke_external(func.clone()),
        }
    }

    fn invoke_internal(&mut self, func: InternalFuncInst) -> anyhow::Result<Option<Value>> {
        let arity = func.func_type.results.len();

        self.push_frame(&func);

        if let Err(e) = self.execute() {
            self.cleanup();
            anyhow::bail!("failed to execute instructions: {e}");
        }

        if arity > 0 {
            let Some(value) = self.stack.pop() else {
                anyhow::bail!("not found return value");
            };
            return Ok(Some(value));
        }
        Ok(None)
    }

    fn invoke_external(&mut self, func: ExternalFuncInst) -> anyhow::Result<Option<Value>> {
        let args = self
            .stack
            .split_off(self.stack.len() - func.func_type.params.len());

        if func.module == "wasi_snapshot_preview1"
            && let Some(wasi) = &mut self.wasi
        {
            return wasi.invoke(&mut self.store, &func.func, args);
        }

        let module = self
            .import
            .get_mut(&func.module)
            .ok_or(anyhow::anyhow!("not found module"))?;
        let import_func = module
            .get_mut(&func.func)
            .ok_or(anyhow::anyhow!("not found function"))?;

        import_func(&mut self.store, args)
    }

    fn push_frame(&mut self, func: &InternalFuncInst) {
        let bottom = self.stack.len() - func.func_type.params.len();
        let mut locals = self.stack.split_off(bottom);

        for local in func.code.locals.iter() {
            match local {
                ValueType::I32 => locals.push(Value::I32(0)),
                ValueType::I64 => locals.push(Value::I64(0)),
            }
        }

        let arity = func.func_type.results.len();

        let frame = Frame {
            pc: -1,
            sp: self.stack.len(),
            insts: func.code.body.clone(),
            arity,
            labels: vec![],
            locals,
        };

        self.call_stack.push(frame);
    }

    fn execute(&mut self) -> anyhow::Result<()> {
        loop {
            let Some(frame) = self.call_stack.last_mut() else {
                break;
            };

            frame.pc += 1;

            let Some(inst) = frame.insts.get(frame.pc as usize) else {
                break;
            };

            match inst {
                Instruction::If(block) => {
                    let cond = self
                        .stack
                        .pop()
                        .ok_or(anyhow::anyhow!("not found value in the stack"))?;

                    let next_pc = get_end_address(&frame.insts, frame.pc as usize)?;
                    if cond == Value::I32(0) {
                        frame.pc = next_pc as isize;
                    }

                    let label = Label {
                        kind: LabelKind::If,
                        pc: next_pc,
                        sp: self.stack.len(),
                        arity: block.block_type.result_count(),
                    };
                    frame.labels.push(label);
                }
                Instruction::End => match frame.labels.pop() {
                    Some(label) => {
                        let Label { pc, sp, arity, .. } = label;
                        frame.pc = pc as isize;
                        stack_unwind(&mut self.stack, sp, arity)?;
                    }
                    None => {
                        let frame = self
                            .call_stack
                            .pop()
                            .ok_or(anyhow::anyhow!("not found value in the stack"))?;
                        let Frame { sp, arity, .. } = frame;
                        stack_unwind(&mut self.stack, sp, arity)?;
                    }
                },
                Instruction::Return => {
                    let Some(frame) = self.call_stack.pop() else {
                        anyhow::bail!("not found frame")
                    };
                    let Frame { sp, arity, .. } = frame;
                    stack_unwind(&mut self.stack, sp, arity)?;
                }
                Instruction::LocalGet(idx) => {
                    let Some(value) = frame.locals.get(*idx as usize) else {
                        anyhow::bail!("not found local");
                    };
                    self.stack.push(*value);
                }
                Instruction::LocalSet(idx) => {
                    let Some(value) = self.stack.pop() else {
                        anyhow::bail!("not found value in the stack")
                    };
                    frame.locals[*idx as usize] = value;
                }
                Instruction::I32Store { align: _, offset } => {
                    let (Some(value), Some(addr)) = (self.stack.pop(), self.stack.pop()) else {
                        anyhow::bail!("not found any value in the stack")
                    };
                    let addr: i32 = addr
                        .try_into()
                        .map_err(|_| anyhow::anyhow!("type mismatch"))?;
                    let offset = (*offset) as usize;
                    let at = addr as usize + offset;
                    let end = at + std::mem::size_of::<i32>();
                    let memory = self
                        .store
                        .memories
                        .get_mut(0)
                        .ok_or(anyhow::anyhow!("not found memory"))?;
                    let value: i32 = value
                        .try_into()
                        .map_err(|_| anyhow::anyhow!("type mismatch"))?;
                    memory.data[at..end].copy_from_slice(&value.to_le_bytes());
                }
                Instruction::I32Const(value) => self.stack.push(Value::I32(*value)),
                Instruction::I32Add => {
                    let (Some(right), Some(left)) = (self.stack.pop(), self.stack.pop()) else {
                        anyhow::bail!("not found any value in the stack")
                    };
                    let result = left
                        .checked_add(right)
                        .ok_or(anyhow::anyhow!("type mismatch"))?;
                    self.stack.push(result);
                }
                Instruction::I32Sub => {
                    let (Some(right), Some(left)) = (self.stack.pop(), self.stack.pop()) else {
                        anyhow::bail!("not found any value in the stack")
                    };
                    let result = left
                        .checked_sub(right)
                        .ok_or(anyhow::anyhow!("type mismatch"))?;
                    self.stack.push(result);
                }
                Instruction::I32Lts => {
                    let (Some(right), Some(left)) = (self.stack.pop(), self.stack.pop()) else {
                        anyhow::bail!("not found any value in the stack")
                    };
                    let result = left
                        .partial_cmp(&right)
                        .ok_or(anyhow::anyhow!("type mismatch"))?
                        .is_lt();
                    self.stack.push(result.into());
                }
                Instruction::Call(idx) => {
                    let Some(func) = self.store.funcs.get(*idx as usize) else {
                        anyhow::bail!("not found func");
                    };
                    let func_inst = func.clone();
                    match func_inst {
                        FuncInst::Internal(func) => self.push_frame(&func),
                        FuncInst::External(func) => {
                            if let Some(value) = self.invoke_external(func)? {
                                self.stack.push(value);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn cleanup(&mut self) {
        self.stack.clear();
        self.call_stack.clear();
    }
}

pub fn stack_unwind(stack: &mut Vec<Value>, sp: usize, arity: usize) -> anyhow::Result<()> {
    if arity > 0 {
        let Some(value) = stack.pop() else {
            anyhow::bail!("not found return value");
        };
        stack.drain(sp..);
        stack.push(value);
    } else {
        stack.drain(sp..);
    }
    Ok(())
}

pub fn get_end_address(insts: &[Instruction], pc: usize) -> anyhow::Result<usize> {
    let mut pc = pc;
    let mut depth = 0;
    loop {
        pc += 1;
        let inst = insts
            .get(pc)
            .ok_or(anyhow::anyhow!("not found instructions"))?;
        match inst {
            Instruction::If(_) => {
                depth += 1;
            }
            Instruction::End => {
                if depth == 0 {
                    return Ok(pc);
                } else {
                    depth -= 1;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_i32_add() -> anyhow::Result<()> {
        let wasm = wat::parse_file("fixtures/func_add.wat")?;
        let mut runtime = Runtime::instantiate(wasm)?;
        let cases = vec![(2, 3, 5), (10, 5, 15), (1, 1, 2)];

        for (left, right, want) in cases {
            let args = vec![Value::I32(left), Value::I32(right)];
            let result = runtime.call("add", args)?;
            assert_eq!(result, Some(Value::I32(want)));
        }
        Ok(())
    }

    #[test]
    fn execute_nonexistent_export_func() -> anyhow::Result<()> {
        let wasm = wat::parse_file("fixtures/func_add.wat")?;
        let mut runtime = Runtime::instantiate(wasm)?;
        let result = runtime.call("foobar", vec![]);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn func_call() -> anyhow::Result<()> {
        let wasm = wat::parse_file("fixtures/func_call.wat")?;
        let mut runtime = Runtime::instantiate(wasm)?;
        let cases = vec![(2, 4), (10, 20), (1, 2)];

        for (arg, want) in cases {
            let args = vec![Value::I32(arg)];
            let result = runtime.call("call_doubler", args)?;
            assert_eq!(result, Some(Value::I32(want)));
        }
        Ok(())
    }

    #[test]
    fn call_imported_func() -> anyhow::Result<()> {
        let wasm = wat::parse_file("fixtures/import.wat")?;
        let mut runtime = Runtime::instantiate(wasm)?;
        runtime.add_import("env", "add", |_, args| {
            let arg = args[0];
            Ok(Some(arg + arg))
        })?;

        let cases = vec![(2, 4), (10, 20), (1, 2)];

        for (arg, want) in cases {
            let args = vec![Value::I32(arg)];
            let result = runtime.call("call_add", args)?;
            assert_eq!(result, Some(Value::I32(want)));
        }
        Ok(())
    }

    #[test]
    fn call_imported_func_not_found() -> anyhow::Result<()> {
        let wasm = wat::parse_file("fixtures/import.wat")?;
        let mut runtime = Runtime::instantiate(wasm)?;
        runtime.add_import("env", "foo", |_, _| Ok(None))?;
        let result = runtime.call("call_add", vec![Value::I32(1)]);
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn i32_const() -> anyhow::Result<()> {
        let wasm = wat::parse_file("fixtures/i32_const.wat")?;
        let mut runtime = Runtime::instantiate(wasm)?;
        let result = runtime.call("i32_const", vec![])?;
        assert_eq!(result, Some(Value::I32(42)));
        Ok(())
    }

    #[test]
    fn local_set() -> anyhow::Result<()> {
        let wasm = wat::parse_file("fixtures/local_set.wat")?;
        let mut runtime = Runtime::instantiate(wasm)?;
        let result = runtime.call("local_set", vec![])?;
        assert_eq!(result, Some(Value::I32(42)));
        Ok(())
    }

    #[test]
    fn i32_store() -> anyhow::Result<()> {
        let wasm = wat::parse_file("fixtures/i32_store.wat")?;
        let mut runtime = Runtime::instantiate(wasm)?;
        runtime.call("i32_store", vec![])?;
        let memory = &runtime.store.memories[0].data;
        assert_eq!(memory[0], 42);
        Ok(())
    }

    #[test]
    fn i32_sub() -> anyhow::Result<()> {
        let wasm = wat::parse_file("fixtures/func_sub.wat")?;
        let mut runtime = Runtime::instantiate(wasm)?;
        let result = runtime.call("sub", vec![Value::I32(10), Value::I32(5)])?;
        assert_eq!(result, Some(Value::I32(5)));
        Ok(())
    }

    #[test]
    fn i32_lts() -> anyhow::Result<()> {
        let wasm = wat::parse_file("fixtures/func_lts.wat")?;
        let mut runtime = Runtime::instantiate(wasm)?;
        let result = runtime.call("lts", vec![Value::I32(10), Value::I32(5)])?;
        assert_eq!(result, Some(Value::I32(0)));
        Ok(())
    }

    #[test]
    fn fib() -> anyhow::Result<()> {
        let wasm = wat::parse_file("fixtures/fibonacci.wat")?;
        let mut runtime = Runtime::instantiate(wasm)?;
        let cases = vec![
            (1, 1),
            (2, 2),
            (3, 3),
            (4, 5),
            (5, 8),
            (6, 13),
            (7, 21),
            (8, 34),
            (9, 55),
            (10, 89),
        ];

        for (arg, want) in cases {
            let args = vec![Value::I32(arg)];
            let result = runtime.call("fib", args)?;
            assert_eq!(result, Some(Value::I32(want)));
        }

        Ok(())
    }
}
