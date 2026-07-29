use crate::{ExecuteError, Result};

/// A runtime control label, tracked only for the data a branch needs: the values
/// it carries and where it lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Label {
    /// Whether the label is a `loop` (its branch target is its own header).
    pub is_loop: bool,
    /// Values a branch to this label carries: the loop's input arity, or a
    /// forward block's result arity.
    pub branch_arity: u32,
    /// Program point of the label's matching `end`.
    pub end_pc: u32,
}

#[derive(Debug, Default)]
pub struct Labels(Vec<Label>);

impl Labels {
    /// Pops every label whose scope has closed by the time control reaches
    /// `target`---i.e. every label whose `end_pc` is strictly less than `target`.
    pub fn pop_past(&mut self, target: usize) {
        while self
            .0
            .last()
            .is_some_and(|label| (label.end_pc as usize) < target)
        {
            self.0.pop();
        }
    }

    pub fn push(&mut self, label: Label) {
        self.0.push(label)
    }

    /// Resolves a branch `relative_depth` levels up from the innermost label
    /// (`0` = the current label), per WebAssembly's label-index encoding.
    ///
    /// Returns the target label and the stack length to truncate to should
    /// the branch be taken---the label itself for a `loop` (so it remains
    /// live to catch a subsequent iteration), or everything above it for a
    /// forward block.
    pub fn resolve_branch(&self, relative_depth: u32) -> Result<(&Label, usize)> {
        let idx = self
            .0
            .len()
            .checked_sub(1)
            .and_then(|top| top.checked_sub(relative_depth as usize))
            .ok_or_else(|| {
                ExecuteError::invalid_binary("branch depth exceeds the control stack")
            })?;
        let label = &self.0[idx];
        let truncate_to = if label.is_loop { idx + 1 } else { idx };
        Ok((label, truncate_to))
    }

    pub fn truncate(&mut self, len: usize) {
        self.0.truncate(len)
    }
}
