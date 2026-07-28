use ananse_decoder::{Image, Instruction, Module, OpCode, WASM32_PAGE_SIZE, Word};
use ananse_lift::{LiftedFunction, LiftedProgram, Successors, lift};

use crate::operations::{Binary, Compare, Unary, binop, cmpop, sign_extend, unop};
use crate::record::{Control, Eval, Outcome, RtFrame};
use crate::{
    Entry, ExecuteError, Execution, Host, HostAction, MemAccess, Result, StepObserver, StepRecord,
    Transition, Trap, memory,
};

/// Maximum direct-call nesting before the executor reports
/// [`Trap::CallStackExhausted`], bounding host stack use.
const MAX_CALL_DEPTH: usize = 1024;

/// Executes a validated module under its static register schedule.
///
/// The module is lifted to its register schedule and parsed into an executable
/// image, then interpreted: every instruction's register touches are resolved
/// against the live operand stack, locals, and globals and emitted as a
/// [`StepRecord`]. At each program point the operand-stack height execution
/// reaches is checked against the height the schedule predicts; a disagreement
/// is reported as [`ExecuteError::ScheduleMismatch`].
///
/// The record stream is a deterministic function of the
/// `(module, entry, args, host)` inputs.
pub fn execute<O: StepObserver, H: Host>(
    module: &Module,
    entry: &Entry,
    args: &[Word],
    host: &mut H,
    observer: &mut O,
) -> Result<Execution> {
    let program = lift(module)?;
    let image = Image::parse(module.bytes())?;

    let Some(func_index) = entry.resolve(&image)? else {
        return Ok(Execution {
            returns: Vec::new(),
            exit: None,
            steps: 0,
        });
    };
    let defined = func_index
        .checked_sub(image.num_imported)
        .map(|d| d as usize)
        .filter(|&d| d < image.funcs.len())
        .ok_or(ExecuteError::UndefinedEntry)?;

    let (params, _) = image
        .func_arity(func_index)
        .ok_or(ExecuteError::UndefinedEntry)?;

    let call_args = validate_entry_args(&image, func_index, params, args)?;

    let mut interpreter = Interpreter {
        globals: image.globals.clone(),
        memory: image.memory.clone(),
        max_pages: image.max_pages,
        observer,
        host,
        steps: 0,
    };
    let flow = interpreter.eval(&image, &program, defined, call_args, 0)?;

    Ok(match flow {
        Eval::Return(returns) => Execution {
            returns,
            exit: None,
            steps: interpreter.steps,
        },
        Eval::Exit(code) => Execution {
            returns: Vec::new(),
            exit: Some(code),
            steps: interpreter.steps,
        },
    })
}

fn validate_entry_args(
    image: &Image,
    func_index: u32,
    params: u32,
    args: &[Word],
) -> Result<Vec<Word>> {
    if args.is_empty() && params > 0 {
        let ty = &image.types[image.func_types[func_index as usize] as usize];
        return Ok(ty.params.iter().map(|t| t.zero()).collect());
    }
    if args.len() != params as usize {
        return Err(ExecuteError::ArgumentCountMismatch {
            expected: params as usize,
            actual: args.len(),
        });
    }
    Ok(args.to_vec())
}

/// The mutable execution state: module-wide globals and linear memory, the
/// record sink, and the host environment.
struct Interpreter<'o, O: StepObserver, H: Host> {
    globals: Vec<Word>,
    memory: Vec<u8>,
    max_pages: Option<u64>,
    observer: &'o mut O,
    host: &'o mut H,
    steps: u64,
}

impl<O: StepObserver, H: Host> Interpreter<'_, O, H> {
    fn eval(
        &mut self,
        image: &Image,
        program: &LiftedProgram,
        defined: usize,
        args: Vec<Word>,
        depth: usize,
    ) -> Result<Eval> {
        if depth > MAX_CALL_DEPTH {
            return Err(Trap::CallStackExhausted.into());
        }

        let func_image = &image.funcs[defined];
        let lifted_func = &program.functions[defined];
        let func_index = func_image.func_index;
        let result_arity = image.types[func_image.type_idx as usize].results;

        let mut locals = args;
        locals.extend(func_image.declared.iter().map(|t| t.zero()));
        let mut stack: Vec<Word> = Vec::new();
        let mut frames: Vec<RtFrame> = Vec::new();
        let mut pc = 0usize;

        loop {
            let instr = &func_image.instructions[pc];
            let sched = &lifted_func.schedules[pc];
            if stack.len() != sched.height_in as usize {
                return Err(ExecuteError::ScheduleMismatch {
                    func_index,
                    pc: pc as u32,
                    schedule: sched.height_in,
                    actual: stack.len() as u32,
                });
            }
            let reads =
                memory::resolve_register_access(&sched.reads, &stack, &locals, &self.globals)?;

            let outcome = match instr {
                Instruction::Unreachable => return Err(Trap::Unreachable.into()),
                Instruction::Nop => Outcome::advance(OpCode::Nop, pc + 1),

                Instruction::Block { blockty } => {
                    let (_, out) = image.block_arity(blockty)?;
                    frames.push(RtFrame {
                        is_loop: false,
                        branch_arity: out,
                        end_pc: func_image.ends[pc],
                    });
                    Outcome::advance(OpCode::Block, pc + 1)
                }
                Instruction::Loop { blockty } => {
                    let (input, _) = image.block_arity(blockty)?;
                    frames.push(RtFrame {
                        is_loop: true,
                        branch_arity: input,
                        end_pc: func_image.ends[pc],
                    });
                    Outcome::advance(OpCode::Loop, pc + 1)
                }
                Instruction::If { blockty } => {
                    let (_, out) = image.block_arity(blockty)?;
                    let cond = memory::stack_pop(&mut stack)?;
                    frames.push(RtFrame {
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
                    memory::frame_pop(&mut frames, next);
                    Outcome::advance(OpCode::If, next)
                }
                Instruction::Else => {
                    let Successors::Jump(target) = sched.successors else {
                        return Err(ExecuteError::invalid_binary(
                            "`else` without a jump successor",
                        ));
                    };
                    memory::frame_pop(&mut frames, target as usize);
                    Outcome::advance(OpCode::Else, target as usize)
                }
                Instruction::End => match sched.successors {
                    Successors::Return => {
                        let results = memory::stack_take_top(&mut stack, result_arity)?;
                        Outcome {
                            opcode: OpCode::End,
                            transition: Transition::Return,
                            mem: Vec::new(),
                            control: Control::Return(results),
                        }
                    }
                    _ => {
                        memory::frame_pop(&mut frames, pc + 1);
                        Outcome::advance(OpCode::End, pc + 1)
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
                        &mut frames,
                        *relative_depth,
                        target,
                        lifted_func,
                        result_arity,
                    )?;
                    Outcome {
                        opcode: OpCode::Br,
                        transition: control.transition(),
                        mem: Vec::new(),
                        control,
                    }
                }
                Instruction::BrIf { relative_depth } => {
                    let cond = memory::stack_pop(&mut stack)?;
                    let Successors::Branch { taken, not_taken } = sched.successors else {
                        return Err(ExecuteError::invalid_binary(
                            "`br_if` without a branch successor",
                        ));
                    };
                    if cond.is_true() {
                        let control = branch_to(
                            &mut stack,
                            &mut frames,
                            *relative_depth,
                            taken,
                            lifted_func,
                            result_arity,
                        )?;
                        Outcome {
                            opcode: OpCode::BrIf,
                            transition: control.transition(),
                            mem: Vec::new(),
                            control,
                        }
                    } else {
                        memory::frame_pop(&mut frames, not_taken as usize);
                        Outcome::advance(OpCode::BrIf, not_taken as usize)
                    }
                }
                Instruction::BrTable { targets } => {
                    let selector = memory::stack_pop_u32(&mut stack)? as usize;
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
                        &mut frames,
                        depth_to,
                        target,
                        lifted_func,
                        result_arity,
                    )?;
                    Outcome {
                        opcode: OpCode::BrTable,
                        transition: control.transition(),
                        mem: Vec::new(),
                        control,
                    }
                }
                Instruction::Return => {
                    let results = memory::stack_take_top(&mut stack, result_arity)?;
                    Outcome {
                        opcode: OpCode::Return,
                        transition: Transition::Return,
                        mem: Vec::new(),
                        control: Control::Return(results),
                    }
                }

                Instruction::Call { function_index } => {
                    let f = *function_index;
                    let (params, _) = image.func_arity(f).ok_or_else(|| {
                        ExecuteError::invalid_binary("call to an undeclared function")
                    })?;
                    let call_args = memory::stack_take_top(&mut stack, params)?;
                    if f < image.num_imported {
                        let import = &image.imports[f as usize];
                        match self.host.call(import, &call_args, &mut self.memory)? {
                            HostAction::Return(results) => {
                                stack.extend(results);
                                Outcome::advance(OpCode::Call, pc + 1)
                            }
                            HostAction::Exit(code) => Outcome {
                                opcode: OpCode::Call,
                                transition: Transition::Exit(code),
                                mem: Vec::new(),
                                control: Control::Exit(code),
                            },
                        }
                    } else {
                        let callee = (f - image.num_imported) as usize;
                        match self.eval(image, program, callee, call_args, depth + 1)? {
                            Eval::Return(results) => {
                                stack.extend(results);
                                Outcome {
                                    opcode: OpCode::Call,
                                    transition: Transition::Call {
                                        callee: f,
                                        return_pc: (pc + 1) as u32,
                                    },
                                    mem: Vec::new(),
                                    control: Control::Advance(pc + 1),
                                }
                            }
                            Eval::Exit(code) => Outcome {
                                opcode: OpCode::Call,
                                transition: Transition::Exit(code),
                                mem: Vec::new(),
                                control: Control::Exit(code),
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
                    memory::stack_pop(&mut stack)?;
                    Outcome::advance(OpCode::Drop, pc + 1)
                }
                Instruction::Select => {
                    let cond = memory::stack_pop(&mut stack)?;
                    let rhs = memory::stack_pop(&mut stack)?;
                    let lhs = memory::stack_pop(&mut stack)?;
                    stack.push(if cond.is_true() { lhs } else { rhs });
                    Outcome::advance(OpCode::Select, pc + 1)
                }

                Instruction::LocalGet { local_index } => {
                    let value = *locals
                        .get(*local_index as usize)
                        .ok_or_else(|| ExecuteError::invalid_binary("local index out of range"))?;
                    stack.push(value);
                    Outcome::advance(OpCode::LocalGet, pc + 1)
                }
                Instruction::LocalSet { local_index } => {
                    let value = memory::stack_pop(&mut stack)?;
                    *locals.get_mut(*local_index as usize).ok_or_else(|| {
                        ExecuteError::invalid_binary("local index out of range")
                    })? = value;
                    Outcome::advance(OpCode::LocalSet, pc + 1)
                }
                Instruction::LocalTee { local_index } => {
                    let value = *stack
                        .last()
                        .ok_or_else(|| ExecuteError::invalid_binary("operand stack underflow"))?;
                    *locals.get_mut(*local_index as usize).ok_or_else(|| {
                        ExecuteError::invalid_binary("local index out of range")
                    })? = value;
                    Outcome::advance(OpCode::LocalTee, pc + 1)
                }
                Instruction::GlobalGet { global_index } => {
                    let value = *self
                        .globals
                        .get(*global_index as usize)
                        .ok_or_else(|| ExecuteError::invalid_binary("global index out of range"))?;
                    stack.push(value);
                    Outcome::advance(OpCode::GlobalGet, pc + 1)
                }
                Instruction::GlobalSet { global_index } => {
                    let value = memory::stack_pop(&mut stack)?;
                    *self
                        .globals
                        .get_mut(*global_index as usize)
                        .ok_or_else(|| {
                            ExecuteError::invalid_binary("global index out of range")
                        })? = value;
                    Outcome::advance(OpCode::GlobalSet, pc + 1)
                }

                Instruction::I32Const { value } => {
                    stack.push(Word::I32(*value as u32));
                    Outcome::advance(OpCode::I32Const, pc + 1)
                }
                Instruction::I64Const { value } => {
                    stack.push(Word::I64(*value as u64));
                    Outcome::advance(OpCode::I64Const, pc + 1)
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
                    let bits = match memory::stack_pop(&mut stack)? {
                        Word::I64(b) => b,
                        Word::I32(b) => u64::from(b),
                    };
                    stack.push(Word::I32(bits as u32));
                    Outcome::advance(OpCode::I32WrapI64, pc + 1)
                }
                Instruction::I64ExtendI32S => {
                    let bits = match memory::stack_pop(&mut stack)? {
                        Word::I32(b) => b,
                        Word::I64(b) => b as u32,
                    };
                    stack.push(Word::I64(i64::from(bits as i32) as u64));
                    Outcome::advance(OpCode::I64ExtendI32S, pc + 1)
                }
                Instruction::I64ExtendI32U => {
                    let bits = match memory::stack_pop(&mut stack)? {
                        Word::I32(b) => b,
                        Word::I64(b) => b as u32,
                    };
                    stack.push(Word::I64(u64::from(bits)));
                    Outcome::advance(OpCode::I64ExtendI32U, pc + 1)
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
                Control::Exit(_) => Vec::new(),
                _ => {
                    memory::resolve_register_access(&sched.writes, &stack, &locals, &self.globals)?
                }
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
                Control::Advance(next) => pc = next,
                Control::Return(results) => return Ok(Eval::Return(results)),
                Control::Exit(code) => return Ok(Eval::Exit(code)),
            }
        }
    }

    fn load(
        &mut self,
        stack: &mut Vec<Word>,
        offset: u64,
        opcode: OpCode,
        pc: usize,
    ) -> Result<Outcome> {
        let (bytes, signed, result64) = opcode.load_shape();
        let base = memory::stack_pop_u32(stack)?;
        let address = u64::from(base)
            .checked_add(offset)
            .ok_or(Trap::MemoryOutOfBounds)?;
        let end = address
            .checked_add(bytes as u64)
            .filter(|&e| e <= self.memory.len() as u64)
            .ok_or(Trap::MemoryOutOfBounds)? as usize;
        let raw = self.memory[address as usize..end]
            .iter()
            .enumerate()
            .fold(0u64, |acc, (i, &byte)| acc | (u64::from(byte) << (8 * i)));
        stack.push(sign_extend(raw, bytes, signed, result64));
        Ok(Outcome {
            opcode,
            transition: Transition::Next((pc + 1) as u32),
            mem: vec![MemAccess {
                address,
                width: bytes as u8,
                value: raw,
                store: false,
            }],
            control: Control::Advance(pc + 1),
        })
    }

    fn store(
        &mut self,
        stack: &mut Vec<Word>,
        offset: u64,
        bytes: usize,
        opcode: OpCode,
        pc: usize,
    ) -> Result<Outcome> {
        let value = memory::stack_pop(stack)?;
        let base = memory::stack_pop_u32(stack)?;
        let bits = match value {
            Word::I32(v) => u64::from(v),
            Word::I64(v) => v,
        };
        let raw = if bytes == 8 {
            bits
        } else {
            bits & ((1u64 << (8 * bytes)) - 1)
        };
        let address = u64::from(base)
            .checked_add(offset)
            .ok_or(Trap::MemoryOutOfBounds)?;
        let end = address
            .checked_add(bytes as u64)
            .filter(|&e| e <= self.memory.len() as u64)
            .ok_or(Trap::MemoryOutOfBounds)? as usize;
        for (i, slot) in self.memory[address as usize..end].iter_mut().enumerate() {
            *slot = (raw >> (8 * i)) as u8;
        }
        Ok(Outcome {
            opcode,
            transition: Transition::Next((pc + 1) as u32),
            mem: vec![MemAccess {
                address,
                width: bytes as u8,
                value: raw,
                store: true,
            }],
            control: Control::Advance(pc + 1),
        })
    }

    fn size(&self, stack: &mut Vec<Word>, pc: usize) -> Outcome {
        stack.push(Word::I32((self.memory.len() / WASM32_PAGE_SIZE) as u32));
        Outcome::advance(OpCode::MemorySize, pc + 1)
    }

    fn grow(&mut self, stack: &mut Vec<Word>, pc: usize) -> Result<Outcome> {
        let delta = memory::stack_pop_u32(stack)? as usize;
        let old_pages = self.memory.len() / WASM32_PAGE_SIZE;
        let grown = old_pages
            .checked_add(delta)
            .filter(|&n| n <= WASM32_PAGE_SIZE)
            .filter(|&n| self.max_pages.is_none_or(|m| n as u64 <= m))
            .and_then(|n| n.checked_mul(WASM32_PAGE_SIZE).map(|bytes| (n, bytes)));
        let result = match grown {
            Some((_, bytes)) => {
                self.memory.resize(bytes, 0);
                old_pages as u32
            }
            None => u32::MAX,
        };
        stack.push(Word::I32(result));
        Ok(Outcome::advance(OpCode::MemoryGrow, pc + 1))
    }
}

/// Resolves a taken branch to `relative_depth`, unwinding the operand stack and
/// runtime control frames so execution can continue at `target`.
fn branch_to(
    stack: &mut Vec<Word>,
    frames: &mut Vec<RtFrame>,
    relative_depth: u32,
    target: u32,
    lf: &LiftedFunction,
    result_arity: u32,
) -> Result<Control> {
    if target as usize == lf.schedules.len() {
        return Ok(Control::Return(memory::stack_take_top(
            stack,
            result_arity,
        )?));
    }
    let target_idx = frames
        .len()
        .checked_sub(1)
        .and_then(|top| top.checked_sub(relative_depth as usize))
        .ok_or_else(|| ExecuteError::invalid_binary("branch depth exceeds the control stack"))?;
    let frame = frames
        .get(target_idx)
        .ok_or_else(|| ExecuteError::invalid_binary("branch target frame is missing"))?;
    let (arity, is_loop) = (frame.branch_arity as usize, frame.is_loop);
    let target_height = lf
        .schedules
        .get(target as usize)
        .map(|instr| instr.height_in as usize)
        .ok_or_else(|| ExecuteError::invalid_binary("branch target out of range"))?;
    memory::stack_unwind(stack, target_height, arity)?;
    frames.truncate(if is_loop { target_idx + 1 } else { target_idx });
    Ok(Control::Advance(target as usize))
}
