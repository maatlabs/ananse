use ananse_air::{AnanseAir, pack_edge, program_rom};
use ananse_decoder::{Felt, Module, OpCode, Word, function_opcodes};
use ananse_executor::{Entry, Transition, execute};
use ananse_lift::{Register, lift};
use ananse_tests::{
    SINGLE_FRAME_FIXTURES, air_for, failing_rows, main_matrix, mock_challenges, permutation_of,
    trace_and_rom, trace_and_rom_entry, wat_from_fixture,
};
use ananse_trace::Trace;
use ananse_trace::layout::{
    BUS_SLOTS, BW_P_BASE, COL_CLK, COL_HEIGHT, COL_PC, PC_DATA_A, PC_DATA_B, RC_WRITE_LO,
    SELECTOR_BASE, bus_slot, rc_gap, slot, sorted, sorted_slot,
};
use ananse_trace::selector::{NUM_SELECTORS, SEL_PADDING, opcode_index};
use ananse_wasi::WasiSnapshotPreview1;
use p3_field::PrimeCharacteristicRing;

fn is_arithmetic(opcode: OpCode) -> bool {
    matches!(
        opcode,
        OpCode::I32Add | OpCode::I32Sub | OpCode::I64Add | OpCode::I64Sub
    )
}

fn assert_binary_relation(
    fixture: &str,
    export: &str,
    args: &[Word],
    opcode: OpCode,
    tamper_hi: bool,
) {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom_entry(
        &wat_from_fixture(fixture),
        &Entry::Export(export.into()),
        args,
    );
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(
        failing_rows(&air, &main, &perm, challenges).is_empty(),
        "{export}{args:?}: honest trace violates the AIR"
    );

    let row = trace
        .records()
        .iter()
        .position(|r| r.opcode == opcode)
        .expect("opcode present");
    // A binary operator writes its result on value-bus slot 2 of its own row; nudge the
    // chosen limb away from its correct value.
    let column = bus_slot(2) + if tamper_hi { slot::HI } else { slot::LO };
    let mut columns = trace.columns().to_vec();
    columns[column][row] += Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "{export}{args:?} (tamper_hi={tamper_hi}): binary value relation missed the corruption, got {failing:?}"
    );
}

fn assert_move_relation(
    fixture: &str,
    export: &str,
    args: &[Word],
    opcode: OpCode,
    tamper_hi: bool,
) {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom_entry(
        &wat_from_fixture(fixture),
        &Entry::Export(export.into()),
        args,
    );
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(
        failing_rows(&air, &main, &perm, challenges).is_empty(),
        "{export}{args:?}: honest trace violates the AIR"
    );

    let row = trace
        .records()
        .iter()
        .position(|r| r.opcode == opcode)
        .expect("opcode present");
    // A data-movement operator writes its result on value-bus slot 1 of its own row;
    // nudge the chosen limb away from the value it must copy or clear.
    let column = bus_slot(1) + if tamper_hi { slot::HI } else { slot::LO };
    let mut columns = trace.columns().to_vec();
    columns[column][row] += Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "{export}{args:?} (tamper_hi={tamper_hi}): movement family missed the corruption, got {failing:?}"
    );
}

fn assert_select_relation(export: &str, args: &[Word], tamper_hi: bool) {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom_entry(
        &wat_from_fixture("select.wat"),
        &Entry::Export(export.into()),
        args,
    );
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(
        failing_rows(&air, &main, &perm, challenges).is_empty(),
        "{export}{args:?}: honest select trace violates the AIR"
    );

    let row = trace
        .records()
        .iter()
        .position(|r| r.opcode == OpCode::Select)
        .expect("select present");
    // `select` writes its chosen operand on value-bus slot 3 of its own row; nudging the
    // picked limb makes the result disagree with the muxed operand.
    let column = bus_slot(3) + if tamper_hi { slot::HI } else { slot::LO };
    let mut columns = trace.columns().to_vec();
    columns[column][row] += Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "{export}{args:?} (tamper_hi={tamper_hi}): select mux missed the corruption, got {failing:?}"
    );
}

fn non_arithmetic_bus_read(trace: &Trace) -> Option<(usize, usize)> {
    let columns = trace.columns();
    (0..trace.steps())
        .filter(|&r| !is_arithmetic(trace.records()[r].opcode))
        .find_map(|r| {
            (0..BUS_SLOTS).find_map(|s| {
                let base = bus_slot(s);
                let active = columns[base + slot::ACTIVE][r] == Felt::ONE;
                let is_read = columns[base + slot::IS_WRITE][r] == Felt::ZERO;
                (active && is_read).then_some((r, base))
            })
        })
}

fn active_sorted_entry(trace: &Trace) -> Option<(usize, usize)> {
    let columns = trace.columns();
    (0..trace.length()).find_map(|r| {
        (0..BUS_SLOTS).find_map(|s| {
            let base = sorted_slot(s);
            (columns[base + sorted::ACTIVE][r] == Felt::ONE).then_some((r, base))
        })
    })
}

fn first_write_row(trace: &Trace) -> Option<usize> {
    let columns = trace.columns();
    (0..trace.steps()).find(|&r| {
        (0..BUS_SLOTS).any(|s| {
            let base = bus_slot(s);
            columns[base + slot::ACTIVE][r] == Felt::ONE
                && columns[base + slot::IS_WRITE][r] == Felt::ONE
        })
    })
}

#[test]
fn every_single_frame_trace_satisfies_the_constraints() {
    let challenges = mock_challenges();
    for name in SINGLE_FRAME_FIXTURES {
        let (trace, rom, data) = trace_and_rom(&wat_from_fixture(name));
        let air = air_for(&trace, &rom, &data);
        let main = main_matrix(trace.columns(), trace.length());
        let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
        let failing = failing_rows(&air, &main, &perm, challenges);
        assert!(failing.is_empty(), "{name}: {failing:?}");
    }
}

#[test]
fn adding_a_second_hot_selector_breaks_one_hotness() {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom(&wat_from_fixture("func_add.wat"));
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    // Forcing a second hot selector on row 0---the padding selector, otherwise zero on
    // a real step---makes the row's selectors sum to two and breaks one-hotness.
    let mut columns = trace.columns().to_vec();
    columns[SELECTOR_BASE + SEL_PADDING][0] = Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&0),
        "expected a row-0 violation, got {failing:?}"
    );
}

#[test]
fn stalling_the_clock_breaks_the_increment() {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom(&wat_from_fixture("func_add.wat"));
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    // The clock advances by one every row; stalling it on row 1 makes row 0's
    // increment constraint fail, so timestamps cannot be reused across accesses.
    let mut columns = trace.columns().to_vec();
    columns[COL_CLK][1] = columns[COL_CLK][0];
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&0),
        "expected a row-0 clock violation, got {failing:?}"
    );
}

#[test]
fn clearing_padding_in_the_halt_suffix_breaks_absorption() {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom(&wat_from_fixture("func_add.wat"));
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    // Padding is absorbing: clearing the padding selector on a row inside the halt
    // suffix makes the preceding still-padding row's absorbing constraint fail, so a
    // prover cannot revive a real step in the trace tail.
    let pad = trace.steps();
    assert!(pad + 1 < trace.length(), "fixture needs a padding pair");
    let mut columns = trace.columns().to_vec();
    columns[SELECTOR_BASE + SEL_PADDING][pad + 1] = Felt::ZERO;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&pad),
        "expected an absorbing violation at row {pad}, got {failing:?}"
    );
}

#[test]
fn forging_an_opcode_breaks_the_control_flow_lookup() {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom(&wat_from_fixture("func_add.wat"));
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    // Move row 0's hot selector to a different opcode: one-hotness still holds, but the
    // edge recomputed from the main trace changes, so the honest grand sum no longer
    // telescopes and the lookup recurrence breaks on that row.
    let mut columns = trace.columns().to_vec();
    let hot = (0..NUM_SELECTORS)
        .find(|&j| columns[SELECTOR_BASE + j][0] == Felt::ONE)
        .expect("row 0 is one-hot");
    columns[SELECTOR_BASE + hot][0] = Felt::ZERO;
    let forged_selector = if hot == 0 { 1 } else { 0 };
    columns[SELECTOR_BASE + forged_selector][0] = Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&0),
        "expected a row-0 lookup violation, got {failing:?}"
    );
}

#[test]
fn forging_a_height_breaks_the_control_flow_lookup() {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom(&wat_from_fixture("func_add.wat"));
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    // The height rides the ROM edge. Nudging it on a non-first, non-arithmetic step
    // packs an edge absent from the table without tripping the first-row height
    // boundary or the address family, isolating the break to the lookup recurrence.
    let row = (1..trace.steps())
        .find(|&r| !is_arithmetic(trace.records()[r].opcode))
        .expect("a non-arithmetic step past the first");
    let mut columns = trace.columns().to_vec();
    columns[COL_HEIGHT][row] += Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "expected a row-{row} lookup violation, got {failing:?}"
    );
}

#[test]
fn forging_a_bus_access_value_breaks_the_permutation() {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom(&wat_from_fixture("func_add.wat"));
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    let (row, base) = non_arithmetic_bus_read(&trace).expect("a non-arithmetic bus read");
    let mut columns = trace.columns().to_vec();
    columns[base + slot::LO][row] += Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "expected a row-{row} permutation violation, got {failing:?}"
    );
}

#[test]
fn dropping_a_bus_access_breaks_the_permutation() {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom(&wat_from_fixture("func_add.wat"));
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    let (row, base) = non_arithmetic_bus_read(&trace).expect("a non-arithmetic bus read");
    let mut columns = trace.columns().to_vec();
    columns[base + slot::ACTIVE][row] = Felt::ZERO;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "expected a row-{row} permutation violation, got {failing:?}"
    );
}

#[test]
fn forging_a_sorted_timestamp_breaks_the_permutation() {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom(&wat_from_fixture("func_add.wat"));
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    let (row, base) = active_sorted_entry(&trace).expect("an active sorted entry");
    let mut columns = trace.columns().to_vec();
    columns[base + sorted::TS][row] += Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "expected a row-{row} permutation violation, got {failing:?}"
    );
}

#[test]
fn an_access_on_the_terminal_padding_row_is_rejected() {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom(&wat_from_fixture("func_add.wat"));
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    let last = trace.length() - 1;
    let mut columns = trace.columns().to_vec();
    columns[bus_slot(0) + slot::ACTIVE][last] = Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&last),
        "expected a row-{last} padding-gate violation, got {failing:?}"
    );
}

#[test]
fn corrupting_a_written_value_byte_fails_the_range_check() {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom(&wat_from_fixture("func_add.wat"));
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    let row = first_write_row(&trace).expect("a value-bus write");
    let mut columns = trace.columns().to_vec();
    columns[RC_WRITE_LO][row] += Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "expected a row-{row} range violation, got {failing:?}"
    );
}

#[test]
fn corrupting_an_ordering_gap_byte_fails_the_range_check() {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom(&wat_from_fixture("func_add.wat"));
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    let mut columns = trace.columns().to_vec();
    columns[rc_gap(0)][0] += Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&0),
        "expected a row-0 range violation, got {failing:?}"
    );
}

#[test]
fn a_byte_outside_the_table_fails_the_lookup() {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom(&wat_from_fixture("func_add.wat"));
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    let row = first_write_row(&trace).expect("a value-bus write");
    let mut columns = trace.columns().to_vec();
    columns[RC_WRITE_LO][row] = Felt::new(256);
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "expected a row-{row} lookup violation, got {failing:?}"
    );
}

#[test]
fn a_forged_initial_value_fails_the_boundary_lookup() {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom_entry(
        &wat_from_fixture("func_add.wat"),
        &Entry::Export("add".into()),
        &[Word::I32(7), Word::I32(5)],
    );
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    let mut forged_initial = trace.initial_state().to_vec();
    forged_initial[0].1 = Felt::ZERO;
    let forged_air = AnanseAir::new(
        &rom,
        &data,
        trace.length(),
        trace.stack_base(),
        &forged_initial,
    );
    let failing = failing_rows(&forged_air, &main, &perm, challenges);
    assert!(
        failing.contains(&0),
        "expected a row-0 boundary violation, got {failing:?}"
    );
}

#[test]
fn i32_add_relation_holds_and_catches_a_wrong_result() {
    for tamper_hi in [false, true] {
        assert_binary_relation(
            "func_add.wat",
            "add",
            &[Word::I32(7), Word::I32(5)],
            OpCode::I32Add,
            tamper_hi,
        );
    }
    // The carry path: the low limbs sum past 2^32 and the result wraps to zero.
    assert_binary_relation(
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
        assert_binary_relation(
            "func_sub.wat",
            "sub",
            &[Word::I32(7), Word::I32(5)],
            OpCode::I32Sub,
            tamper_hi,
        );
    }
    // The borrow path: the subtraction underflows and wraps to `2^32 - 2`.
    assert_binary_relation(
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
        assert_binary_relation(
            "func_add_i64.wat",
            "add",
            &[Word::I64(0xFFFF_FFFF), Word::I64(1)],
            OpCode::I64Add,
            tamper_hi,
        );
    }
    // The high limbs overflow past 2^64 and the carry out is dropped.
    assert_binary_relation(
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
        assert_binary_relation(
            "func_sub_i64.wat",
            "sub",
            &[Word::I64(0x1_0000_0000), Word::I64(1)],
            OpCode::I64Sub,
            tamper_hi,
        );
    }
}

#[test]
fn copy_families_move_their_operand_and_catch_a_wrong_result() {
    // `local.get` / `local.set` carry the value 42 between local 0 and the stack.
    for opcode in [OpCode::LocalGet, OpCode::LocalSet] {
        for tamper_hi in [false, true] {
            assert_move_relation("local_set.wat", "local_set", &[], opcode, tamper_hi);
        }
    }
    // `local.tee` copies its operand into a local while leaving it on the stack.
    for tamper_hi in [false, true] {
        assert_move_relation(
            "local_tee.wat",
            "tee",
            &[Word::I32(7)],
            OpCode::LocalTee,
            tamper_hi,
        );
    }
    // `global.set` / `global.get` round-trip a value through a mutable global.
    for opcode in [OpCode::GlobalSet, OpCode::GlobalGet] {
        for tamper_hi in [false, true] {
            assert_move_relation("global.wat", "g_rw", &[Word::I32(9)], opcode, tamper_hi);
        }
    }
}

#[test]
fn narrowing_conversions_keep_the_low_limb_and_clear_the_high() {
    // `wrap` truncates the i64 to its low 32 bits; unsigned `extend` widens it back
    // with a zero high limb. Corrupting either limb of the result breaks the relation.
    let arg = &[Word::I64(0x1234_5678_9ABC_DEF0)];
    for opcode in [OpCode::I32WrapI64, OpCode::I64ExtendI32U] {
        for tamper_hi in [false, true] {
            assert_move_relation("conv.wat", "conv", arg, opcode, tamper_hi);
        }
    }
}

#[test]
fn unsigned_comparisons_hold_and_catch_a_wrong_result() {
    let cases: &[(&str, OpCode, [Word; 2])] = &[
        ("lt_u", OpCode::I64LtU, [Word::I64(5), Word::I64(7)]),
        ("gt_u", OpCode::I64GtU, [Word::I64(7), Word::I64(5)]),
        // Equal operands drive the is-zero gadget.
        ("le_u", OpCode::I64LeU, [Word::I64(5), Word::I64(5)]),
        ("eq", OpCode::I64Eq, [Word::I64(5), Word::I64(5)]),
        ("ne", OpCode::I64Ne, [Word::I64(5), Word::I64(7)]),
        // A high-limb difference exercises the borrow out of the low limb's compare.
        (
            "lt_u",
            OpCode::I64LtU,
            [Word::I64(0x1_0000_0000), Word::I64(0x3_0000_0000)],
        ),
    ];
    for &(export, opcode, args) in cases {
        for tamper_hi in [false, true] {
            assert_binary_relation("cmp.wat", export, &args, opcode, tamper_hi);
        }
    }
}

#[test]
fn signed_comparisons_use_the_sign_bits() {
    // On sign-crossing operands the signed and unsigned orderings disagree, so an
    // honest trace satisfies the relation only when the sign witnesses are correct.
    let neg_one = Word::I64(-1i64 as u64);
    let neg_two = Word::I64(-2i64 as u64);
    let cases: &[(&str, OpCode, [Word; 2])] = &[
        ("lt_s", OpCode::I64LtS, [neg_one, Word::I64(0)]),
        ("lt_s", OpCode::I64LtS, [Word::I64(0), neg_one]),
        ("ge_s", OpCode::I64GeS, [neg_one, neg_two]),
    ];
    for &(export, opcode, args) in cases {
        for tamper_hi in [false, true] {
            assert_binary_relation("cmp.wat", export, &args, opcode, tamper_hi);
        }
    }
    // The i32 signed path pulls its sign from the low limb instead of the high.
    for args in [
        [Word::I32(-1i32 as u32), Word::I32(0)],
        [Word::I32(3), Word::I32(5)],
    ] {
        for tamper_hi in [false, true] {
            assert_binary_relation("func_lts.wat", "lts", &args, OpCode::I32LtS, tamper_hi);
        }
    }
}

#[test]
fn eqz_detects_zero_and_catches_a_wrong_result() {
    // Zero, a low-limb value, and a value living only in the high limb.
    for arg in [Word::I64(0), Word::I64(5), Word::I64(0x1_0000_0000)] {
        for tamper_hi in [false, true] {
            assert_move_relation("cmp.wat", "eqz", &[arg], OpCode::I64Eqz, tamper_hi);
        }
    }
}

#[test]
fn signed_extension_fills_the_high_limb_from_the_sign() {
    // A negative i32 widens with an all-ones high limb; a non-negative one clears it.
    for arg in [Word::I32(-1i32 as u32), Word::I32(1)] {
        for tamper_hi in [false, true] {
            assert_move_relation(
                "ext_s.wat",
                "ext_s",
                &[arg],
                OpCode::I64ExtendI32S,
                tamper_hi,
            );
        }
    }
}

#[test]
fn bitwise_ops_hold_and_catch_a_wrong_result() {
    // Operands with bits set across both limbs, so and/or/xor differ nibble by nibble.
    let a = Word::I64(0xF0F0_F0F0_0F0F_0F0F);
    let b = Word::I64(0xFF00_FF00_00FF_00FF);
    let cases: &[(&str, OpCode)] = &[
        ("and", OpCode::I64And),
        ("or", OpCode::I64Or),
        ("xor", OpCode::I64Xor),
    ];
    for &(export, opcode) in cases {
        for tamper_hi in [false, true] {
            assert_binary_relation("bitwise.wat", export, &[a, b], opcode, tamper_hi);
        }
    }
    // The i32 path leaves the high nibbles---and the result's high limb---zero.
    for tamper_hi in [false, true] {
        assert_binary_relation(
            "bitwise.wat",
            "and32",
            &[Word::I32(0xF0F0_0F0F), Word::I32(0xFF00_00FF)],
            OpCode::I32And,
            tamper_hi,
        );
    }
}

#[test]
fn popcount_sums_the_set_bits_and_catches_a_wrong_result() {
    // A value with bits set across both limbs, an all-ones word (count = 64), and the
    // i32 path whose high bytes stay zero.
    for arg in [
        Word::I64(0xF0F0_0F0F_1234_5678),
        Word::I64(u64::MAX),
        Word::I64(0),
    ] {
        for tamper_hi in [false, true] {
            assert_move_relation(
                "bitwise.wat",
                "popcnt",
                &[arg],
                OpCode::I64Popcnt,
                tamper_hi,
            );
        }
    }
    for tamper_hi in [false, true] {
        assert_move_relation(
            "bitwise.wat",
            "popcnt32",
            &[Word::I32(0xF0F0_0F0F)],
            OpCode::I32Popcnt,
            tamper_hi,
        );
    }
}

#[test]
fn select_muxes_on_the_condition_and_catches_a_wrong_result() {
    // A non-zero condition keeps the first operand; a zero condition takes the second.
    // The `i64` cases place the operands' distinguishing bits in the high limb so a
    // wrong choice shows in either limb.
    let cases: &[(&str, &[Word])] = &[
        ("select32", &[Word::I32(7), Word::I32(9), Word::I32(1)]),
        ("select32", &[Word::I32(7), Word::I32(9), Word::I32(0)]),
        (
            "select64",
            &[
                Word::I64(0x1_0000_0007),
                Word::I64(0x2_0000_0009),
                Word::I32(1),
            ],
        ),
        (
            "select64",
            &[
                Word::I64(0x1_0000_0007),
                Word::I64(0x2_0000_0009),
                Word::I32(0),
            ],
        ),
    ];
    for &(export, args) in cases {
        for tamper_hi in [false, true] {
            assert_select_relation(export, args, tamper_hi);
        }
    }
}

#[test]
fn branch_direction_follows_the_condition() {
    // `if`: a true condition enters the then-arm, a false one the else-arm. `br_if`: a
    // true condition branches, a false one falls through. All four cases satisfy the
    // AIR only if the executed successor is the one the condition selects, so a wrong
    // mux polarity or target column would break one of them.
    let cases: &[(&str, i32)] = &[
        ("if_pick", 1),
        ("if_pick", 0),
        ("brif_pick", 1),
        ("brif_pick", 0),
    ];
    let challenges = mock_challenges();
    for &(export, arg) in cases {
        let (trace, rom, data) = trace_and_rom_entry(
            &wat_from_fixture("branch.wat"),
            &Entry::Export(export.into()),
            &[Word::I32(arg as u32)],
        );
        let air = air_for(&trace, &rom, &data);
        let main = main_matrix(trace.columns(), trace.length());
        let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
        assert!(
            failing_rows(&air, &main, &perm, challenges).is_empty(),
            "{export}({arg}): honest branch trace violates the AIR"
        );
    }
}

#[test]
fn redirecting_a_branch_to_its_sibling_edge_is_rejected() {
    // A true condition takes the then-arm. Redirecting the branch to the not-taken
    // (else) target it carries---the sibling ROM edge the control-flow lookup would
    // accept on its own---leaves the branch row disagreeing with its condition, so the
    // machine rejects it.
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom_entry(
        &wat_from_fixture("branch.wat"),
        &Entry::Export("if_pick".into()),
        &[Word::I32(1)],
    );
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    let row = trace
        .records()
        .iter()
        .position(|r| r.opcode == OpCode::If)
        .expect("if present");
    let not_taken = trace.column_at(PC_DATA_B).expect("data column")[row];
    let mut columns = trace.columns().to_vec();
    columns[COL_PC][row + 1] = not_taken;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "expected the branch row {row} rejected, got {failing:?}"
    );
}

#[test]
fn const_binds_its_pushed_value_to_the_module_immediate() {
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom_entry(
        &wat_from_fixture("const.wat"),
        &Entry::Export("c64".into()),
        &[],
    );
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    let row = trace
        .records()
        .iter()
        .position(|r| r.opcode == OpCode::I64Const)
        .expect("const present");

    // Nudging only the pushed limb breaks the value relation tying the write to the
    // carried data-ROM limb.
    let mut columns = trace.columns().to_vec();
    columns[bus_slot(0) + slot::LO][row] += Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    assert!(
        failing_rows(&air, &forged, &perm, challenges).contains(&row),
        "a forged push should break the value relation"
    );

    // Sliding the pushed limb and its carried data-ROM limb together keeps the value
    // relation satisfied, so only the data-ROM lookup---which pins the limb to the
    // module immediate---rejects the forged constant.
    let mut columns = trace.columns().to_vec();
    columns[bus_slot(0) + slot::LO][row] += Felt::ONE;
    columns[PC_DATA_A][row] += Felt::ONE;
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "expected a row-{row} data-ROM violation, got {failing:?}"
    );
}

#[test]
fn a_forged_and_nibble_fails_the_bitwise_lookup() {
    // Corrupt one AND nibble to a wrong in-range value and slide the result's low limb
    // to match it, so the decomposition relation still holds and only the AND-table
    // lookup---which pins the nibble to a genuine `a & b`---rejects the triple.
    let challenges = mock_challenges();
    let (trace, rom, data) = trace_and_rom_entry(
        &wat_from_fixture("bitwise.wat"),
        &Entry::Export("and".into()),
        &[Word::I64(0xFF), Word::I64(0xFF)],
    );
    let air = air_for(&trace, &rom, &data);
    let main = main_matrix(trace.columns(), trace.length());
    let perm = permutation_of(&main, &rom, &data, trace.initial_state(), challenges);
    assert!(failing_rows(&air, &main, &perm, challenges).is_empty());

    let row = trace
        .records()
        .iter()
        .position(|r| r.opcode == OpCode::I64And)
        .expect("and present");
    // Nibble 0 of `0xFF & 0xFF` is `0xF & 0xF = 0xF`; forging it to `0x0` leaves the
    // AND-table (the pair `(0xF, 0xF)` maps only to `0xF`).
    let mut columns = trace.columns().to_vec();
    columns[BW_P_BASE][row] = Felt::ZERO;
    columns[bus_slot(2) + slot::LO][row] -= Felt::new(0xF);
    let forged = main_matrix(&columns, trace.length());
    let failing = failing_rows(&air, &forged, &perm, challenges);
    assert!(
        failing.contains(&row),
        "expected a row-{row} bitwise lookup violation, got {failing:?}"
    );
}

#[test]
fn program_rom_contains_every_executed_edge() {
    let mut checked = 0usize;
    for name in SINGLE_FRAME_FIXTURES {
        let module = Module::decode(&wat_from_fixture(name)).expect("decode");
        let program = lift(&module).expect("lift");
        let mut host = WasiSnapshotPreview1::new();
        let mut records = Vec::new();
        execute(&module, &Entry::Auto, &[], &mut host, &mut records).expect("execute");

        let func_index = records.first().expect("fixture executes").func_index;
        let function = program
            .functions
            .iter()
            .find(|f| f.func_index == func_index)
            .expect("executed function was lifted");
        let opcodes = function_opcodes(&module, func_index).expect("opcodes");
        let rom = program_rom(&opcodes, function).expect("rom");

        // The local/global offset the operator touches, resolved the way the ROM and
        // trace resolve it: a local keeps its index, a global sits above the locals.
        let immediate_offset = |pc: usize| -> u32 {
            let schedule = &function.schedules[pc];
            schedule
                .reads
                .iter()
                .chain(&schedule.writes)
                .find_map(|reg| match reg {
                    Register::Local(index) => Some(*index),
                    Register::Global(index) => Some(function.locals_count + index),
                    Register::Stack(_) => None,
                })
                .unwrap_or(0)
        };

        for record in &records {
            if let Transition::Next(next_pc) = record.transition {
                let pc = record.pc as usize;
                let height = function.schedules[pc].height_in;
                let imm = immediate_offset(pc);
                let edge = pack_edge(record.pc, opcode_index(record.opcode), next_pc, height, imm)
                    .expect("executed edge packs");
                assert!(
                    rom.contains(&edge),
                    "{name}: edge pc{} -> {next_pc} absent from ROM",
                    record.pc
                );
                checked += 1;
            }
        }

        // The halt self-loop the padded trace tail rests on is a table member too,
        // packed at the exit sentinel's height and offset zero.
        let halt = u32::try_from(function.schedules.len()).expect("body fits u32");
        let halt_loop = pack_edge(halt, SEL_PADDING, halt, 0, 0).expect("halt edge packs");
        assert!(rom.contains(&halt_loop), "{name}: halt self-loop absent");
    }
    assert!(checked > 0, "no executed edges were checked");
}
