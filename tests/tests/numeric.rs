use ananse_executor::{Entry, OpCode, Word};
use ananse_tests::{air_for, trace_and_rom_entry, transition_violations, wat_from_file};
use ananse_trace::layout::{bus_slot, slot};
use maat_field::{Felt, FieldElement};

/// The numeric family occupies the final main transition constraints.
const NUMERIC_CONSTRAINTS: usize = 5;

/// Runs `export(args)`, confirms the honest trace satisfies the transition system,
/// then corrupts one limb of the operator's result on the value bus and confirms the
/// numeric family localizes the break to the operator's own row and one of its own
/// constraints. `tamper_hi` selects the high limb (the inter-limb carry / borrow and
/// the `i32` zeroing) over the low limb (the addition / subtraction balance).
fn assert_numeric_relation(
    fixture: &str,
    export: &str,
    args: &[Word],
    opcode: OpCode,
    tamper_hi: bool,
) {
    let (trace, rom) =
        trace_and_rom_entry(&wat_from_file(fixture), &Entry::Export(export.into()), args);
    let air = air_for(&trace, rom);
    assert!(
        transition_violations(&air, trace.columns(), trace.length()).is_empty(),
        "{export}{args:?}: honest trace violates the transition system"
    );

    let row = trace
        .records()
        .iter()
        .position(|r| r.opcode == opcode)
        .expect("opcode present");
    // A binary operator writes its result on value-bus slot 2 of its own row; nudge
    // the chosen limb away from its correct value.
    let column = bus_slot(2) + if tamper_hi { slot::HI } else { slot::LO };
    let mut columns = trace.columns().to_vec();
    columns[column][row] += Felt::ONE;

    let numeric_base = air.num_main_transition_constraints() - NUMERIC_CONSTRAINTS;
    let violations = transition_violations(&air, &columns, trace.length());
    assert!(
        violations
            .iter()
            .any(|&(r, c)| r == row && c >= numeric_base),
        "{export}{args:?} (tamper_hi={tamper_hi}): numeric family missed the corruption, got {violations:?}"
    );
}

#[test]
fn i32_add_relation_holds_and_catches_a_wrong_result() {
    for tamper_hi in [false, true] {
        assert_numeric_relation(
            "func_add.wat",
            "add",
            &[Word::I32(7), Word::I32(5)],
            OpCode::I32Add,
            tamper_hi,
        );
    }
    // The carry path: the low limbs sum past 2^32 and the result wraps to zero.
    assert_numeric_relation(
        "func_add.wat",
        "add",
        &[Word::I32(u32::MAX), Word::I32(1)],
        OpCode::I32Add,
        false,
    );
}

#[test]
fn i32_sub_relation_holds_and_catches_a_wrong_result() {
    for tamper_hi in [false, true] {
        assert_numeric_relation(
            "func_sub.wat",
            "sub",
            &[Word::I32(7), Word::I32(5)],
            OpCode::I32Sub,
            tamper_hi,
        );
    }
    // The borrow path: the subtraction underflows and wraps to `2^32 - 2`.
    assert_numeric_relation(
        "func_sub.wat",
        "sub",
        &[Word::I32(5), Word::I32(7)],
        OpCode::I32Sub,
        false,
    );
}

#[test]
fn i64_add_relation_holds_and_catches_a_wrong_result() {
    // The low limbs carry into the high limb, so both limbs are non-trivial.
    for tamper_hi in [false, true] {
        assert_numeric_relation(
            "func_add_i64.wat",
            "add",
            &[Word::I64(0xFFFF_FFFF), Word::I64(1)],
            OpCode::I64Add,
            tamper_hi,
        );
    }
    // The high limbs overflow past 2^64 and the carry out is dropped.
    assert_numeric_relation(
        "func_add_i64.wat",
        "add",
        &[Word::I64(0xFFFF_FFFF_0000_0000), Word::I64(0x1_0000_0000)],
        OpCode::I64Add,
        true,
    );
}

#[test]
fn i64_sub_relation_holds_and_catches_a_wrong_result() {
    // The low limb borrows from the high limb.
    for tamper_hi in [false, true] {
        assert_numeric_relation(
            "func_sub_i64.wat",
            "sub",
            &[Word::I64(0x1_0000_0000), Word::I64(1)],
            OpCode::I64Sub,
            tamper_hi,
        );
    }
}
