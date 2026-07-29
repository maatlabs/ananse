mod ops;
mod signal;

use ananse_decoder::{Image, Instruction, OpCode, Word};
use ananse_lift::{LiftedFunction, LiftedProgram, Register, Successors};
use ops::{Binary, Compare, Unary, binop, cmpop, sign_extend, unop};
pub use signal::Completion;
use signal::{Signal, StepOutcome};

use crate::runtime::{Label, Labels, Memory, OperandStack, Registers};
use crate::{
    ExecuteError, Host, HostAction, MemAccess, RegAccess, Result, StepObserver, StepRecord,
    Transition, Trap,
};

/// Maximum direct-call nesting before the executor reports
/// [`Trap::CallStackExhausted`], bounding host stack use.
const MAX_CALL_DEPTH: usize = 1024;

/// The mutable execution state: module-wide globals and linear memory, the
/// record sink, and the host environment.
pub struct Interpreter<'o, O: StepObserver, H: Host> {
    pub globals: Vec<Word>,
    pub memory: Memory,
    pub observer: &'o mut O,
    pub host: &'o mut H,
    pub steps: u64,
}

impl<O: StepObserver, H: Host> Interpreter<'_, O, H> {
    pub fn eval(
        &mut self,
        image: &Image,
        program: &LiftedProgram,
        defined: usize,
        args: Vec<Word>,
        depth: usize,
    ) -> Result<Completion> {
        if depth > MAX_CALL_DEPTH {
            return Err(Trap::CallStackExhausted.into());
        }

        let func_image = &image.funcs[defined];
        let lifted_func = &program.functions[defined];
        let func_index = func_image.func_index;
        let result_arity = image.types[func_image.type_idx as usize].results;

        let mut locals = args;
        locals.extend(func_image.declared.iter().map(|t| t.zero()));
        let mut stack = OperandStack::default();
        let mut labels = Labels::default();
        let mut pc = 0usize;

        loop {
            let instr = &func_image.instructions[pc];
            let sched = &lifted_func.schedules[pc];
            stack.expect_height(sched.height_in, func_index, pc as u32)?;

            let reads = resolve_reads(
                &Registers {
                    stack: stack.as_slice(),
                    locals: &locals,
                    globals: &self.globals,
                },
                &sched.reads,
            )?;

            let outcome = match instr {
                Instruction::Unreachable => return Err(Trap::Unreachable.into()),
                Instruction::Nop => StepOutcome::advance(OpCode::Nop, pc + 1),

                Instruction::Block { blockty } => {
                    let (_, out) = image.block_arity(blockty)?;
                    labels.push(Label {
                        is_loop: false,
                        branch_arity: out,
                        end_pc: func_image.ends[pc],
                    });
                    StepOutcome::advance(OpCode::Block, pc + 1)
                }
                Instruction::Loop { blockty } => {
                    let (input, _) = image.block_arity(blockty)?;
                    labels.push(Label {
                        is_loop: true,
                        branch_arity: input,
                        end_pc: func_image.ends[pc],
                    });
                    StepOutcome::advance(OpCode::Loop, pc + 1)
                }
                Instruction::If { blockty } => {
                    let (_, out) = image.block_arity(blockty)?;
                    let cond = stack.pop()?;
                    labels.push(Label {
                        is_loop: false,
                        branch_arity: out,
                        end_pc: func_image.ends[pc],
                    });
                    let Successors::Branch { taken, not_taken } = sched.successors else {
                        return Err(ExecuteError::invalid_binary(
                            "`if` without a branch successor",
                        ));
                    };
                    let next = if cond.is_true() { taken } else { not_taken } as usize;
                    labels.pop_past(next);
                    StepOutcome::advance(OpCode::If, next)
                }
                Instruction::Else => {
                    let Successors::Jump(target) = sched.successors else {
                        return Err(ExecuteError::invalid_binary(
                            "`else` without a jump successor",
                        ));
                    };
                    labels.pop_past(target as usize);
                    StepOutcome::advance(OpCode::Else, target as usize)
                }
                Instruction::End => match sched.successors {
                    Successors::Return => {
                        let results = stack.take_top(result_arity)?;
                        StepOutcome {
                            opcode: OpCode::End,
                            transition: Transition::Return,
                            mem: Vec::new(),
                            control: Signal::Return(results),
                        }
                    }
                    _ => {
                        labels.pop_past(pc + 1);
                        StepOutcome::advance(OpCode::End, pc + 1)
                    }
                },

                Instruction::Br { relative_depth } => {
                    let Successors::Jump(target) = sched.successors else {
                        return Err(ExecuteError::invalid_binary(
                            "`br` without a jump successor",
                        ));
                    };
                    let control = branch_to(
                        &mut stack,
                        &mut labels,
                        *relative_depth,
                        target,
                        lifted_func,
                        result_arity,
                    )?;
                    StepOutcome {
                        opcode: OpCode::Br,
                        transition: control.transition(),
                        mem: Vec::new(),
                        control,
                    }
                }
                Instruction::BrIf { relative_depth } => {
                    let cond = stack.pop()?;
                    let Successors::Branch { taken, not_taken } = sched.successors else {
                        return Err(ExecuteError::invalid_binary(
                            "`br_if` without a branch successor",
                        ));
                    };
                    if cond.is_true() {
                        let control = branch_to(
                            &mut stack,
                            &mut labels,
                            *relative_depth,
                            taken,
                            lifted_func,
                            result_arity,
                        )?;
                        StepOutcome {
                            opcode: OpCode::BrIf,
                            transition: control.transition(),
                            mem: Vec::new(),
                            control,
                        }
                    } else {
                        labels.pop_past(not_taken as usize);
                        StepOutcome::advance(OpCode::BrIf, not_taken as usize)
                    }
                }
                Instruction::BrTable { targets } => {
                    let selector = stack.pop_u32()? as usize;
                    let Successors::Table {
                        targets: target_pcs,
                        default,
                    } = &sched.successors
                    else {
                        return Err(ExecuteError::invalid_binary(
                            "`br_table` without a table successor",
                        ));
                    };
                    let depths = targets
                        .targets()
                        .collect::<core::result::Result<Vec<u32>, _>>()
                        .map_err(|e| ExecuteError::invalid_binary(&e.to_string()))?;
                    let (depth_to, target) = if selector < depths.len() {
                        (depths[selector], target_pcs[selector])
                    } else {
                        (targets.default(), *default)
                    };
                    let control = branch_to(
                        &mut stack,
                        &mut labels,
                        depth_to,
                        target,
                        lifted_func,
                        result_arity,
                    )?;
                    StepOutcome {
                        opcode: OpCode::BrTable,
                        transition: control.transition(),
                        mem: Vec::new(),
                        control,
                    }
                }
                Instruction::Return => {
                    let results = stack.take_top(result_arity)?;
                    StepOutcome {
                        opcode: OpCode::Return,
                        transition: Transition::Return,
                        mem: Vec::new(),
                        control: Signal::Return(results),
                    }
                }

                Instruction::Call { function_index } => {
                    let f = *function_index;
                    let (params, _) = image.func_arity(f).ok_or_else(|| {
                        ExecuteError::invalid_binary("call to an undeclared function")
                    })?;
                    let call_args = stack.take_top(params)?;
                    if f < image.num_imported {
                        let import = &image.imports[f as usize];
                        match self
                            .host
                            .call(import, &call_args, self.memory.as_mut_slice())?
                        {
                            HostAction::Return(results) => {
                                stack.extend(results);
                                StepOutcome::advance(OpCode::Call, pc + 1)
                            }
                            HostAction::Exit(code) => StepOutcome {
                                opcode: OpCode::Call,
                                transition: Transition::Exit(code),
                                mem: Vec::new(),
                                control: Signal::Exit(code),
                            },
                        }
                    } else {
                        let callee = (f - image.num_imported) as usize;
                        match self.eval(image, program, callee, call_args, depth + 1)? {
                            Completion::Return(results) => {
                                stack.extend(results);
                                StepOutcome {
                                    opcode: OpCode::Call,
                                    transition: Transition::Call {
                                        callee: f,
                                        return_pc: (pc + 1) as u32,
                                    },
                                    mem: Vec::new(),
                                    control: Signal::Advance(pc + 1),
                                }
                            }
                            Completion::Exit(code) => StepOutcome {
                                opcode: OpCode::Call,
                                transition: Transition::Exit(code),
                                mem: Vec::new(),
                                control: Signal::Exit(code),
                            },
                        }
                    }
                }
                Instruction::CallIndirect { .. } => {
                    return Err(ExecuteError::UnsupportedOperator {
                        offset: 0,
                        message: "call_indirect".into(),
                    });
                }

                Instruction::Drop => {
                    stack.pop()?;
                    StepOutcome::advance(OpCode::Drop, pc + 1)
                }
                Instruction::Select => {
                    let cond = stack.pop()?;
                    let rhs = stack.pop()?;
                    let lhs = stack.pop()?;
                    stack.push(if cond.is_true() { lhs } else { rhs });
                    StepOutcome::advance(OpCode::Select, pc + 1)
                }

                Instruction::LocalGet { local_index } => {
                    let value = *locals
                        .get(*local_index as usize)
                        .ok_or_else(|| ExecuteError::invalid_binary("local index out of range"))?;
                    stack.push(value);
                    StepOutcome::advance(OpCode::LocalGet, pc + 1)
                }
                Instruction::LocalSet { local_index } => {
                    let value = stack.pop()?;
                    *locals.get_mut(*local_index as usize).ok_or_else(|| {
                        ExecuteError::invalid_binary("local index out of range")
                    })? = value;
                    StepOutcome::advance(OpCode::LocalSet, pc + 1)
                }
                Instruction::LocalTee { local_index } => {
                    let value = *stack
                        .last()
                        .ok_or_else(|| ExecuteError::invalid_binary("operand stack underflow"))?;
                    *locals.get_mut(*local_index as usize).ok_or_else(|| {
                        ExecuteError::invalid_binary("local index out of range")
                    })? = value;
                    StepOutcome::advance(OpCode::LocalTee, pc + 1)
                }
                Instruction::GlobalGet { global_index } => {
                    let value = *self
                        .globals
                        .get(*global_index as usize)
                        .ok_or_else(|| ExecuteError::invalid_binary("global index out of range"))?;
                    stack.push(value);
                    StepOutcome::advance(OpCode::GlobalGet, pc + 1)
                }
                Instruction::GlobalSet { global_index } => {
                    let value = stack.pop()?;
                    *self
                        .globals
                        .get_mut(*global_index as usize)
                        .ok_or_else(|| {
                            ExecuteError::invalid_binary("global index out of range")
                        })? = value;
                    StepOutcome::advance(OpCode::GlobalSet, pc + 1)
                }

                Instruction::I32Const { value } => {
                    stack.push(Word::I32(*value as u32));
                    StepOutcome::advance(OpCode::I32Const, pc + 1)
                }
                Instruction::I64Const { value } => {
                    stack.push(Word::I64(*value as u64));
                    StepOutcome::advance(OpCode::I64Const, pc + 1)
                }

                Instruction::I32Load { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I32Load, pc)?
                }
                Instruction::I64Load { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I64Load, pc)?
                }
                Instruction::I32Load8S { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I32Load8S, pc)?
                }
                Instruction::I32Load8U { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I32Load8U, pc)?
                }
                Instruction::I32Load16S { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I32Load16S, pc)?
                }
                Instruction::I32Load16U { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I32Load16U, pc)?
                }
                Instruction::I64Load8S { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I64Load8S, pc)?
                }
                Instruction::I64Load8U { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I64Load8U, pc)?
                }
                Instruction::I64Load16S { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I64Load16S, pc)?
                }
                Instruction::I64Load16U { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I64Load16U, pc)?
                }
                Instruction::I64Load32S { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I64Load32S, pc)?
                }
                Instruction::I64Load32U { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I64Load32U, pc)?
                }

                Instruction::I32Store { memarg } => {
                    self.store(&mut stack, memarg.offset, 4, OpCode::I32Store, pc)?
                }
                Instruction::I64Store { memarg } => {
                    self.store(&mut stack, memarg.offset, 8, OpCode::I64Store, pc)?
                }
                Instruction::I32Store8 { memarg } => {
                    self.store(&mut stack, memarg.offset, 1, OpCode::I32Store8, pc)?
                }
                Instruction::I32Store16 { memarg } => {
                    self.store(&mut stack, memarg.offset, 2, OpCode::I32Store16, pc)?
                }
                Instruction::I64Store8 { memarg } => {
                    self.store(&mut stack, memarg.offset, 1, OpCode::I64Store8, pc)?
                }
                Instruction::I64Store16 { memarg } => {
                    self.store(&mut stack, memarg.offset, 2, OpCode::I64Store16, pc)?
                }
                Instruction::I64Store32 { memarg } => {
                    self.store(&mut stack, memarg.offset, 4, OpCode::I64Store32, pc)?
                }

                Instruction::MemorySize { .. } => self.size(&mut stack, pc),
                Instruction::MemoryGrow { .. } => self.grow(&mut stack, pc)?,

                Instruction::I32Eqz => unop(&mut stack, Unary::Eqz, OpCode::I32Eqz, pc)?,
                Instruction::I64Eqz => unop(&mut stack, Unary::Eqz, OpCode::I64Eqz, pc)?,
                Instruction::I32Clz => unop(&mut stack, Unary::Clz, OpCode::I32Clz, pc)?,
                Instruction::I32Ctz => unop(&mut stack, Unary::Ctz, OpCode::I32Ctz, pc)?,
                Instruction::I32Popcnt => unop(&mut stack, Unary::Popcnt, OpCode::I32Popcnt, pc)?,
                Instruction::I64Clz => unop(&mut stack, Unary::Clz, OpCode::I64Clz, pc)?,
                Instruction::I64Ctz => unop(&mut stack, Unary::Ctz, OpCode::I64Ctz, pc)?,
                Instruction::I64Popcnt => unop(&mut stack, Unary::Popcnt, OpCode::I64Popcnt, pc)?,

                Instruction::I32Add => binop(&mut stack, Binary::Add, OpCode::I32Add, pc)?,
                Instruction::I32Sub => binop(&mut stack, Binary::Sub, OpCode::I32Sub, pc)?,
                Instruction::I32Mul => binop(&mut stack, Binary::Mul, OpCode::I32Mul, pc)?,
                Instruction::I32DivS => binop(&mut stack, Binary::DivS, OpCode::I32DivS, pc)?,
                Instruction::I32DivU => binop(&mut stack, Binary::DivU, OpCode::I32DivU, pc)?,
                Instruction::I32RemS => binop(&mut stack, Binary::RemS, OpCode::I32RemS, pc)?,
                Instruction::I32RemU => binop(&mut stack, Binary::RemU, OpCode::I32RemU, pc)?,
                Instruction::I32And => binop(&mut stack, Binary::And, OpCode::I32And, pc)?,
                Instruction::I32Or => binop(&mut stack, Binary::Or, OpCode::I32Or, pc)?,
                Instruction::I32Xor => binop(&mut stack, Binary::Xor, OpCode::I32Xor, pc)?,
                Instruction::I32Shl => binop(&mut stack, Binary::Shl, OpCode::I32Shl, pc)?,
                Instruction::I32ShrS => binop(&mut stack, Binary::ShrS, OpCode::I32ShrS, pc)?,
                Instruction::I32ShrU => binop(&mut stack, Binary::ShrU, OpCode::I32ShrU, pc)?,
                Instruction::I32Rotl => binop(&mut stack, Binary::Rotl, OpCode::I32Rotl, pc)?,
                Instruction::I32Rotr => binop(&mut stack, Binary::Rotr, OpCode::I32Rotr, pc)?,
                Instruction::I64Add => binop(&mut stack, Binary::Add, OpCode::I64Add, pc)?,
                Instruction::I64Sub => binop(&mut stack, Binary::Sub, OpCode::I64Sub, pc)?,
                Instruction::I64Mul => binop(&mut stack, Binary::Mul, OpCode::I64Mul, pc)?,
                Instruction::I64DivS => binop(&mut stack, Binary::DivS, OpCode::I64DivS, pc)?,
                Instruction::I64DivU => binop(&mut stack, Binary::DivU, OpCode::I64DivU, pc)?,
                Instruction::I64RemS => binop(&mut stack, Binary::RemS, OpCode::I64RemS, pc)?,
                Instruction::I64RemU => binop(&mut stack, Binary::RemU, OpCode::I64RemU, pc)?,
                Instruction::I64And => binop(&mut stack, Binary::And, OpCode::I64And, pc)?,
                Instruction::I64Or => binop(&mut stack, Binary::Or, OpCode::I64Or, pc)?,
                Instruction::I64Xor => binop(&mut stack, Binary::Xor, OpCode::I64Xor, pc)?,
                Instruction::I64Shl => binop(&mut stack, Binary::Shl, OpCode::I64Shl, pc)?,
                Instruction::I64ShrS => binop(&mut stack, Binary::ShrS, OpCode::I64ShrS, pc)?,
                Instruction::I64ShrU => binop(&mut stack, Binary::ShrU, OpCode::I64ShrU, pc)?,
                Instruction::I64Rotl => binop(&mut stack, Binary::Rotl, OpCode::I64Rotl, pc)?,
                Instruction::I64Rotr => binop(&mut stack, Binary::Rotr, OpCode::I64Rotr, pc)?,

                Instruction::I32Eq => cmpop(&mut stack, Compare::Eq, OpCode::I32Eq, pc)?,
                Instruction::I32Ne => cmpop(&mut stack, Compare::Ne, OpCode::I32Ne, pc)?,
                Instruction::I32LtS => cmpop(&mut stack, Compare::LtS, OpCode::I32LtS, pc)?,
                Instruction::I32LtU => cmpop(&mut stack, Compare::LtU, OpCode::I32LtU, pc)?,
                Instruction::I32GtS => cmpop(&mut stack, Compare::GtS, OpCode::I32GtS, pc)?,
                Instruction::I32GtU => cmpop(&mut stack, Compare::GtU, OpCode::I32GtU, pc)?,
                Instruction::I32LeS => cmpop(&mut stack, Compare::LeS, OpCode::I32LeS, pc)?,
                Instruction::I32LeU => cmpop(&mut stack, Compare::LeU, OpCode::I32LeU, pc)?,
                Instruction::I32GeS => cmpop(&mut stack, Compare::GeS, OpCode::I32GeS, pc)?,
                Instruction::I32GeU => cmpop(&mut stack, Compare::GeU, OpCode::I32GeU, pc)?,
                Instruction::I64Eq => cmpop(&mut stack, Compare::Eq, OpCode::I64Eq, pc)?,
                Instruction::I64Ne => cmpop(&mut stack, Compare::Ne, OpCode::I64Ne, pc)?,
                Instruction::I64LtS => cmpop(&mut stack, Compare::LtS, OpCode::I64LtS, pc)?,
                Instruction::I64LtU => cmpop(&mut stack, Compare::LtU, OpCode::I64LtU, pc)?,
                Instruction::I64GtS => cmpop(&mut stack, Compare::GtS, OpCode::I64GtS, pc)?,
                Instruction::I64GtU => cmpop(&mut stack, Compare::GtU, OpCode::I64GtU, pc)?,
                Instruction::I64LeS => cmpop(&mut stack, Compare::LeS, OpCode::I64LeS, pc)?,
                Instruction::I64LeU => cmpop(&mut stack, Compare::LeU, OpCode::I64LeU, pc)?,
                Instruction::I64GeS => cmpop(&mut stack, Compare::GeS, OpCode::I64GeS, pc)?,
                Instruction::I64GeU => cmpop(&mut stack, Compare::GeU, OpCode::I64GeU, pc)?,

                Instruction::I32WrapI64 => {
                    let bits = match stack.pop()? {
                        Word::I64(b) => b,
                        Word::I32(b) => u64::from(b),
                    };
                    stack.push(Word::I32(bits as u32));
                    StepOutcome::advance(OpCode::I32WrapI64, pc + 1)
                }
                Instruction::I64ExtendI32S => {
                    let bits = match stack.pop()? {
                        Word::I32(b) => b,
                        Word::I64(b) => b as u32,
                    };
                    stack.push(Word::I64(i64::from(bits as i32) as u64));
                    StepOutcome::advance(OpCode::I64ExtendI32S, pc + 1)
                }
                Instruction::I64ExtendI32U => {
                    let bits = match stack.pop()? {
                        Word::I32(b) => b,
                        Word::I64(b) => b as u32,
                    };
                    stack.push(Word::I64(u64::from(bits)));
                    StepOutcome::advance(OpCode::I64ExtendI32U, pc + 1)
                }

                other => {
                    return Err(ExecuteError::UnsupportedOperator {
                        offset: 0,
                        message: format!("{other:?}"),
                    });
                }
            };

            // A `proc_exit` halts before its call's results reach the stack, so
            // the call's scheduled result writes have nothing to resolve.
            let writes = match outcome.control {
                Signal::Exit(_) => Vec::new(),
                _ => resolve_reads(
                    &Registers {
                        stack: stack.as_slice(),
                        locals: locals.as_slice(),
                        globals: &self.globals,
                    },
                    &sched.writes,
                )?,
            };
            self.steps += 1;
            self.observer.observe(StepRecord {
                func_index,
                pc: pc as u32,
                opcode: outcome.opcode,
                reads,
                writes,
                memory: outcome.mem,
                transition: outcome.transition,
            });
            match outcome.control {
                Signal::Advance(next) => pc = next,
                Signal::Return(results) => return Ok(Completion::Return(results)),
                Signal::Exit(code) => return Ok(Completion::Exit(code)),
            }
        }
    }

    fn load(
        &mut self,
        stack: &mut OperandStack,
        offset: u64,
        opcode: OpCode,
        pc: usize,
    ) -> Result<StepOutcome> {
        let (bytes, signed, result64) = opcode.load_shape();
        let base = stack.pop_u32()?;
        let (address, raw) = self.memory.read(base, offset, bytes)?;
        stack.push(sign_extend(raw, bytes, signed, result64));
        Ok(StepOutcome {
            opcode,
            transition: Transition::Next((pc + 1) as u32),
            mem: vec![MemAccess {
                address,
                width: bytes as u8,
                value: raw,
                store: false,
            }],
            control: Signal::Advance(pc + 1),
        })
    }

    fn store(
        &mut self,
        stack: &mut OperandStack,
        offset: u64,
        bytes: usize,
        opcode: OpCode,
        pc: usize,
    ) -> Result<StepOutcome> {
        let value = stack.pop()?;
        let base = stack.pop_u32()?;
        let bits = match value {
            Word::I32(v) => u64::from(v),
            Word::I64(v) => v,
        };
        let raw = if bytes == 8 {
            bits
        } else {
            bits & ((1u64 << (8 * bytes)) - 1)
        };
        let address = self.memory.write(base, offset, bytes, raw)?;
        Ok(StepOutcome {
            opcode,
            transition: Transition::Next((pc + 1) as u32),
            mem: vec![MemAccess {
                address,
                width: bytes as u8,
                value: raw,
                store: true,
            }],
            control: Signal::Advance(pc + 1),
        })
    }

    fn size(&self, stack: &mut OperandStack, pc: usize) -> StepOutcome {
        stack.push(Word::I32(self.memory.size_pages()));
        StepOutcome::advance(OpCode::MemorySize, pc + 1)
    }

    fn grow(&mut self, stack: &mut OperandStack, pc: usize) -> Result<StepOutcome> {
        let delta = stack.pop_u32()? as usize;
        let result = self.memory.grow(delta);
        stack.push(Word::I32(result));
        Ok(StepOutcome::advance(OpCode::MemoryGrow, pc + 1))
    }
}

/// Resolves a taken branch to `relative_depth`, unwinding the operand stack and
/// runtime control labels so execution can continue at `target`.
fn branch_to(
    stack: &mut OperandStack,
    labels: &mut Labels,
    relative_depth: u32,
    target: u32,
    lifted_func: &LiftedFunction,
    result_arity: u32,
) -> Result<Signal> {
    if target as usize == lifted_func.schedules.len() {
        return Ok(Signal::Return(stack.take_top(result_arity)?));
    }

    let (label, truncate_to) = labels.resolve_branch(relative_depth)?;
    let arity = label.branch_arity as usize;

    let target_height = lifted_func
        .schedules
        .get(target as usize)
        .map(|instr| instr.height_in as usize)
        .ok_or_else(|| ExecuteError::invalid_binary("branch target out of range"))?;

    stack.unwind(target_height, arity)?;
    labels.truncate(truncate_to);
    Ok(Signal::Advance(target as usize))
}

fn resolve_reads(regs: &Registers, wanted: &[Register]) -> Result<Vec<RegAccess>> {
    wanted
        .iter()
        .map(|&reg| {
            Ok(RegAccess {
                reg,
                value: regs.read(reg)?,
            })
        })
        .collect()
}
