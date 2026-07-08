use ananse_decoder::{Image, Module, OpCode, WASM_PAGE_SIZE, Word};
use ananse_lift::{LiftedFunction, LiftedProgram, Register, Successors, lift};
use wasmparser::Operator;

use crate::record::{Control, Flow, Outcome, RtFrame};
use crate::value::{Arithmetic, Compare, Unary, arithmetic, compare, unary};
use crate::{
    Entry, ExecuteError, Host, HostAction, MemAccess, RegAccess, Result, StepObserver, StepRecord,
    Transition, Trap,
};

/// Maximum direct-call nesting before the executor reports
/// [`Trap::CallStackExhausted`], bounding host stack use.
const MAX_CALL_DEPTH: usize = 1024;

/// The outcome of an execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Execution {
    /// The entry function's return values, empty if the program exited.
    pub returns: Vec<Word>,
    /// The status code if the program halted through `proc_exit`.
    pub exit: Option<i32>,
    /// The number of operators executed (records emitted).
    pub steps: u64,
}

/// Executes a validated module under its static register schedule.
///
/// The module is lifted to its register schedule and parsed into an executable
/// image, then interpreted: every operator's register touches are resolved
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
    let call_args = entry_args(&image, func_index, params, args)?;

    let mut interp = Interpreter {
        globals: image.globals.clone(),
        memory: image.memory.clone(),
        max_pages: image.max_pages,
        observer,
        host,
        steps: 0,
    };
    let flow = interp.run_function(&image, &program, defined, call_args, 0)?;
    Ok(match flow {
        Flow::Return(returns) => Execution {
            returns,
            exit: None,
            steps: interp.steps,
        },
        Flow::Exit(code) => Execution {
            returns: Vec::new(),
            exit: Some(code),
            steps: interp.steps,
        },
    })
}

fn entry_args(image: &Image, func_index: u32, params: u32, args: &[Word]) -> Result<Vec<Word>> {
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
    fn run_function(
        &mut self,
        image: &Image,
        program: &LiftedProgram,
        defined: usize,
        args: Vec<Word>,
        depth: usize,
    ) -> Result<Flow> {
        if depth > MAX_CALL_DEPTH {
            return Err(Trap::CallStackExhausted.into());
        }

        let func = &image.funcs[defined];
        let lf = &program.functions[defined];
        let func_index = func.func_index;
        let result_arity = image.types[func.type_idx as usize].results;

        let mut locals = args;
        locals.extend(func.declared.iter().map(|t| t.zero()));
        let mut stack: Vec<Word> = Vec::new();
        let mut frames: Vec<RtFrame> = Vec::new();
        let mut pc = 0usize;

        loop {
            let op = &func.ops[pc];
            let sched = &lf.instrs[pc];
            if stack.len() != sched.height_in as usize {
                return Err(ExecuteError::ScheduleMismatch {
                    func_index,
                    pc: pc as u32,
                    schedule: sched.height_in,
                    actual: stack.len() as u32,
                });
            }
            let reads = resolve(&sched.reads, &stack, &locals, &self.globals)?;

            let outcome = match op {
                Operator::Unreachable => return Err(Trap::Unreachable.into()),
                Operator::Nop => Outcome::advance(OpCode::Nop, pc + 1),

                Operator::Block { blockty } => {
                    let (_, out) = image.block_arity(blockty)?;
                    frames.push(RtFrame {
                        is_loop: false,
                        branch_arity: out,
                        end_pc: func.ends[pc],
                    });
                    Outcome::advance(OpCode::Block, pc + 1)
                }
                Operator::Loop { blockty } => {
                    let (input, _) = image.block_arity(blockty)?;
                    frames.push(RtFrame {
                        is_loop: true,
                        branch_arity: input,
                        end_pc: func.ends[pc],
                    });
                    Outcome::advance(OpCode::Loop, pc + 1)
                }
                Operator::If { blockty } => {
                    let (_, out) = image.block_arity(blockty)?;
                    let cond = pop(&mut stack)?;
                    frames.push(RtFrame {
                        is_loop: false,
                        branch_arity: out,
                        end_pc: func.ends[pc],
                    });
                    let Successors::Branch { taken, not_taken } = sched.successors else {
                        return Err(ExecuteError::invalid_binary(
                            "`if` without a branch successor",
                        ));
                    };
                    let next = if cond.is_true() { taken } else { not_taken } as usize;
                    reconcile(&mut frames, next);
                    Outcome::advance(OpCode::If, next)
                }
                Operator::Else => {
                    let Successors::Jump(target) = sched.successors else {
                        return Err(ExecuteError::invalid_binary(
                            "`else` without a jump successor",
                        ));
                    };
                    reconcile(&mut frames, target as usize);
                    Outcome::advance(OpCode::Else, target as usize)
                }
                Operator::End => match sched.successors {
                    Successors::Return => {
                        let results = take_top(&mut stack, result_arity)?;
                        Outcome {
                            opcode: OpCode::End,
                            transition: Transition::Return,
                            mem: Vec::new(),
                            control: Control::Return(results),
                        }
                    }
                    _ => {
                        reconcile(&mut frames, pc + 1);
                        Outcome::advance(OpCode::End, pc + 1)
                    }
                },

                Operator::Br { relative_depth } => {
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
                        lf,
                        result_arity,
                    )?;
                    Outcome {
                        opcode: OpCode::Br,
                        transition: transition_of(&control),
                        mem: Vec::new(),
                        control,
                    }
                }
                Operator::BrIf { relative_depth } => {
                    let cond = pop(&mut stack)?;
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
                            lf,
                            result_arity,
                        )?;
                        Outcome {
                            opcode: OpCode::BrIf,
                            transition: transition_of(&control),
                            mem: Vec::new(),
                            control,
                        }
                    } else {
                        reconcile(&mut frames, not_taken as usize);
                        Outcome::advance(OpCode::BrIf, not_taken as usize)
                    }
                }
                Operator::BrTable { targets } => {
                    let selector = pop_u32(&mut stack)? as usize;
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
                    let control =
                        branch_to(&mut stack, &mut frames, depth_to, target, lf, result_arity)?;
                    Outcome {
                        opcode: OpCode::BrTable,
                        transition: transition_of(&control),
                        mem: Vec::new(),
                        control,
                    }
                }
                Operator::Return => {
                    let results = take_top(&mut stack, result_arity)?;
                    Outcome {
                        opcode: OpCode::Return,
                        transition: Transition::Return,
                        mem: Vec::new(),
                        control: Control::Return(results),
                    }
                }

                Operator::Call { function_index } => {
                    let f = *function_index;
                    let (params, _) = image.func_arity(f).ok_or_else(|| {
                        ExecuteError::invalid_binary("call to an undeclared function")
                    })?;
                    let call_args = take_top(&mut stack, params)?;
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
                        match self.run_function(image, program, callee, call_args, depth + 1)? {
                            Flow::Return(results) => {
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
                            Flow::Exit(code) => Outcome {
                                opcode: OpCode::Call,
                                transition: Transition::Exit(code),
                                mem: Vec::new(),
                                control: Control::Exit(code),
                            },
                        }
                    }
                }
                Operator::CallIndirect { .. } => {
                    return Err(ExecuteError::UnsupportedOperator {
                        offset: 0,
                        message: "call_indirect".into(),
                    });
                }

                Operator::Drop => {
                    pop(&mut stack)?;
                    Outcome::advance(OpCode::Drop, pc + 1)
                }
                Operator::Select => {
                    let cond = pop(&mut stack)?;
                    let rhs = pop(&mut stack)?;
                    let lhs = pop(&mut stack)?;
                    stack.push(if cond.is_true() { lhs } else { rhs });
                    Outcome::advance(OpCode::Select, pc + 1)
                }

                Operator::LocalGet { local_index } => {
                    let value = *locals
                        .get(*local_index as usize)
                        .ok_or_else(|| ExecuteError::invalid_binary("local index out of range"))?;
                    stack.push(value);
                    Outcome::advance(OpCode::LocalGet, pc + 1)
                }
                Operator::LocalSet { local_index } => {
                    let value = pop(&mut stack)?;
                    *locals.get_mut(*local_index as usize).ok_or_else(|| {
                        ExecuteError::invalid_binary("local index out of range")
                    })? = value;
                    Outcome::advance(OpCode::LocalSet, pc + 1)
                }
                Operator::LocalTee { local_index } => {
                    let value = *stack
                        .last()
                        .ok_or_else(|| ExecuteError::invalid_binary("operand stack underflow"))?;
                    *locals.get_mut(*local_index as usize).ok_or_else(|| {
                        ExecuteError::invalid_binary("local index out of range")
                    })? = value;
                    Outcome::advance(OpCode::LocalTee, pc + 1)
                }
                Operator::GlobalGet { global_index } => {
                    let value = *self
                        .globals
                        .get(*global_index as usize)
                        .ok_or_else(|| ExecuteError::invalid_binary("global index out of range"))?;
                    stack.push(value);
                    Outcome::advance(OpCode::GlobalGet, pc + 1)
                }
                Operator::GlobalSet { global_index } => {
                    let value = pop(&mut stack)?;
                    *self
                        .globals
                        .get_mut(*global_index as usize)
                        .ok_or_else(|| {
                            ExecuteError::invalid_binary("global index out of range")
                        })? = value;
                    Outcome::advance(OpCode::GlobalSet, pc + 1)
                }

                Operator::I32Const { value } => {
                    stack.push(Word::I32(*value as u32));
                    Outcome::advance(OpCode::I32Const, pc + 1)
                }
                Operator::I64Const { value } => {
                    stack.push(Word::I64(*value as u64));
                    Outcome::advance(OpCode::I64Const, pc + 1)
                }

                Operator::I32Load { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I32Load, pc)?
                }
                Operator::I64Load { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I64Load, pc)?
                }
                Operator::I32Load8S { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I32Load8S, pc)?
                }
                Operator::I32Load8U { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I32Load8U, pc)?
                }
                Operator::I32Load16S { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I32Load16S, pc)?
                }
                Operator::I32Load16U { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I32Load16U, pc)?
                }
                Operator::I64Load8S { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I64Load8S, pc)?
                }
                Operator::I64Load8U { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I64Load8U, pc)?
                }
                Operator::I64Load16S { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I64Load16S, pc)?
                }
                Operator::I64Load16U { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I64Load16U, pc)?
                }
                Operator::I64Load32S { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I64Load32S, pc)?
                }
                Operator::I64Load32U { memarg } => {
                    self.load(&mut stack, memarg.offset, OpCode::I64Load32U, pc)?
                }

                Operator::I32Store { memarg } => {
                    self.store(&mut stack, memarg.offset, 4, OpCode::I32Store, pc)?
                }
                Operator::I64Store { memarg } => {
                    self.store(&mut stack, memarg.offset, 8, OpCode::I64Store, pc)?
                }
                Operator::I32Store8 { memarg } => {
                    self.store(&mut stack, memarg.offset, 1, OpCode::I32Store8, pc)?
                }
                Operator::I32Store16 { memarg } => {
                    self.store(&mut stack, memarg.offset, 2, OpCode::I32Store16, pc)?
                }
                Operator::I64Store8 { memarg } => {
                    self.store(&mut stack, memarg.offset, 1, OpCode::I64Store8, pc)?
                }
                Operator::I64Store16 { memarg } => {
                    self.store(&mut stack, memarg.offset, 2, OpCode::I64Store16, pc)?
                }
                Operator::I64Store32 { memarg } => {
                    self.store(&mut stack, memarg.offset, 4, OpCode::I64Store32, pc)?
                }

                Operator::MemorySize { .. } => self.size(&mut stack, pc),
                Operator::MemoryGrow { .. } => self.grow(&mut stack, pc)?,

                Operator::I32Eqz => unop(&mut stack, Unary::Eqz, OpCode::I32Eqz, pc)?,
                Operator::I64Eqz => unop(&mut stack, Unary::Eqz, OpCode::I64Eqz, pc)?,
                Operator::I32Clz => unop(&mut stack, Unary::Clz, OpCode::I32Clz, pc)?,
                Operator::I32Ctz => unop(&mut stack, Unary::Ctz, OpCode::I32Ctz, pc)?,
                Operator::I32Popcnt => unop(&mut stack, Unary::Popcnt, OpCode::I32Popcnt, pc)?,
                Operator::I64Clz => unop(&mut stack, Unary::Clz, OpCode::I64Clz, pc)?,
                Operator::I64Ctz => unop(&mut stack, Unary::Ctz, OpCode::I64Ctz, pc)?,
                Operator::I64Popcnt => unop(&mut stack, Unary::Popcnt, OpCode::I64Popcnt, pc)?,

                Operator::I32Add => binop(&mut stack, Arithmetic::Add, OpCode::I32Add, pc)?,
                Operator::I32Sub => binop(&mut stack, Arithmetic::Sub, OpCode::I32Sub, pc)?,
                Operator::I32Mul => binop(&mut stack, Arithmetic::Mul, OpCode::I32Mul, pc)?,
                Operator::I32DivS => binop(&mut stack, Arithmetic::DivS, OpCode::I32DivS, pc)?,
                Operator::I32DivU => binop(&mut stack, Arithmetic::DivU, OpCode::I32DivU, pc)?,
                Operator::I32RemS => binop(&mut stack, Arithmetic::RemS, OpCode::I32RemS, pc)?,
                Operator::I32RemU => binop(&mut stack, Arithmetic::RemU, OpCode::I32RemU, pc)?,
                Operator::I32And => binop(&mut stack, Arithmetic::And, OpCode::I32And, pc)?,
                Operator::I32Or => binop(&mut stack, Arithmetic::Or, OpCode::I32Or, pc)?,
                Operator::I32Xor => binop(&mut stack, Arithmetic::Xor, OpCode::I32Xor, pc)?,
                Operator::I32Shl => binop(&mut stack, Arithmetic::Shl, OpCode::I32Shl, pc)?,
                Operator::I32ShrS => binop(&mut stack, Arithmetic::ShrS, OpCode::I32ShrS, pc)?,
                Operator::I32ShrU => binop(&mut stack, Arithmetic::ShrU, OpCode::I32ShrU, pc)?,
                Operator::I32Rotl => binop(&mut stack, Arithmetic::Rotl, OpCode::I32Rotl, pc)?,
                Operator::I32Rotr => binop(&mut stack, Arithmetic::Rotr, OpCode::I32Rotr, pc)?,
                Operator::I64Add => binop(&mut stack, Arithmetic::Add, OpCode::I64Add, pc)?,
                Operator::I64Sub => binop(&mut stack, Arithmetic::Sub, OpCode::I64Sub, pc)?,
                Operator::I64Mul => binop(&mut stack, Arithmetic::Mul, OpCode::I64Mul, pc)?,
                Operator::I64DivS => binop(&mut stack, Arithmetic::DivS, OpCode::I64DivS, pc)?,
                Operator::I64DivU => binop(&mut stack, Arithmetic::DivU, OpCode::I64DivU, pc)?,
                Operator::I64RemS => binop(&mut stack, Arithmetic::RemS, OpCode::I64RemS, pc)?,
                Operator::I64RemU => binop(&mut stack, Arithmetic::RemU, OpCode::I64RemU, pc)?,
                Operator::I64And => binop(&mut stack, Arithmetic::And, OpCode::I64And, pc)?,
                Operator::I64Or => binop(&mut stack, Arithmetic::Or, OpCode::I64Or, pc)?,
                Operator::I64Xor => binop(&mut stack, Arithmetic::Xor, OpCode::I64Xor, pc)?,
                Operator::I64Shl => binop(&mut stack, Arithmetic::Shl, OpCode::I64Shl, pc)?,
                Operator::I64ShrS => binop(&mut stack, Arithmetic::ShrS, OpCode::I64ShrS, pc)?,
                Operator::I64ShrU => binop(&mut stack, Arithmetic::ShrU, OpCode::I64ShrU, pc)?,
                Operator::I64Rotl => binop(&mut stack, Arithmetic::Rotl, OpCode::I64Rotl, pc)?,
                Operator::I64Rotr => binop(&mut stack, Arithmetic::Rotr, OpCode::I64Rotr, pc)?,

                Operator::I32Eq => cmpop(&mut stack, Compare::Eq, OpCode::I32Eq, pc)?,
                Operator::I32Ne => cmpop(&mut stack, Compare::Ne, OpCode::I32Ne, pc)?,
                Operator::I32LtS => cmpop(&mut stack, Compare::LtS, OpCode::I32LtS, pc)?,
                Operator::I32LtU => cmpop(&mut stack, Compare::LtU, OpCode::I32LtU, pc)?,
                Operator::I32GtS => cmpop(&mut stack, Compare::GtS, OpCode::I32GtS, pc)?,
                Operator::I32GtU => cmpop(&mut stack, Compare::GtU, OpCode::I32GtU, pc)?,
                Operator::I32LeS => cmpop(&mut stack, Compare::LeS, OpCode::I32LeS, pc)?,
                Operator::I32LeU => cmpop(&mut stack, Compare::LeU, OpCode::I32LeU, pc)?,
                Operator::I32GeS => cmpop(&mut stack, Compare::GeS, OpCode::I32GeS, pc)?,
                Operator::I32GeU => cmpop(&mut stack, Compare::GeU, OpCode::I32GeU, pc)?,
                Operator::I64Eq => cmpop(&mut stack, Compare::Eq, OpCode::I64Eq, pc)?,
                Operator::I64Ne => cmpop(&mut stack, Compare::Ne, OpCode::I64Ne, pc)?,
                Operator::I64LtS => cmpop(&mut stack, Compare::LtS, OpCode::I64LtS, pc)?,
                Operator::I64LtU => cmpop(&mut stack, Compare::LtU, OpCode::I64LtU, pc)?,
                Operator::I64GtS => cmpop(&mut stack, Compare::GtS, OpCode::I64GtS, pc)?,
                Operator::I64GtU => cmpop(&mut stack, Compare::GtU, OpCode::I64GtU, pc)?,
                Operator::I64LeS => cmpop(&mut stack, Compare::LeS, OpCode::I64LeS, pc)?,
                Operator::I64LeU => cmpop(&mut stack, Compare::LeU, OpCode::I64LeU, pc)?,
                Operator::I64GeS => cmpop(&mut stack, Compare::GeS, OpCode::I64GeS, pc)?,
                Operator::I64GeU => cmpop(&mut stack, Compare::GeU, OpCode::I64GeU, pc)?,

                Operator::I32WrapI64 => {
                    let bits = match pop(&mut stack)? {
                        Word::I64(b) => b,
                        Word::I32(b) => u64::from(b),
                    };
                    stack.push(Word::I32(bits as u32));
                    Outcome::advance(OpCode::I32WrapI64, pc + 1)
                }
                Operator::I64ExtendI32S => {
                    let bits = match pop(&mut stack)? {
                        Word::I32(b) => b,
                        Word::I64(b) => b as u32,
                    };
                    stack.push(Word::I64(i64::from(bits as i32) as u64));
                    Outcome::advance(OpCode::I64ExtendI32S, pc + 1)
                }
                Operator::I64ExtendI32U => {
                    let bits = match pop(&mut stack)? {
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
                _ => resolve(&sched.writes, &stack, &locals, &self.globals)?,
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
                Control::Return(results) => return Ok(Flow::Return(results)),
                Control::Exit(code) => return Ok(Flow::Exit(code)),
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
        let (bytes, signed, result64) = load_shape(opcode);
        let base = pop_u32(stack)?;
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
        stack.push(extend(raw, bytes, signed, result64));
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
        let value = pop(stack)?;
        let base = pop_u32(stack)?;
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
        stack.push(Word::I32((self.memory.len() / WASM_PAGE_SIZE) as u32));
        Outcome::advance(OpCode::MemorySize, pc + 1)
    }

    fn grow(&mut self, stack: &mut Vec<Word>, pc: usize) -> Result<Outcome> {
        let delta = pop_u32(stack)? as usize;
        let old_pages = self.memory.len() / WASM_PAGE_SIZE;
        let grown = old_pages
            .checked_add(delta)
            .filter(|&n| n <= WASM_PAGE_SIZE)
            .filter(|&n| self.max_pages.is_none_or(|m| n as u64 <= m))
            .and_then(|n| n.checked_mul(WASM_PAGE_SIZE).map(|bytes| (n, bytes)));
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
    if target as usize == lf.instrs.len() {
        return Ok(Control::Return(take_top(stack, result_arity)?));
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
        .instrs
        .get(target as usize)
        .map(|instr| instr.height_in as usize)
        .ok_or_else(|| ExecuteError::invalid_binary("branch target out of range"))?;
    unwind_stack(stack, target_height, arity)?;
    frames.truncate(if is_loop { target_idx + 1 } else { target_idx });
    Ok(Control::Advance(target as usize))
}

fn unwind_stack(stack: &mut Vec<Word>, target_height: usize, arity: usize) -> Result<()> {
    let keep = target_height
        .checked_sub(arity)
        .ok_or_else(|| ExecuteError::invalid_binary("branch arity exceeds the target height"))?;
    let from = stack
        .len()
        .checked_sub(arity)
        .ok_or_else(|| ExecuteError::invalid_binary("branch arity exceeds the stack height"))?;
    let carried = stack.split_off(from);
    stack.truncate(keep);
    stack.extend(carried);
    Ok(())
}

fn take_top(stack: &mut Vec<Word>, n: u32) -> Result<Vec<Word>> {
    let at = stack
        .len()
        .checked_sub(n as usize)
        .ok_or_else(|| ExecuteError::invalid_binary("arity exceeds the stack height"))?;
    Ok(stack.split_off(at))
}

fn reconcile(frames: &mut Vec<RtFrame>, next: usize) {
    while frames
        .last()
        .is_some_and(|frame| (frame.end_pc as usize) < next)
    {
        frames.pop();
    }
}

fn transition_of(control: &Control) -> Transition {
    match control {
        Control::Advance(next) => Transition::Next(*next as u32),
        Control::Return(_) => Transition::Return,
        Control::Exit(code) => Transition::Exit(*code),
    }
}

fn pop(stack: &mut Vec<Word>) -> Result<Word> {
    stack
        .pop()
        .ok_or_else(|| ExecuteError::invalid_binary("operand stack underflow"))
}

fn pop_u32(stack: &mut Vec<Word>) -> Result<u32> {
    Ok(match pop(stack)? {
        Word::I32(v) => v,
        Word::I64(v) => v as u32,
    })
}

fn binop(stack: &mut Vec<Word>, kind: Arithmetic, opcode: OpCode, pc: usize) -> Result<Outcome> {
    let rhs = pop(stack)?;
    let lhs = pop(stack)?;
    stack.push(arithmetic(kind, lhs, rhs)?);
    Ok(Outcome::advance(opcode, pc + 1))
}

fn cmpop(stack: &mut Vec<Word>, kind: Compare, opcode: OpCode, pc: usize) -> Result<Outcome> {
    let rhs = pop(stack)?;
    let lhs = pop(stack)?;
    stack.push(compare(kind, lhs, rhs));
    Ok(Outcome::advance(opcode, pc + 1))
}

fn unop(stack: &mut Vec<Word>, kind: Unary, opcode: OpCode, pc: usize) -> Result<Outcome> {
    let operand = pop(stack)?;
    stack.push(unary(kind, operand));
    Ok(Outcome::advance(opcode, pc + 1))
}

fn load_shape(opcode: OpCode) -> (usize, bool, bool) {
    match opcode {
        OpCode::I32Load => (4, false, false),
        OpCode::I64Load => (8, false, true),
        OpCode::I32Load8S => (1, true, false),
        OpCode::I32Load8U => (1, false, false),
        OpCode::I32Load16S => (2, true, false),
        OpCode::I32Load16U => (2, false, false),
        OpCode::I64Load8S => (1, true, true),
        OpCode::I64Load8U => (1, false, true),
        OpCode::I64Load16S => (2, true, true),
        OpCode::I64Load16U => (2, false, true),
        OpCode::I64Load32S => (4, true, true),
        OpCode::I64Load32U => (4, false, true),
        _ => (0, false, false),
    }
}

fn extend(raw: u64, bytes: usize, signed: bool, result64: bool) -> Word {
    let value = if signed && bytes < 8 {
        let shift = 64 - (bytes as u32 * 8);
        ((raw << shift) as i64 >> shift) as u64
    } else {
        raw
    };
    if result64 {
        Word::I64(value)
    } else {
        Word::I32(value as u32)
    }
}

fn resolve(
    regs: &[Register],
    stack: &[Word],
    locals: &[Word],
    globals: &[Word],
) -> Result<Vec<RegAccess>> {
    regs.iter()
        .map(|&reg| {
            Ok(RegAccess {
                reg,
                value: read_reg(reg, stack, locals, globals)?,
            })
        })
        .collect()
}

fn read_reg(reg: Register, stack: &[Word], locals: &[Word], globals: &[Word]) -> Result<Word> {
    let value = match reg {
        Register::Local(i) => locals.get(i as usize).copied(),
        Register::Global(g) => globals.get(g as usize).copied(),
        Register::Stack(d) => stack.get(d as usize).copied(),
    };
    value.ok_or_else(|| ExecuteError::invalid_binary("register access out of range"))
}
