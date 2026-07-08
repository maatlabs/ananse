use p3_goldilocks::Goldilocks as Felt;

/// A WebAssembly integer value, held as its unsigned bit pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Word {
    /// A 32-bit value, stored as its unsigned bit pattern.
    I32(u32),
    /// A 64-bit value, stored as its unsigned bit pattern.
    I64(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordType {
    I32,
    I64,
}

impl Word {
    /// This value's little-endian 32-bit limbs `(lo, hi)` as Goldilocks
    /// residues, with the value equal to `lo + hi * 2^32`.
    ///
    /// An `i32` occupies the low limb alone (`hi` is zero);
    /// an `i64` splits across both.
    pub fn to_limbs(self) -> (Felt, Felt) {
        let bits = match self {
            Word::I32(bits) => u64::from(bits),
            Word::I64(bits) => bits,
        };
        (Felt::new(bits & 0xFFFF_FFFF), Felt::new(bits >> 32))
    }

    /// Whether this value is non-zero, the WebAssembly truth value used by
    /// `if`, `br_if`, and `select`.
    pub fn is_true(self) -> bool {
        match self {
            Word::I32(bits) => bits != 0,
            Word::I64(bits) => bits != 0,
        }
    }

    /// The bit pattern widened to `u64` together with the value's bit width.
    pub fn raw(self) -> (u64, u32) {
        match self {
            Word::I32(bits) => (u64::from(bits), 32),
            Word::I64(bits) => (bits, 64),
        }
    }
}

/// A single `(module, name)` import declared by a [`Module`](crate::Module).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportEntry {
    /// The import's module namespace.
    pub module: String,
    /// The imported item's name.
    pub name: String,
}

/// A single export declared by a [`Module`](crate::Module).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportEntry {
    /// The export's name.
    pub name: String,
    /// The kind of item being exported.
    pub kind: ExportKind,
    /// The index of the exported item within its index space.
    pub index: u32,
}

/// The kind of item an [`ExportEntry`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    /// A function export.
    Function,
    /// A table export.
    Table,
    /// A linear-memory export.
    Memory,
    /// A global export.
    Global,
}
