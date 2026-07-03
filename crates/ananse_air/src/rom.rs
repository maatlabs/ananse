//! The program ROM: the committed table the control-flow and layout lookup binds against.

use core::iter::once;

use ananse_executor::OpCode;
use ananse_lift::{LiftedFunction, Register, Successors};
use ananse_trace::selector::{NUM_SELECTORS, SEL_PADDING, opcode_index};
use p3_goldilocks::Goldilocks as Felt;

use crate::AirError;

/// Radix of the opcode digit: the selector-column count, so every opcode index
/// (including the padding selector) is a valid digit.
pub(crate) const OPCODE_RADIX: u64 = NUM_SELECTORS as u64;
/// Radix of the two program-counter digits.
const PC_RADIX: u64 = 1 << 18;
/// Radix of the operand-stack-height and local/global-offset digits.
const REG_RADIX: u64 = 1 << 10;
/// Place value of the `next_pc` digit.
pub(crate) const NEXT_PC_PLACE: u64 = OPCODE_RADIX * PC_RADIX;
/// Place value of the `height` digit.
pub(crate) const HEIGHT_PLACE: u64 = NEXT_PC_PLACE * PC_RADIX;
/// Place value of the `imm` (local/global offset) digit.
pub(crate) const IMM_PLACE: u64 = HEIGHT_PLACE * REG_RADIX;

/// Builds the program ROM for a lifted function: every valid schedule edge
/// `(pc, opcode, next_pc, height, imm)` packed into one field element, sorted and
/// deduplicated so the prover and verifier produce a byte-identical table.
pub fn program_rom(opcodes: &[OpCode], function: &LiftedFunction) -> Result<Vec<Felt>, AirError> {
    let body_len = function.instrs.len();
    if opcodes.len() != body_len {
        return Err(AirError::ScheduleLengthMismatch {
            opcodes: opcodes.len(),
            schedule: body_len,
        });
    }
    let halt_pc = u64::try_from(body_len).map_err(|_| AirError::ProgramTooLarge { body_len })?;
    let reg_width = u64::from(function.reg_file_width);
    if halt_pc >= PC_RADIX || reg_width >= REG_RADIX {
        return Err(AirError::ProgramTooLarge { body_len });
    }

    let mut packed = function
        .instrs
        .iter()
        .enumerate()
        .flat_map(|(pc, sched)| {
            // `pc < body_len <= halt_pc < PC_RADIX`, and both `height_in` and the
            // offset are below the register-file width `< REG_RADIX`, so every digit
            // stays within its radix.
            let opcode = opcodes[pc];
            let opcode_id = opcode_index(opcode) as u64;
            let height = u64::from(sched.height_in);
            let imm = immediate_offset(function, pc);
            let mut targets = successor_targets(&sched.successors, pc as u64, halt_pc);
            // A call either returns (its scheduled fall-through) or halts the program
            // through a host `proc_exit`; the halt outcome is a valid edge.
            if matches!(opcode, OpCode::Call) {
                targets.push(halt_pc);
            }
            targets
                .into_iter()
                .map(move |next_pc| (pc as u64, opcode_id, next_pc, height, imm))
        })
        .map(|(pc, opcode_id, next_pc, height, imm)| {
            pack(pc, opcode_id, next_pc, height, imm).ok_or(AirError::ProgramTooLarge { body_len })
        })
        .collect::<Result<Vec<u64>, _>>()?;

    packed.push(
        pack(halt_pc, SEL_PADDING as u64, halt_pc, 0, 0)
            .ok_or(AirError::ProgramTooLarge { body_len })?,
    );

    packed.sort_unstable();
    packed.dedup();
    Ok(packed.into_iter().map(Felt::new).collect())
}

/// Packs a single executed edge `(pc, opcode, next_pc, height, imm)` into its ROM
/// element, or `None` if any component lies outside the injective range. The lookup
/// side packs each executed row's edge with this and checks membership against
/// [`program_rom()`]'s table.
pub fn pack_edge(pc: u32, opcode_id: usize, next_pc: u32, height: u32, imm: u32) -> Option<Felt> {
    let opcode_id = u64::try_from(opcode_id).ok()?;
    let (pc, next_pc, height, imm) = (
        u64::from(pc),
        u64::from(next_pc),
        u64::from(height),
        u64::from(imm),
    );
    if opcode_id >= OPCODE_RADIX
        || pc >= PC_RADIX
        || next_pc >= PC_RADIX
        || height >= REG_RADIX
        || imm >= REG_RADIX
    {
        return None;
    }
    pack(pc, opcode_id, next_pc, height, imm).map(Felt::new)
}

/// Packs one schedule edge into a field-element value. Returns `None` only if a
/// digit exceeds its radix, which [`program_rom()`] rules out up front.
fn pack(pc: u64, opcode_id: u64, next_pc: u64, height: u64, imm: u64) -> Option<u64> {
    imm.checked_mul(IMM_PLACE)?
        .checked_add(height.checked_mul(HEIGHT_PLACE)?)?
        .checked_add(next_pc.checked_mul(NEXT_PC_PLACE)?)?
        .checked_add(pc.checked_mul(OPCODE_RADIX)?)?
        .checked_add(opcode_id)
}

/// The register-file offset of the local or global slot the scheduled operator at
/// `pc` touches, or zero when it touches none. Mirrors the trace's resolution: a
/// local keeps its index, a global sits above the locals.
fn immediate_offset(function: &LiftedFunction, pc: usize) -> u64 {
    let schedule = &function.instrs[pc];
    schedule
        .reads
        .iter()
        .chain(&schedule.writes)
        .find_map(|reg| match reg {
            Register::Local(index) => Some(u64::from(*index)),
            Register::Global(index) => {
                Some(u64::from(function.locals_count).saturating_add(u64::from(*index)))
            }
            Register::Stack(_) => None,
        })
        .unwrap_or(0)
}

/// The in-body successor program points an operator can move control to. Terminal
/// successors collapse to the exit sentinel `halt_pc`.
fn successor_targets(successors: &Successors, pc: u64, halt_pc: u64) -> Vec<u64> {
    match successors {
        // `pc < PC_RADIX`, so the fall-through target cannot overflow.
        Successors::Fallthrough => vec![pc.saturating_add(1)],
        Successors::Jump(target) => vec![u64::from(*target)],
        Successors::Branch { taken, not_taken } => vec![u64::from(*taken), u64::from(*not_taken)],
        Successors::Table { targets, default } => targets
            .iter()
            .chain(once(default))
            .map(|target| u64::from(*target))
            .collect(),
        Successors::Return | Successors::Trap => vec![halt_pc],
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn packing_distinct_edges_gives_distinct_values() {
        let mut seen = HashSet::new();
        for pc in [0, 1, 2, 100, PC_RADIX - 1] {
            for opcode_id in [0, 1, OPCODE_RADIX - 1] {
                for next_pc in [0, 1, 100, PC_RADIX - 1] {
                    for height in [0, 1, REG_RADIX - 1] {
                        for imm in [0, REG_RADIX - 1] {
                            let value =
                                pack(pc, opcode_id, next_pc, height, imm).expect("within radices");
                            assert!(
                                seen.insert(value),
                                "collision at ({pc}, {opcode_id}, {next_pc}, {height}, {imm})"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn largest_edge_stays_below_the_goldilocks_prime() {
        let max = pack(
            PC_RADIX - 1,
            OPCODE_RADIX - 1,
            PC_RADIX - 1,
            REG_RADIX - 1,
            REG_RADIX - 1,
        )
        .expect("within radices");
        assert!(max < (1u64 << 63));
    }

    #[test]
    fn out_of_range_components_do_not_pack() {
        assert!(pack_edge(0, NUM_SELECTORS, 0, 0, 0).is_none());
        assert!(pack_edge(u32::try_from(PC_RADIX).unwrap(), 0, 0, 0, 0).is_none());
        assert!(pack_edge(0, 0, 0, u32::try_from(REG_RADIX).unwrap(), 0).is_none());
        assert!(pack_edge(0, 0, 0, 0, u32::try_from(REG_RADIX).unwrap()).is_none());
    }
}
