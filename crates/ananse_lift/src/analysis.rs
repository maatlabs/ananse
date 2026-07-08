use alloc::vec::Vec;

use ananse_decoder::ModuleInfo;
use wasmparser::{BlockType, FunctionBody, Operator};

use crate::{InstructionSchedule, LiftError, LiftedFunction, Register, Result, Successors};

/// Upper bound on a function's register-file width.
const MAX_REGISTER_FILE_WIDTH: u32 = 4096;

/// Placeholder target for a forward branch, patched to the real continuation
/// when the enclosing control frame is closed at its `end`.
const PENDING: u32 = u32::MAX;

/// Lifts every defined function in `bytes` to its static register schedule.
pub(crate) fn lift_functions(bytes: &[u8]) -> Result<Vec<LiftedFunction>> {
    let info = ModuleInfo::parse(bytes)?;
    info.bodies
        .iter()
        .enumerate()
        .map(|(i, body)| lift_function(&info, i, body))
        .collect()
}

fn lift_function(
    info: &ModuleInfo,
    code_index: usize,
    body: &FunctionBody,
) -> Result<LiftedFunction> {
    let func_index = u32::try_from(code_index)
        .ok()
        .and_then(|i| info.num_imported_funcs.checked_add(i))
        .ok_or(LiftError::FunctionTooLarge { func_index: 0 })?;

    let (param_count, result_arity) = info.func_arity(func_index).ok_or(LiftError::internal(
        "function references an undeclared type",
    ))?;

    let declared = body
        .get_locals_reader()
        .map_err(LiftError::malformed)?
        .into_iter()
        .try_fold(0u32, |acc, local| {
            let (count, _ty) = local.map_err(LiftError::malformed)?;
            acc.checked_add(count)
                .ok_or(LiftError::RegisterFileOverflow {
                    func_index,
                    width: u64::from(u32::MAX),
                })
        })?;

    let locals_count =
        param_count
            .checked_add(declared)
            .ok_or(LiftError::RegisterFileOverflow {
                func_index,
                width: u64::from(param_count) + u64::from(declared),
            })?;

    let mut lifter = Lifter::new(info, func_index, locals_count, result_arity);

    let mut ops = body.get_operators_reader().map_err(LiftError::malformed)?;
    while !ops.eof() {
        let offset = ops.original_position();
        let op = ops.read().map_err(LiftError::malformed)?;
        lifter.step(&op, offset)?;
    }

    lifter.finish()
}

/// A WebAssembly structured-control frame, tracked as the lift walks a body.
struct Frame {
    /// Branch-target arity: values a branch carries to this label. Loops use
    /// their input arity (the label is the loop header); all others use the
    /// output arity (the label is past the matching `end`).
    in_arity: u32,
    out_arity: u32,
    /// Operand-stack height at frame entry, below the frame's inputs.
    floor: u32,
    /// Whether the current point in this frame is unreachable (dead code).
    unreachable: bool,
    /// `Some(header_pc)` for a `loop`, whose branch target is its own header;
    /// `None` for forward-closing frames.
    loop_header: Option<u32>,
    /// Forward branches awaiting this frame's continuation, patched at `end`.
    fixups: Vec<Fixup>,
    /// For an `if` frame, the `if` instruction whose `not_taken` target is still
    /// pending an `else` or `end`.
    if_instr: Option<usize>,
}

/// A forward branch whose target becomes known when its frame closes.
struct Fixup {
    /// Index of the instruction whose successor target must be patched.
    instr: usize,
    /// Which successor field of that instruction to patch.
    slot: Slot,
}

enum Slot {
    Jump,
    BranchTaken,
    TableEntry(usize),
    TableDefault,
}

/// The stack effect and register touches of a non-control value operator.
struct ValueEffect {
    pops: u32,
    pushes: u32,
    bank_read: Option<Register>,
    bank_write: Option<Register>,
    /// `local.tee`: reads the operand-stack top and writes a local while leaving
    /// the top in place, so its stack slot is not rewritten.
    is_tee: bool,
}

impl ValueEffect {
    fn stack(pops: u32, pushes: u32) -> Self {
        Self {
            pops,
            pushes,
            bank_read: None,
            bank_write: None,
            is_tee: false,
        }
    }

    fn bank_read(reg: Register) -> Self {
        Self {
            pops: 0,
            pushes: 1,
            bank_read: Some(reg),
            bank_write: None,
            is_tee: false,
        }
    }

    fn bank_write(reg: Register) -> Self {
        Self {
            pops: 1,
            pushes: 0,
            bank_read: None,
            bank_write: Some(reg),
            is_tee: false,
        }
    }

    fn tee(reg: Register) -> Self {
        Self {
            pops: 1,
            pushes: 1,
            bank_read: None,
            bank_write: Some(reg),
            is_tee: true,
        }
    }
}

struct Lifter<'a> {
    info: &'a ModuleInfo<'a>,
    func_index: u32,
    locals_count: u32,
    globals_count: u32,
    result_arity: u32,
    height: u32,
    max_height: u32,
    ctrl: Vec<Frame>,
    instrs: Vec<InstructionSchedule>,
}

impl<'a> Lifter<'a> {
    fn new(
        info: &'a ModuleInfo<'a>,
        func_index: u32,
        locals_count: u32,
        result_arity: u32,
    ) -> Self {
        let func_frame = Frame {
            in_arity: 0,
            out_arity: result_arity,
            floor: 0,
            unreachable: false,
            loop_header: None,
            fixups: Vec::new(),
            if_instr: None,
        };
        Self {
            info,
            func_index,
            locals_count,
            globals_count: info.num_globals,
            result_arity,
            height: 0,
            max_height: 0,
            ctrl: alloc::vec![func_frame],
            instrs: Vec::new(),
        }
    }

    fn reachable(&self) -> bool {
        self.ctrl.last().map(|f| !f.unreachable).unwrap_or(false)
    }

    fn push_one(&mut self) -> Result<()> {
        self.height = self
            .height
            .checked_add(1)
            .ok_or(LiftError::RegisterFileOverflow {
                func_index: self.func_index,
                width: u64::from(self.height) + 1,
            })?;
        self.max_height = self.max_height.max(self.height);
        Ok(())
    }

    fn push(&mut self, n: u32) -> Result<()> {
        (0..n).try_for_each(|_| self.push_one())
    }

    /// Pops one operand-stack slot, honouring the WASM validation rule that a
    /// pop at the current frame's floor in unreachable code is polymorphic and
    /// leaves the height unchanged.
    fn pop_one(&mut self) {
        let (floor, unreachable) = self
            .ctrl
            .last()
            .map(|f| (f.floor, f.unreachable))
            .unwrap_or((0, false));
        if self.height > floor {
            if let Some(h) = self.height.checked_sub(1) {
                self.height = h;
            }
        } else if !unreachable {
            // Unreachable in a valid module: a reachable pop never underflows the
            // frame floor. Leave the height untouched rather than panic.
        }
    }

    fn pop(&mut self, n: u32) {
        (0..n).for_each(|_| self.pop_one());
    }

    /// Marks the current frame unreachable, resetting the height to its floor---
    /// the WASM validation algorithm's treatment of code after an unconditional
    /// branch, `return`, or `unreachable`.
    fn mark_unreachable(&mut self) {
        if let Some(frame) = self.ctrl.last_mut() {
            self.height = frame.floor;
            frame.unreachable = true;
        }
    }

    fn push_instr(
        &mut self,
        pc: u32,
        height_in: u32,
        reads: Vec<Register>,
        writes: Vec<Register>,
        successors: Successors,
    ) {
        self.instrs.push(InstructionSchedule {
            pc,
            height_in,
            reads,
            writes,
            successors,
        });
    }

    fn block_arity(&self, bt: &BlockType) -> Result<(u32, u32)> {
        match bt {
            BlockType::Empty => Ok((0, 0)),
            BlockType::Type(_) => Ok((0, 1)),
            BlockType::FuncType(idx) => self
                .info
                .type_arity(*idx)
                .ok_or_else(|| LiftError::internal("block references an undeclared type")),
        }
    }

    fn push_ctrl(
        &mut self,
        in_arity: u32,
        out_arity: u32,
        loop_header: Option<u32>,
        if_instr: Option<usize>,
    ) -> Result<()> {
        self.pop(in_arity);
        let floor = self.height;
        self.ctrl.push(Frame {
            in_arity,
            out_arity,
            floor,
            unreachable: false,
            loop_header,
            fixups: Vec::new(),
            if_instr,
        });
        self.push(in_arity)
    }

    /// Closes the top frame at the `end` whose program point is `end_pc`.
    ///
    /// Returns `true` when the closed frame was the function frame.
    fn pop_ctrl(&mut self, end_pc: u32) -> Result<bool> {
        let frame = self
            .ctrl
            .pop()
            .ok_or_else(|| LiftError::internal("`end` with no open control frame"))?;
        let continuation = end_pc.checked_add(1).ok_or(LiftError::FunctionTooLarge {
            func_index: self.func_index,
        })?;

        for fixup in &frame.fixups {
            let succ = &mut self
                .instrs
                .get_mut(fixup.instr)
                .ok_or(LiftError::internal(
                    "branch fixup references a missing instruction",
                ))?
                .successors;
            match (succ, &fixup.slot) {
                (Successors::Jump(t), Slot::Jump) => *t = continuation,
                (Successors::Branch { taken, .. }, Slot::BranchTaken) => *taken = continuation,
                (Successors::Table { targets, .. }, Slot::TableEntry(i)) => {
                    if let Some(t) = targets.get_mut(*i) {
                        *t = continuation;
                    }
                }
                (Successors::Table { default, .. }, Slot::TableDefault) => *default = continuation,
                _ => {
                    return Err(LiftError::internal(
                        "branch fixup slot does not match its successor",
                    ));
                }
            }
        }

        if let Some(if_idx) = frame.if_instr
            && let Some(InstructionSchedule {
                successors: Successors::Branch { not_taken, .. },
                ..
            }) = self.instrs.get_mut(if_idx)
        {
            *not_taken = continuation;
        }

        self.height =
            frame
                .floor
                .checked_add(frame.out_arity)
                .ok_or(LiftError::RegisterFileOverflow {
                    func_index: self.func_index,
                    width: u64::from(frame.floor) + u64::from(frame.out_arity),
                })?;
        self.max_height = self.max_height.max(self.height);

        Ok(self.ctrl.is_empty())
    }

    /// Resolves a branch to `relative_depth`, returning the target program point
    /// and registering a fixup when the target frame closes forward.
    fn branch_target(&mut self, relative_depth: u32, instr: usize, slot: Slot) -> Result<u32> {
        let idx = self
            .ctrl
            .len()
            .checked_sub(1)
            .and_then(|top| top.checked_sub(relative_depth as usize))
            .ok_or_else(|| LiftError::internal("branch depth exceeds the control stack"))?;
        let frame = self
            .ctrl
            .get_mut(idx)
            .ok_or_else(|| LiftError::internal("branch target frame is missing"))?;
        match frame.loop_header {
            Some(header) => Ok(header),
            None => {
                frame.fixups.push(Fixup { instr, slot });
                Ok(PENDING)
            }
        }
    }

    fn stack_read(&self, depth_from_top: u32) -> Result<Register> {
        self.height
            .checked_sub(depth_from_top)
            .ok_or_else(|| LiftError::internal("operand-stack read underflows the stack"))
            .map(Register::Stack)
    }

    fn top_reads(&self, n: u32) -> Result<Vec<Register>> {
        (1..=n).map(|k| self.stack_read(k)).collect()
    }

    fn value_regs(&self, eff: &ValueEffect) -> Result<(Vec<Register>, Vec<Register>)> {
        let mut reads = Vec::new();
        if let Some(reg) = eff.bank_read {
            reads.push(reg);
        }
        for k in 1..=eff.pops {
            reads.push(self.stack_read(k)?);
        }

        let mut writes = Vec::new();
        if eff.is_tee {
            if let Some(reg) = eff.bank_write {
                writes.push(reg);
            }
        } else {
            let base = self
                .height
                .checked_sub(eff.pops)
                .ok_or_else(|| LiftError::internal("operand-stack write underflows the stack"))?;
            for k in 0..eff.pushes {
                let depth = base.checked_add(k).ok_or_else(|| {
                    LiftError::internal("operand-stack write overflows the stack")
                })?;
                writes.push(Register::Stack(depth));
            }
            if let Some(reg) = eff.bank_write {
                writes.push(reg);
            }
        }
        Ok((reads, writes))
    }

    fn step(&mut self, op: &Operator, offset: usize) -> Result<()> {
        use Operator::{Block, Br, BrIf, BrTable, Else, End, If, Loop, Nop, Return, Unreachable};

        let pc = u32::try_from(self.instrs.len()).map_err(|_| LiftError::FunctionTooLarge {
            func_index: self.func_index,
        })?;
        let height_in = self.height;
        let reachable = self.reachable();

        match op {
            Block { blockty } => {
                let (in_arity, out_arity) = self.block_arity(blockty)?;
                self.push_instr(
                    pc,
                    height_in,
                    Vec::new(),
                    Vec::new(),
                    Successors::Fallthrough,
                );
                self.push_ctrl(in_arity, out_arity, None, None)?;
            }
            Loop { blockty } => {
                let (in_arity, out_arity) = self.block_arity(blockty)?;
                self.push_instr(
                    pc,
                    height_in,
                    Vec::new(),
                    Vec::new(),
                    Successors::Fallthrough,
                );
                let header = pc.checked_add(1).ok_or(LiftError::FunctionTooLarge {
                    func_index: self.func_index,
                })?;
                self.push_ctrl(in_arity, out_arity, Some(header), None)?;
            }
            If { blockty } => {
                let (in_arity, out_arity) = self.block_arity(blockty)?;
                let reads = if reachable {
                    alloc::vec![self.stack_read(1)?]
                } else {
                    Vec::new()
                };
                let then_pc = pc.checked_add(1).ok_or(LiftError::FunctionTooLarge {
                    func_index: self.func_index,
                })?;
                self.push_instr(
                    pc,
                    height_in,
                    reads,
                    Vec::new(),
                    Successors::Branch {
                        taken: then_pc,
                        not_taken: PENDING,
                    },
                );
                self.pop(1);
                self.push_ctrl(in_arity, out_arity, None, Some(pc as usize))?;
            }
            Else => {
                let top = self
                    .ctrl
                    .len()
                    .checked_sub(1)
                    .ok_or_else(|| LiftError::internal("`else` with no open control frame"))?;
                let (floor, in_arity, if_instr) = {
                    let frame = &mut self.ctrl[top];
                    (frame.floor, frame.in_arity, frame.if_instr.take())
                };
                let if_idx = if_instr
                    .ok_or_else(|| LiftError::internal("`else` without a matching `if`"))?;
                let else_body = pc.checked_add(1).ok_or(LiftError::FunctionTooLarge {
                    func_index: self.func_index,
                })?;
                if let Some(InstructionSchedule {
                    successors: Successors::Branch { not_taken, .. },
                    ..
                }) = self.instrs.get_mut(if_idx)
                {
                    *not_taken = else_body;
                }
                self.push_instr(
                    pc,
                    height_in,
                    Vec::new(),
                    Vec::new(),
                    Successors::Jump(PENDING),
                );
                self.ctrl[top].fixups.push(Fixup {
                    instr: pc as usize,
                    slot: Slot::Jump,
                });
                self.ctrl[top].unreachable = false;
                self.height = floor;
                self.push(in_arity)?;
            }
            End => {
                self.push_instr(
                    pc,
                    height_in,
                    Vec::new(),
                    Vec::new(),
                    Successors::Fallthrough,
                );
                if self.pop_ctrl(pc)?
                    && let Some(instr) = self.instrs.get_mut(pc as usize)
                {
                    instr.successors = Successors::Return;
                }
            }
            Br { relative_depth } => {
                let target = self.branch_target(*relative_depth, pc as usize, Slot::Jump)?;
                self.push_instr(
                    pc,
                    height_in,
                    Vec::new(),
                    Vec::new(),
                    Successors::Jump(target),
                );
                self.mark_unreachable();
            }
            BrIf { relative_depth } => {
                let reads = if reachable {
                    alloc::vec![self.stack_read(1)?]
                } else {
                    Vec::new()
                };
                self.pop(1);
                let taken = self.branch_target(*relative_depth, pc as usize, Slot::BranchTaken)?;
                let not_taken = pc.checked_add(1).ok_or(LiftError::FunctionTooLarge {
                    func_index: self.func_index,
                })?;
                self.push_instr(
                    pc,
                    height_in,
                    reads,
                    Vec::new(),
                    Successors::Branch { taken, not_taken },
                );
            }
            BrTable { targets } => {
                let reads = if reachable {
                    alloc::vec![self.stack_read(1)?]
                } else {
                    Vec::new()
                };
                self.pop(1);
                let mut resolved = Vec::new();
                for (i, depth) in targets.targets().enumerate() {
                    let depth = depth.map_err(LiftError::malformed)?;
                    resolved.push(self.branch_target(depth, pc as usize, Slot::TableEntry(i))?);
                }
                let default =
                    self.branch_target(targets.default(), pc as usize, Slot::TableDefault)?;
                self.push_instr(
                    pc,
                    height_in,
                    reads,
                    Vec::new(),
                    Successors::Table {
                        targets: resolved,
                        default,
                    },
                );
                self.mark_unreachable();
            }
            Return => {
                let reads = if reachable {
                    self.top_reads(self.result_arity)?
                } else {
                    Vec::new()
                };
                self.push_instr(pc, height_in, reads, Vec::new(), Successors::Return);
                self.mark_unreachable();
            }
            Unreachable => {
                self.push_instr(pc, height_in, Vec::new(), Vec::new(), Successors::Trap);
                self.mark_unreachable();
            }
            Nop => {
                self.push_instr(
                    pc,
                    height_in,
                    Vec::new(),
                    Vec::new(),
                    Successors::Fallthrough,
                );
            }
            _ => {
                let eff = classify_value_op(op, self.info, offset)?;
                let (reads, writes) = if reachable {
                    self.value_regs(&eff)?
                } else {
                    (Vec::new(), Vec::new())
                };
                self.push_instr(pc, height_in, reads, writes, Successors::Fallthrough);
                self.pop(eff.pops);
                self.push(eff.pushes)?;
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<LiftedFunction> {
        if !self.ctrl.is_empty() {
            return Err(LiftError::internal(
                "function body ended with an open control frame",
            ));
        }

        let width = u64::from(self.locals_count)
            .checked_add(u64::from(self.globals_count))
            .and_then(|w| w.checked_add(u64::from(self.max_height)))
            .ok_or(LiftError::RegisterFileOverflow {
                func_index: self.func_index,
                width: u64::MAX,
            })?;
        if width > u64::from(MAX_REGISTER_FILE_WIDTH) {
            return Err(LiftError::RegisterFileOverflow {
                func_index: self.func_index,
                width,
            });
        }
        let reg_file_width = u32::try_from(width).map_err(|_| LiftError::RegisterFileOverflow {
            func_index: self.func_index,
            width,
        })?;

        Ok(LiftedFunction {
            func_index: self.func_index,
            locals_count: self.locals_count,
            globals_count: self.globals_count,
            max_stack_height: self.max_height,
            reg_file_width,
            instrs: self.instrs,
        })
    }
}

fn classify_value_op(op: &Operator, info: &ModuleInfo, offset: usize) -> Result<ValueEffect> {
    use Operator::*;

    let eff = match op {
        I32Const { .. } | I64Const { .. } => ValueEffect::stack(0, 1),

        LocalGet { local_index } => ValueEffect::bank_read(Register::Local(*local_index)),
        LocalSet { local_index } => ValueEffect::bank_write(Register::Local(*local_index)),
        LocalTee { local_index } => ValueEffect::tee(Register::Local(*local_index)),
        GlobalGet { global_index } => ValueEffect::bank_read(Register::Global(*global_index)),
        GlobalSet { global_index } => ValueEffect::bank_write(Register::Global(*global_index)),

        I32Eqz | I32Clz | I32Ctz | I32Popcnt | I32WrapI64 | I64Eqz | I64Clz | I64Ctz
        | I64Popcnt | I64ExtendI32S | I64ExtendI32U => ValueEffect::stack(1, 1),

        I32Add | I32Sub | I32Mul | I32DivS | I32DivU | I32RemS | I32RemU | I32And | I32Or
        | I32Xor | I32Shl | I32ShrS | I32ShrU | I32Rotl | I32Rotr | I32Eq | I32Ne | I32LtS
        | I32LtU | I32GtS | I32GtU | I32LeS | I32LeU | I32GeS | I32GeU | I64Add | I64Sub
        | I64Mul | I64DivS | I64DivU | I64RemS | I64RemU | I64And | I64Or | I64Xor | I64Shl
        | I64ShrS | I64ShrU | I64Rotl | I64Rotr | I64Eq | I64Ne | I64LtS | I64LtU | I64GtS
        | I64GtU | I64LeS | I64LeU | I64GeS | I64GeU => ValueEffect::stack(2, 1),

        I32Load { .. }
        | I64Load { .. }
        | I32Load8S { .. }
        | I32Load8U { .. }
        | I32Load16S { .. }
        | I32Load16U { .. }
        | I64Load8S { .. }
        | I64Load8U { .. }
        | I64Load16S { .. }
        | I64Load16U { .. }
        | I64Load32S { .. }
        | I64Load32U { .. } => ValueEffect::stack(1, 1),

        I32Store { .. }
        | I64Store { .. }
        | I32Store8 { .. }
        | I32Store16 { .. }
        | I64Store8 { .. }
        | I64Store16 { .. }
        | I64Store32 { .. } => ValueEffect::stack(2, 0),

        MemorySize { .. } => ValueEffect::stack(0, 1),
        MemoryGrow { .. } => ValueEffect::stack(1, 1),

        Drop => ValueEffect::stack(1, 0),
        Select => ValueEffect::stack(3, 1),

        Call { function_index } => {
            let (pops, pushes) = info
                .func_arity(*function_index)
                .ok_or(LiftError::UnsupportedOperator { offset })?;
            ValueEffect::stack(pops, pushes)
        }
        CallIndirect { type_index, .. } => {
            let (params, results) = info
                .type_arity(*type_index)
                .ok_or(LiftError::UnsupportedOperator { offset })?;
            let pops = params
                .checked_add(1)
                .ok_or(LiftError::UnsupportedOperator { offset })?;
            ValueEffect::stack(pops, results)
        }

        _ => return Err(LiftError::UnsupportedOperator { offset }),
    };
    Ok(eff)
}
