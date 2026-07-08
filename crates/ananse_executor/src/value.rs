use ananse_decoder::Word;

use crate::Trap;

/// A two-operand integer operator, abstract over the `i32` / `i64` width carried
/// by its operands.
#[derive(Clone, Copy)]
pub(crate) enum Arithmetic {
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

/// A single-operand integer operator.
#[derive(Clone, Copy)]
pub(crate) enum Unary {
    Eqz,
    Clz,
    Ctz,
    Popcnt,
}

/// Applies a binary arithmetic or bitwise operator to two same-width operands.
///
/// Division and remainder trap on a zero divisor; signed division traps on the
/// `MIN / -1` overflow. Shift and rotate counts are reduced modulo the operand
/// width, matching the WebAssembly specification.
pub(crate) fn arithmetic(kind: Arithmetic, lhs: Word, rhs: Word) -> Result<Word, Trap> {
    let (x, width) = lhs.raw();
    let (y, _) = rhs.raw();
    let m = mask(width);
    let result = match kind {
        Arithmetic::Add => x.wrapping_add(y) & m,
        Arithmetic::Sub => x.wrapping_sub(y) & m,
        Arithmetic::Mul => x.wrapping_mul(y) & m,
        Arithmetic::DivU => (x.checked_div(y).ok_or(Trap::DivideByZero)?) & m,
        Arithmetic::RemU => x.checked_rem(y).ok_or(Trap::DivideByZero)?,
        Arithmetic::DivS => {
            if y == 0 {
                return Err(Trap::DivideByZero);
            }
            let (a, b) = (signed(x, width), signed(y, width));
            if a == signed_min(width) && b == -1 {
                return Err(Trap::IntegerOverflow);
            }
            (a.wrapping_div(b) as u64) & m
        }
        Arithmetic::RemS => {
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
        Arithmetic::And => x & y,
        Arithmetic::Or => x | y,
        Arithmetic::Xor => x ^ y,
        Arithmetic::Shl => {
            let k = (y % u64::from(width)) as u32;
            (x << k) & m
        }
        Arithmetic::ShrU => {
            let k = (y % u64::from(width)) as u32;
            (x & m) >> k
        }
        Arithmetic::ShrS => {
            let k = (y % u64::from(width)) as u32;
            ((signed(x, width) >> k) as u64) & m
        }
        Arithmetic::Rotl => rotate(x, y, width, true),
        Arithmetic::Rotr => rotate(x, y, width, false),
    };
    Ok(retag(result, width))
}

/// Applies a comparison to two same-width operands, yielding an `i32` `0` or `1`.
pub(crate) fn compare(kind: Compare, lhs: Word, rhs: Word) -> Word {
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

/// Applies a unary operator. `eqz` yields an `i32`; the bit-count operators
/// preserve the operand width.
pub(crate) fn unary(kind: Unary, operand: Word) -> Word {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn i32(value: i32) -> Word {
        Word::I32(value as u32)
    }

    #[test]
    fn shift_counts_wrap_modulo_width() {
        assert_eq!(
            arithmetic(Arithmetic::Shl, Word::I32(1), Word::I32(33)),
            Ok(Word::I32(2))
        );
        assert_eq!(
            arithmetic(Arithmetic::Shl, Word::I64(1), Word::I64(65)),
            Ok(Word::I64(2))
        );
    }

    #[test]
    fn shr_s_is_arithmetic_while_shr_u_is_logical() {
        assert_eq!(
            arithmetic(Arithmetic::ShrS, i32(-2), Word::I32(1)),
            Ok(i32(-1))
        );
        assert_eq!(
            arithmetic(Arithmetic::ShrU, i32(-2), Word::I32(1)),
            Ok(Word::I32(0x7FFF_FFFF))
        );
    }

    #[test]
    fn rotate_wraps_bits_around_the_width() {
        assert_eq!(
            arithmetic(Arithmetic::Rotl, Word::I32(0x8000_0000), Word::I32(1)),
            Ok(Word::I32(1))
        );
        assert_eq!(
            arithmetic(Arithmetic::Rotr, Word::I32(1), Word::I32(1)),
            Ok(Word::I32(0x8000_0000))
        );
    }

    #[test]
    fn signed_remainder_of_min_by_minus_one_is_zero() {
        assert_eq!(
            arithmetic(Arithmetic::RemS, i32(i32::MIN), i32(-1)),
            Ok(Word::I32(0))
        );
    }

    #[test]
    fn division_and_remainder_by_zero_trap() {
        assert_eq!(
            arithmetic(Arithmetic::DivU, Word::I64(7), Word::I64(0)),
            Err(Trap::DivideByZero)
        );
        assert_eq!(
            arithmetic(Arithmetic::RemS, Word::I32(7), Word::I32(0)),
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
