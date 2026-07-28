use ananse_decoder::{OpCode, Word};

use crate::record::Outcome;
use crate::{Result, Trap, memory};

/// A two-operand (binary) integer arithmetic, abstract over the `i32` / `i64`
/// width carried by its operands.
#[derive(Clone, Copy)]
pub(crate) enum Binary {
    Add,
    Sub,
    Mul,
    DivS,
    DivU,
    RemS,
    RemU,
    And,
    Or,
    Xor,
    Shl,
    ShrS,
    ShrU,
    Rotl,
    Rotr,
}

/// A two-operand integer comparison, producing an `i32` boolean.
#[derive(Clone, Copy)]
pub(crate) enum Compare {
    Eq,
    Ne,
    LtS,
    LtU,
    GtS,
    GtU,
    LeS,
    LeU,
    GeS,
    GeU,
}

/// A single-operand integer operation.
#[derive(Clone, Copy)]
pub(crate) enum Unary {
    /// "Equal to zero". Pops a value, pushes 1 if it was 0, else pushes 0.
    Eqz,
    /// "Count leading zeros". Counts how many zero bits are at the
    /// most-significant end before the first 1 bit.
    Clz,
    /// "Count trailing zeros". Counts zero bits starting from the
    /// least-significant end until the first 1.
    Ctz,
    /// "Population count". Counts the total number of 1 bits in the value,
    /// regardless of position.
    Popcnt,
}

/// Applies a binary arithmetic or bitwise operator to two same-width operands.
///
/// Shift and rotate counts are reduced modulo the operand width,
/// matching the WebAssembly specification.
///
/// # Errors
///
/// Division and remainder trap on a zero divisor; signed division traps on the
/// `MIN / -1` overflow.
pub fn binop(stack: &mut Vec<Word>, kind: Binary, opcode: OpCode, pc: usize) -> Result<Outcome> {
    let rhs = memory::stack_pop(stack)?;
    let lhs = memory::stack_pop(stack)?;
    stack.push(arithmetic(kind, lhs, rhs)?);
    Ok(Outcome::advance(opcode, pc + 1))
}

/// Applies a comparison to two same-width operands, yielding an `i32` `0` or `1`.
pub fn cmpop(stack: &mut Vec<Word>, kind: Compare, opcode: OpCode, pc: usize) -> Result<Outcome> {
    let rhs = memory::stack_pop(stack)?;
    let lhs = memory::stack_pop(stack)?;
    stack.push(compare(kind, lhs, rhs));
    Ok(Outcome::advance(opcode, pc + 1))
}

/// Applies a unary operator. `eqz` yields an `i32`; the bit-count operators
/// preserve the operand width.
pub fn unop(stack: &mut Vec<Word>, kind: Unary, opcode: OpCode, pc: usize) -> Result<Outcome> {
    let operand = memory::stack_pop(stack)?;
    stack.push(unary(kind, operand));
    Ok(Outcome::advance(opcode, pc + 1))
}

fn arithmetic(kind: Binary, lhs: Word, rhs: Word) -> core::result::Result<Word, Trap> {
    let (x, width) = lhs.raw();
    let (y, _) = rhs.raw();
    let m = mask(width);
    let result = match kind {
        Binary::Add => x.wrapping_add(y) & m,
        Binary::Sub => x.wrapping_sub(y) & m,
        Binary::Mul => x.wrapping_mul(y) & m,
        Binary::DivU => (x.checked_div(y).ok_or(Trap::DivideByZero)?) & m,
        Binary::RemU => x.checked_rem(y).ok_or(Trap::DivideByZero)?,
        Binary::DivS => {
            if y == 0 {
                return Err(Trap::DivideByZero);
            }
            let (a, b) = (signed(x, width), signed(y, width));
            if a == signed_min(width) && b == -1 {
                return Err(Trap::IntegerOverflow);
            }
            (a.wrapping_div(b) as u64) & m
        }
        Binary::RemS => {
            if y == 0 {
                return Err(Trap::DivideByZero);
            }
            let (a, b) = (signed(x, width), signed(y, width));
            // `MIN % -1` is defined as zero and must not trap.
            if a == signed_min(width) && b == -1 {
                0
            } else {
                (a.wrapping_rem(b) as u64) & m
            }
        }
        Binary::And => x & y,
        Binary::Or => x | y,
        Binary::Xor => x ^ y,
        Binary::Shl => {
            let k = (y % u64::from(width)) as u32;
            (x << k) & m
        }
        Binary::ShrU => {
            let k = (y % u64::from(width)) as u32;
            (x & m) >> k
        }
        Binary::ShrS => {
            let k = (y % u64::from(width)) as u32;
            ((signed(x, width) >> k) as u64) & m
        }
        Binary::Rotl => rotate(x, y, width, true),
        Binary::Rotr => rotate(x, y, width, false),
    };

    Ok(retag(result, width))
}

fn compare(kind: Compare, lhs: Word, rhs: Word) -> Word {
    let (x, width) = lhs.raw();
    let (y, _) = rhs.raw();
    let truth = match kind {
        Compare::Eq => x == y,
        Compare::Ne => x != y,
        Compare::LtU => x < y,
        Compare::GtU => x > y,
        Compare::LeU => x <= y,
        Compare::GeU => x >= y,
        Compare::LtS => signed(x, width) < signed(y, width),
        Compare::GtS => signed(x, width) > signed(y, width),
        Compare::LeS => signed(x, width) <= signed(y, width),
        Compare::GeS => signed(x, width) >= signed(y, width),
    };
    Word::I32(u32::from(truth))
}

fn unary(kind: Unary, operand: Word) -> Word {
    let (x, width) = operand.raw();
    match kind {
        Unary::Eqz => Word::I32(u32::from(x == 0)),
        Unary::Clz => retag(leading_zeros(x, width), width),
        Unary::Ctz => retag(trailing_zeros(x, width), width),
        Unary::Popcnt => retag(u64::from(x.count_ones()), width),
    }
}

/// Re-tags a masked `u64` result as a [`Word`] of the given width. The narrowing
/// cast keeps the low 32 bits, which is the defined `i32` representation.
fn retag(value: u64, width: u32) -> Word {
    if width == 32 {
        Word::I32(value as u32)
    } else {
        Word::I64(value)
    }
}

/// Sign-extends the low `width` bits of `value` to a signed 64-bit integer.
fn signed(value: u64, width: u32) -> i64 {
    if width == 32 {
        i64::from(value as u32 as i32)
    } else {
        value as i64
    }
}

/// The most-negative signed value representable in `width` bits.
fn signed_min(width: u32) -> i64 {
    if width == 32 {
        i64::from(i32::MIN)
    } else {
        i64::MIN
    }
}

/// Rotates the low `width` bits of `x` by `y mod width` places.
fn rotate(x: u64, y: u64, width: u32, left: bool) -> u64 {
    let k = (y % u64::from(width)) as u32;
    if width == 32 {
        let v = x as u32;
        u64::from(if left {
            v.rotate_left(k)
        } else {
            v.rotate_right(k)
        })
    } else if left {
        x.rotate_left(k)
    } else {
        x.rotate_right(k)
    }
}

fn leading_zeros(x: u64, width: u32) -> u64 {
    if width == 32 {
        u64::from((x as u32).leading_zeros())
    } else {
        u64::from(x.leading_zeros())
    }
}

fn trailing_zeros(x: u64, width: u32) -> u64 {
    if width == 32 {
        u64::from((x as u32).trailing_zeros())
    } else {
        u64::from(x.trailing_zeros())
    }
}

/// Low `width`-bit mask.
fn mask(width: u32) -> u64 {
    if width == 32 {
        u64::from(u32::MAX)
    } else {
        u64::MAX
    }
}

pub fn sign_extend(raw: u64, bytes: usize, signed: bool, result64: bool) -> Word {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn i32(value: i32) -> Word {
        Word::I32(value as u32)
    }

    #[test]
    fn shift_counts_wrap_modulo_width() {
        assert_eq!(
            arithmetic(Binary::Shl, Word::I32(1), Word::I32(33)),
            Ok(Word::I32(2))
        );
        assert_eq!(
            arithmetic(Binary::Shl, Word::I64(1), Word::I64(65)),
            Ok(Word::I64(2))
        );
    }

    #[test]
    fn shr_s_is_arithmetic_while_shr_u_is_logical() {
        assert_eq!(arithmetic(Binary::ShrS, i32(-2), Word::I32(1)), Ok(i32(-1)));
        assert_eq!(
            arithmetic(Binary::ShrU, i32(-2), Word::I32(1)),
            Ok(Word::I32(0x7FFF_FFFF))
        );
    }

    #[test]
    fn rotate_wraps_bits_around_the_width() {
        assert_eq!(
            arithmetic(Binary::Rotl, Word::I32(0x8000_0000), Word::I32(1)),
            Ok(Word::I32(1))
        );
        assert_eq!(
            arithmetic(Binary::Rotr, Word::I32(1), Word::I32(1)),
            Ok(Word::I32(0x8000_0000))
        );
    }

    #[test]
    fn signed_remainder_of_min_by_minus_one_is_zero() {
        assert_eq!(
            arithmetic(Binary::RemS, i32(i32::MIN), i32(-1)),
            Ok(Word::I32(0))
        );
    }

    #[test]
    fn division_and_remainder_by_zero_trap() {
        assert_eq!(
            arithmetic(Binary::DivU, Word::I64(7), Word::I64(0)),
            Err(Trap::DivideByZero)
        );
        assert_eq!(
            arithmetic(Binary::RemS, Word::I32(7), Word::I32(0)),
            Err(Trap::DivideByZero)
        );
    }

    #[test]
    fn comparison_distinguishes_signedness() {
        assert_eq!(compare(Compare::LtS, i32(-1), Word::I32(0)), Word::I32(1));
        assert_eq!(compare(Compare::LtU, i32(-1), Word::I32(0)), Word::I32(0));
    }

    #[test]
    fn bit_counts_respect_the_operand_width() {
        assert_eq!(unary(Unary::Ctz, Word::I32(0)), Word::I32(32));
        assert_eq!(unary(Unary::Ctz, Word::I64(0)), Word::I64(64));
        assert_eq!(unary(Unary::Clz, Word::I32(1)), Word::I32(31));
        assert_eq!(unary(Unary::Popcnt, Word::I64(0xFF)), Word::I64(8));
    }

    #[test]
    fn eqz_yields_an_i32_for_both_widths() {
        assert_eq!(unary(Unary::Eqz, Word::I64(0)), Word::I32(1));
        assert_eq!(unary(Unary::Eqz, Word::I64(5)), Word::I32(0));
    }
}
