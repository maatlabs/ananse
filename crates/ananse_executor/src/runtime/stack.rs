use ananse_decoder::Word;

use crate::{ExecuteError, Result};

#[derive(Default)]
pub struct OperandStack(Vec<Word>);

impl OperandStack {
    pub fn pop(&mut self) -> Result<Word> {
        self.0
            .pop()
            .ok_or_else(|| ExecuteError::invalid_binary("operand stack underflow"))
    }

    pub fn pop_u32(&mut self) -> Result<u32> {
        Ok(match self.pop()? {
            Word::I32(v) => v,
            Word::I64(v) => v as u32,
        })
    }

    pub fn push(&mut self, w: Word) {
        self.0.push(w)
    }

    pub fn take_top(&mut self, n: u32) -> Result<Vec<Word>> {
        let at = self
            .0
            .len()
            .checked_sub(n as usize)
            .ok_or_else(|| ExecuteError::invalid_binary("arity exceeds the stack height"))?;
        Ok(self.0.split_off(at))
    }

    pub fn extend(&mut self, iter: Vec<Word>) {
        self.0.extend(iter);
    }

    pub fn unwind(&mut self, target_height: usize, arity: usize) -> Result<()> {
        let keep = target_height.checked_sub(arity).ok_or_else(|| {
            ExecuteError::invalid_binary("branch arity exceeds the target height")
        })?;
        let from =
            self.0.len().checked_sub(arity).ok_or_else(|| {
                ExecuteError::invalid_binary("branch arity exceeds the stack height")
            })?;
        let carried = self.0.split_off(from);
        self.0.truncate(keep);
        self.0.extend(carried);
        Ok(())
    }

    pub fn as_slice(&self) -> &[Word] {
        &self.0
    }

    pub fn last(&self) -> Option<&Word> {
        self.0.last()
    }

    // the height check, moved in from the loop body — same philosophy as
    // `unwind`: the invariant lives on the type, not at every call site
    pub fn expect_height(&self, expected: u32, func_index: u32, pc: u32) -> Result<()> {
        if self.0.len() != expected as usize {
            return Err(ExecuteError::ScheduleMismatch {
                func_index,
                pc,
                schedule: expected,
                actual: self.0.len() as u32,
            });
        }
        Ok(())
    }
}
