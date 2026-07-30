use ananse_decoder::{Felt, Module, OpCode, Word};
use ananse_executor::{Entry, NoHost, Transition, execute};
use ananse_lift::lift;
use ananse_tests::{
    SINGLE_FRAME_FIXTURES, trace_of, wat_from_example, wat_from_fixture, wat_from_str,
};
use ananse_trace::layout::{
    self, BUS_SLOTS, COL_CLK, COL_PC, SELECTOR_BASE, bus_slot, slot, sorted, sorted_slot,
};
use ananse_trace::selector::{NUM_SELECTORS, SEL_PADDING};
use ananse_trace::{Trace, TraceError};
use ananse_wasi::WasiSnapshotPreview1;
use p3_field::PrimeCharacteristicRing;

fn column(trace: &Trace, index: usize) -> &[Felt] {
    trace.column_at(index).expect("column in range")
}

fn assert_selectors_one_hot(trace: &Trace, name: &str) {
    for row in 0..trace.length() {
        let hot = (0..NUM_SELECTORS)
            .filter(|&sel| {
                let value = column(trace, SELECTOR_BASE + sel)[row];
                assert!(
                    value == Felt::ZERO || value == Felt::ONE,
                    "{name}: selector {sel} row {row} is neither 0 nor 1"
                );
                value == Felt::ONE
            })
            .count();
        assert_eq!(hot, 1, "{name}: row {row} is not one-hot");
    }
}

fn assert_reads_land_on_the_bus(trace: &Trace, name: &str) {
    for (row, record) in trace.records().iter().enumerate() {
        for (index, read) in record.reads.iter().enumerate() {
            let base = bus_slot(index);
            let (value_lo, value_hi) = read.value.to_limbs();
            assert_eq!(
                column(trace, base + slot::LO)[row],
                value_lo,
                "{name}: read lo"
            );
            assert_eq!(
                column(trace, base + slot::HI)[row],
                value_hi,
                "{name}: read hi"
            );
            assert_eq!(column(trace, base + slot::IS_WRITE)[row], Felt::ZERO);
            assert_eq!(column(trace, base + slot::ACTIVE)[row], Felt::ONE);
        }
    }
}

fn assert_pc_follows_transitions(trace: &Trace, name: &str) {
    let pc = column(trace, COL_PC);
    for (row, record) in trace.records().iter().enumerate() {
        if let Transition::Next(target) = record.transition {
            assert!(
                row + 1 < trace.steps(),
                "{name}: dangling Next at row {row}"
            );
            assert_eq!(
                pc[row + 1],
                Felt::new(u64::from(target)),
                "{name}: pc after row {row}"
            );
        }
    }
}

fn active_sorted(trace: &Trace) -> Vec<(Felt, Felt, Felt, Felt, Felt)> {
    let mut entries = Vec::new();
    'outer: for row in 0..trace.length() {
        for index in 0..BUS_SLOTS {
            let base = sorted_slot(index);
            if column(trace, base + sorted::ACTIVE)[row] != Felt::ONE {
                break 'outer;
            }
            entries.push((
                column(trace, base + sorted::ADDR)[row],
                column(trace, base + sorted::LO)[row],
                column(trace, base + sorted::HI)[row],
                column(trace, base + sorted::IS_WRITE)[row],
                column(trace, base + sorted::SAME_ADDR)[row],
            ));
        }
    }
    entries
}

fn assert_sorted_log_consistent(trace: &Trace, name: &str) {
    let mut previous: Option<Felt> = None;
    for (addr, _, _, _, same_addr) in active_sorted(trace) {
        match previous {
            None => assert_eq!(same_addr, Felt::ZERO, "{name}: first entry opens a run"),
            Some(prev) if same_addr == Felt::ONE => {
                assert_eq!(addr, prev, "{name}: continuing entry changed address")
            }
            Some(prev) => assert_ne!(addr, prev, "{name}: fresh entry repeated address"),
        }
        previous = Some(addr);
    }
}

#[test]
fn single_frame_fixtures_satisfy_trace_invariants() {
    for name in SINGLE_FRAME_FIXTURES {
        let trace = trace_of(&wat_from_fixture(name));
        assert_selectors_one_hot(&trace, name);
        assert_reads_land_on_the_bus(&trace, name);
        assert_pc_follows_transitions(&trace, name);
        assert_sorted_log_consistent(&trace, name);
        assert!(trace.length().is_power_of_two(), "{name}: length not 2^k");
        assert!(
            trace.length() >= trace.steps().max(8),
            "{name}: length below floor"
        );
        // A trailing padding row is guaranteed so the halt state occupies the final
        // row and no real step lands where transition constraints vanish.
        assert!(
            trace.steps() < trace.length(),
            "{name}: no trailing padding row"
        );
        // The column count is fixed by the layout, independent of register-file width.
        assert_eq!(trace.width(), layout::main_width(), "{name}: width");
    }
}

#[test]
fn recursive_and_cross_function_calls_are_unsupported() {
    // `fib(5)` recurses past its base case; `call_doubler` calls a defined helper
    // unconditionally. Both cross a defined-function call frame.
    let cases: [(&str, Vec<u8>, Entry, &[Word]); 2] = [
        (
            "fibonacci.wat",
            wat_from_example("fibonacci.wat"),
            Entry::Export("fib".into()),
            &[Word::I32(5)],
        ),
        (
            "func_call.wat",
            wat_from_fixture("func_call.wat"),
            Entry::Auto,
            &[],
        ),
    ];
    for (name, bytes, entry, args) in &cases {
        let module = Module::decode(bytes).expect("decode");
        let program = lift(&module).expect("lift");
        let mut host = WasiSnapshotPreview1::new();
        let mut records = Vec::new();
        execute(&module, entry, args, &mut host, &mut records).expect("execute");
        assert!(
            matches!(
                Trace::build(&program, records, args, &[]),
                Err(TraceError::UnsupportedCall)
            ),
            "{name}: expected UnsupportedCall"
        );
    }
}

#[test]
fn entryless_module_produces_no_trace() {
    let module = Module::decode(&wat_from_fixture("memory.wat")).expect("decode");
    let program = lift(&module).expect("lift");
    let mut records = Vec::new();
    execute(&module, &Entry::Auto, &[], &mut NoHost, &mut records).expect("execute");
    assert!(records.is_empty());
    assert!(matches!(
        Trace::build(&program, records, &[], &[]),
        Err(TraceError::EmptyExecution)
    ));
}

#[test]
fn store_then_load_round_trip_through_the_unified_log() {
    let trace = trace_of(&wat_from_str(
        "(module (memory 1) (func (result i32)
            (i32.store (i32.const 8) (i32.const 123))
            (i32.load (i32.const 8))))",
    ));

    let at_eight = active_sorted(&trace)
        .into_iter()
        .filter(|&(addr, ..)| addr == Felt::new(8))
        .collect::<Vec<_>>();
    assert_eq!(at_eight.len(), 2, "one store and one load at address 8");
    let (_, lo0, _, is_write0, same0) = at_eight[0];
    let (_, lo1, _, is_write1, same1) = at_eight[1];
    assert_eq!(
        (lo0, is_write0, same0),
        (Felt::new(123), Felt::ONE, Felt::ZERO)
    );
    assert_eq!(
        (lo1, is_write1, same1),
        (Felt::new(123), Felt::ZERO, Felt::ONE)
    );
}

#[test]
fn padding_rests_on_the_exit_sentinel_with_an_idle_bus() {
    let module = Module::decode(&wat_from_fixture("i32_const.wat")).expect("decode");
    let program = lift(&module).expect("lift");
    let mut host = WasiSnapshotPreview1::new();
    let mut records = Vec::new();
    execute(&module, &Entry::Auto, &[], &mut host, &mut records).expect("execute");
    let func_index = records[0].func_index;
    let halt_pc = program
        .functions
        .iter()
        .find(|f| f.func_index == func_index)
        .expect("lifted")
        .schedules
        .len();
    let trace = Trace::build(&program, records, &[], &[]).expect("build");

    assert!(trace.length().is_power_of_two());
    assert!(
        trace.steps() < trace.length(),
        "fixture should need padding"
    );

    let sentinel = Felt::new(halt_pc as u64);
    for row in trace.steps()..trace.length() {
        // Padding sits at the static exit sentinel under the padding selector, with an
        // idle bus, while the clock keeps advancing so timestamps stay unique.
        assert_eq!(column(&trace, SELECTOR_BASE + SEL_PADDING)[row], Felt::ONE);
        assert_eq!(column(&trace, COL_PC)[row], sentinel);
        assert_eq!(column(&trace, COL_CLK)[row], Felt::new(row as u64));
        for index in 0..BUS_SLOTS {
            assert_eq!(
                column(&trace, bus_slot(index) + slot::ACTIVE)[row],
                Felt::ZERO
            );
        }
    }
}

#[test]
fn a_binary_op_carries_operands_and_result_on_its_own_row() {
    let module = Module::decode(&wat_from_fixture("func_add.wat")).expect("decode");
    let program = lift(&module).expect("lift");
    let mut records = Vec::new();
    execute(
        &module,
        &Entry::Export("add".into()),
        &[Word::I32(7), Word::I32(5)],
        &mut NoHost,
        &mut records,
    )
    .expect("execute");
    let trace = Trace::build(&program, records, &[Word::I32(7), Word::I32(5)], &[]).expect("build");

    let add_row = trace
        .records()
        .iter()
        .position(|r| r.opcode == OpCode::I32Add)
        .expect("add present");

    // The two operands are read onto slots 0 (top of stack, 5) and 1 (7), and the sum
    // 12 is written onto slot 2, all on the add's own row.
    assert_eq!(
        column(&trace, bus_slot(0) + slot::LO)[add_row],
        Felt::new(5)
    );
    assert_eq!(
        column(&trace, bus_slot(1) + slot::LO)[add_row],
        Felt::new(7)
    );
    assert_eq!(
        column(&trace, bus_slot(2) + slot::LO)[add_row],
        Felt::new(12)
    );
    assert_eq!(
        column(&trace, bus_slot(2) + slot::IS_WRITE)[add_row],
        Felt::ONE
    );
    assert_eq!(column(&trace, bus_slot(2) + slot::HI)[add_row], Felt::ZERO);
}

#[test]
fn i64_values_above_the_prime_survive_as_faithful_limbs() {
    // `0xFFFF_FFFF_FFFF_FFFF` exceeds the Goldilocks prime, so a single-residue
    // encoding would alias it to `0xFFFF_FFFE` and drop the high half. The two-limb
    // bus must carry `(lo, hi) = (0xFFFF_FFFF, 0xFFFF_FFFF)`, which reconstructs the
    // true value `lo + hi * 2^32`.
    let trace = trace_of(&wat_from_str(
        "(module (func (result i64)
            (i64.add (i64.const -1) (i64.const 0))))",
    ));

    let add_row = trace
        .records()
        .iter()
        .position(|r| r.opcode == OpCode::I64Add)
        .expect("add present");
    // The `-1` operand is read beneath the `0` on top, so it lands on slot 1.
    let record = &trace.records()[add_row];
    let index = record
        .reads
        .iter()
        .position(|read| read.value == Word::I64(u64::MAX))
        .expect("the -1 operand is read by the add");
    let base = bus_slot(index);
    assert_eq!(
        column(&trace, base + slot::LO)[add_row],
        Felt::new(0xFFFF_FFFF)
    );
    assert_eq!(
        column(&trace, base + slot::HI)[add_row],
        Felt::new(0xFFFF_FFFF)
    );
    // The high limb is non-zero: the lossy residue `(0xFFFF_FFFE, 0)` is refused.
    assert_ne!(column(&trace, base + slot::HI)[add_row], Felt::ZERO);
}
