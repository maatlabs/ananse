#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    I32(i32),
    I64(i64),
}

#[derive(Debug, Clone)]
pub struct Label {
    pub kind: LabelKind,
    pub pc: usize,
    pub sp: usize,
    pub arity: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LabelKind {
    If,
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Self::I32(value)
    }
}

impl From<Value> for i32 {
    fn from(value: Value) -> Self {
        match value {
            Value::I32(v) => v,
            _ => panic!("type mismatch"),
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
        match (self, rhs) {
            (Self::I32(left), Self::I32(right)) => Self::I32(left + right),
            (Self::I64(left), Self::I64(right)) => Self::I64(left + right),
            _ => panic!("type mismatch"),
        }
    }
}

impl core::ops::Sub for Value {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Self::I32(left), Self::I32(right)) => Self::I32(left - right),
            (Self::I64(left), Self::I64(right)) => Self::I64(left - right),
            _ => panic!("type mismatch"),
        }
    }
}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Self::I32(a), Self::I32(b)) => a.partial_cmp(b),
            (Self::I64(a), Self::I64(b)) => a.partial_cmp(b),
            _ => panic!("type mismatch"),
        }
    }
}
