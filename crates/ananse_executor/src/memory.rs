use ananse_decoder::Word;
use ananse_lift::Register;

use crate::record::RtFrame;
use crate::{ExecuteError, RegAccess, Result};

pub fn stack_pop(stack: &mut Vec<Word>) -> Result<Word> {
    stack
        .pop()
        .ok_or_else(|| ExecuteError::invalid_binary("operand stack underflow"))
}

pub fn stack_pop_u32(stack: &mut Vec<Word>) -> Result<u32> {
    Ok(match stack_pop(stack)? {
        Word::I32(v) => v,
        Word::I64(v) => v as u32,
    })
}

pub fn stack_take_top(stack: &mut Vec<Word>, n: u32) -> Result<Vec<Word>> {
    let at = stack
        .len()
        .checked_sub(n as usize)
        .ok_or_else(|| ExecuteError::invalid_binary("arity exceeds the stack height"))?;
    Ok(stack.split_off(at))
}

pub fn stack_unwind(stack: &mut Vec<Word>, target_height: usize, arity: usize) -> Result<()> {
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

pub fn frame_pop(frames: &mut Vec<RtFrame>, next: usize) {
    while frames
        .last()
        .is_some_and(|frame| (frame.end_pc as usize) < next)
    {
        frames.pop();
    }
}

pub fn resolve_register_access(
    regs: &[Register],
    stack: &[Word],
    locals: &[Word],
    globals: &[Word],
) -> Result<Vec<RegAccess>> {
    regs.iter()
        .map(|&reg| {
            Ok(RegAccess {
                reg,
                value: read_register(reg, stack, locals, globals)?,
            })
        })
        .collect()
}

fn read_register(reg: Register, stack: &[Word], locals: &[Word], globals: &[Word]) -> Result<Word> {
    let value = match reg {
        Register::Local(i) => locals.get(i as usize).copied(),
        Register::Global(g) => globals.get(g as usize).copied(),
        Register::Stack(d) => stack.get(d as usize).copied(),
    };
    value.ok_or_else(|| ExecuteError::invalid_binary("register access out of range"))
}
