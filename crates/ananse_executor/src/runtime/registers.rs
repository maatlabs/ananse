use ananse_decoder::Word;
use ananse_lift::Register;

use crate::{ExecuteError, Result};

pub struct Registers<'a> {
    pub stack: &'a [Word],
    pub locals: &'a [Word],
    pub globals: &'a [Word],
}

impl Registers<'_> {
    pub fn read(&self, reg: Register) -> Result<Word> {
        let value = match reg {
            Register::Local(i) => self.locals.get(i as usize).copied(),
            Register::Global(g) => self.globals.get(g as usize).copied(),
            Register::Stack(d) => self.stack.get(d as usize).copied(),
        };
        value.ok_or_else(|| ExecuteError::invalid_binary("register access out of range"))
    }
}
