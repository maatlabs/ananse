/// A runtime value on the WebAssembly stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    /// 32-bit integer value.
    I32(i32),
    /// 64-bit integer value.
    I64(i64),
}

impl Value {
    /// Adds two values, returning `None` if types do not match.
    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        match (self, rhs) {
            (Self::I32(left), Self::I32(right)) => Some(Self::I32(left.wrapping_add(right))),
            (Self::I64(left), Self::I64(right)) => Some(Self::I64(left.wrapping_add(right))),
            _ => None,
        }
    }

    /// Subtracts two values, returning `None` if types do not match.
    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        match (self, rhs) {
            (Self::I32(left), Self::I32(right)) => Some(Self::I32(left.wrapping_sub(right))),
            (Self::I64(left), Self::I64(right)) => Some(Self::I64(left.wrapping_sub(right))),
            _ => None,
        }
    }
}

/// A control flow label for structured control.
#[derive(Debug, Clone)]
pub struct Label {
    /// The kind of control structure.
    pub kind: LabelKind,
    /// Program counter to jump to on branch.
    pub pc: usize,
    /// Stack pointer at label entry.
    pub sp: usize,
    /// Number of result values.
    pub arity: usize,
}

/// The kind of control flow label.
#[derive(Debug, Clone, PartialEq)]
pub enum LabelKind {
    /// Label for an if block.
    If,
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Self::I32(value)
    }
}

impl TryFrom<Value> for i32 {
    type Error = Value;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::I32(v) => Ok(v),
            _ => Err(value),
        }
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::I64(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::I32(if value { 1 } else { 0 })
    }
}

impl core::ops::Add for Value {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        self.checked_add(rhs)
            .expect("type mismatch in Value addition")
    }
}

impl core::ops::Sub for Value {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        self.checked_sub(rhs)
            .expect("type mismatch in Value subtraction")
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::I32(a), Self::I32(b)) => a.partial_cmp(b),
            (Self::I64(a), Self::I64(b)) => a.partial_cmp(b),
            _ => None,
        }
    }
}
