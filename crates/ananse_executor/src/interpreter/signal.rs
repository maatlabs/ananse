use ananse_decoder::{OpCode, Word};

use crate::{MemAccess, Transition};

/// The record-bearing result of executing one instruction.
pub struct StepOutcome {
    pub opcode: OpCode,
    pub transition: Transition,
    pub mem: Vec<MemAccess>,
    pub control: Signal,
}

impl StepOutcome {
    pub fn advance(opcode: OpCode, next: usize) -> Self {
        Self {
            opcode,
            transition: Transition::Next(next as u32),
            mem: Vec::new(),
            control: Signal::Advance(next),
        }
    }
}

/// What executing a single instruction produced.
pub enum Signal {
    Advance(usize),
    Return(Vec<Word>),
    Exit(i32),
}

impl Signal {
    pub fn transition(&self) -> Transition {
        match self {
            Self::Advance(next) => Transition::Next(*next as u32),
            Self::Return(_) => Transition::Return,
            Self::Exit(code) => Transition::Exit(*code),
        }
    }
}

/// What a function evaluation produced.
pub enum Completion {
    Return(Vec<Word>),
    Exit(i32),
}
