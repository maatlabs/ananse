use ananse_decoder::Module;
use ananse_executor::{Entry, NoHost, OpCode, Transition, Word, execute};
use ananse_lift::lift;
use ananse_tests::{SINGLE_FRAME_FIXTURES, TestHost, trace_of, wat_from_file, wat_from_str};
use ananse_trace::layout::{
    COL_MEM_ADDR, COL_MEM_IS_WRITE, COL_MEM_VAL_HI, COL_MEM_VAL_LO, COL_PC, SELECTOR_BASE,
};
use ananse_trace::selector::{NUM_SELECTORS, SEL_PADDING};
use ananse_trace::{Trace, TraceError};
use maat_field::{Felt, FieldElement};

fn column(trace: &Trace, index: usize) -> &[Felt] {
    trace.column_at(index).expect("column in range")
}

/// Exactly one selector column is one on every row, and the selector block holds
/// only zeros and ones.
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

/// Each block's register columns hold, on its own row, the values the operator
/// read---the load-bearing agreement between the register bank and the schedule.
fn assert_reads_match_register_columns(trace: &Trace, name: &str) {
    for (row, record) in trace.records().iter().enumerate() {
        for read in &record.reads {
            let (lo, hi) = trace
                .register_columns(read.reg)
                .expect("read register in range");
            let (value_lo, value_hi) = read.value.to_limbs();
            assert_eq!(
                (column(trace, lo)[row], column(trace, hi)[row]),
                (value_lo, value_hi),
                "{name}: read {:?} at row {row}",
                read.reg
            );
        }
    }
}

/// The program counter of the next row is the successor the transition selected;
/// a fall-through advances it by one, a branch to its taken target.
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

/// The access log is sorted by `(address, step)`.
fn assert_access_log_sorted(trace: &Trace, name: &str) {
    for pair in trace.access_log().windows(2) {
        assert!(
            (pair[0].address, pair[0].step) <= (pair[1].address, pair[1].step),
            "{name}: access log out of order"
        );
    }
}

#[test]
fn single_frame_fixtures_satisfy_trace_invariants() {
    for name in SINGLE_FRAME_FIXTURES {
        let trace = trace_of(&wat_from_file(name));
        assert_selectors_one_hot(&trace, name);
        assert_reads_match_register_columns(&trace, name);
        assert_pc_follows_transitions(&trace, name);
        assert_access_log_sorted(&trace, name);
        assert!(trace.length().is_power_of_two(), "{name}: length not 2^k");
        assert!(
            trace.length() >= trace.steps().max(8),
            "{name}: length below floor"
        );
        assert_eq!(
            trace.width(),
            109 + 2 * trace.register_width(),
            "{name}: width"
        );
    }
}

#[test]
fn recursive_and_cross_function_calls_are_unsupported() {
    // `fib(5)` recurses past its base case; `call_doubler` calls a defined
    // helper unconditionally. Both cross a defined-function call frame.
    let cases: &[(&str, Entry, &[Word])] = &[
        (
            "fibonacci.wat",
            Entry::Export("fib".into()),
            &[Word::I32(5)],
        ),
        ("func_call.wat", Entry::Auto, &[]),
    ];
    for (name, entry, args) in cases {
        let module = Module::decode(&wat_from_file(name)).expect("decode");
        let program = lift(&module).expect("lift");
        let mut host = TestHost::default();
        let mut records = Vec::new();
        execute(&module, entry, args, &mut host, &mut records).expect("execute");
        assert!(
            matches!(
                Trace::build(&program, records),
                Err(TraceError::UnsupportedCall)
            ),
            "{name}: expected UnsupportedCall"
        );
    }
}

#[test]
fn entryless_module_produces_no_trace() {
    let module = Module::decode(&wat_from_file("memory.wat")).expect("decode");
    let program = lift(&module).expect("lift");
    let mut records = Vec::new();
    execute(&module, &Entry::Auto, &[], &mut NoHost, &mut records).expect("execute");
    assert!(records.is_empty());
    assert!(matches!(
        Trace::build(&program, records),
        Err(TraceError::EmptyExecution)
    ));
}

#[test]
fn store_then_load_round_trips_through_the_access_log() {
    let trace = trace_of(&wat_from_str(
        "(module (memory 1) (func (result i32)
            (i32.store (i32.const 8) (i32.const 123))
            (i32.load (i32.const 8))))",
    ));

    let log = trace.access_log();
    assert_eq!(log.len(), 2);
    assert_eq!(
        (log[0].address, log[0].value, log[0].is_write),
        (8, 123, true)
    );
    assert_eq!(
        (log[1].address, log[1].value, log[1].is_write),
        (8, 123, false)
    );
    assert!(log[0].step < log[1].step);

    let store_row = trace
        .records()
        .iter()
        .position(|r| r.opcode == OpCode::I32Store)
        .expect("store present");
    let load_row = trace
        .records()
        .iter()
        .position(|r| r.opcode == OpCode::I32Load)
        .expect("load present");
    assert_eq!(column(&trace, COL_MEM_IS_WRITE)[store_row], Felt::ONE);
    assert_eq!(column(&trace, COL_MEM_ADDR)[store_row], Felt::new(8));
    assert_eq!(column(&trace, COL_MEM_VAL_LO)[store_row], Felt::new(123));
    assert_eq!(column(&trace, COL_MEM_VAL_HI)[store_row], Felt::ZERO);
    assert_eq!(column(&trace, COL_MEM_IS_WRITE)[load_row], Felt::ZERO);
    assert_eq!(column(&trace, COL_MEM_VAL_LO)[load_row], Felt::new(123));
    assert_eq!(column(&trace, COL_MEM_VAL_HI)[load_row], Felt::ZERO);
}

#[test]
fn padding_holds_the_halt_state_to_a_power_of_two() {
    let trace = trace_of(&wat_from_file("i32_const.wat"));
    assert!(trace.length().is_power_of_two());
    assert!(
        trace.steps() < trace.length(),
        "fixture should need padding"
    );

    let held_pc = column(&trace, COL_PC)[trace.steps() - 1];
    let first_pad = trace.steps();
    for row in first_pad..trace.length() {
        assert_eq!(column(&trace, SELECTOR_BASE + SEL_PADDING)[row], Felt::ONE);
        assert_eq!(column(&trace, COL_PC)[row], held_pc);
        assert_eq!(column(&trace, COL_MEM_ADDR)[row], Felt::ZERO);
        assert_eq!(column(&trace, COL_MEM_VAL_LO)[row], Felt::ZERO);
        assert_eq!(column(&trace, COL_MEM_VAL_HI)[row], Felt::ZERO);
        assert_eq!(column(&trace, COL_MEM_IS_WRITE)[row], Felt::ZERO);
    }
    // Every padding row is identical: the state is held, not evolving.
    for col in 0..trace.width() {
        for row in (first_pad + 1)..trace.length() {
            assert_eq!(column(&trace, col)[row], column(&trace, col)[first_pad]);
        }
    }
}

#[test]
fn add_register_bank_carries_operands_and_result() {
    let module = Module::decode(&wat_from_file("func_add.wat")).expect("decode");
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
    let trace = Trace::build(&program, records).expect("build");

    let add_row = trace
        .records()
        .iter()
        .position(|r| r.opcode == OpCode::I32Add)
        .expect("add present");

    // The operands 7 and 5 sit in the add's read columns on its own row, in the
    // low limb, with a zero high limb (they are `i32`).
    let record = trace.records()[add_row].clone();
    let operands = record
        .reads
        .iter()
        .map(|read| {
            let (lo, hi) = trace.register_columns(read.reg).unwrap();
            assert_eq!(column(&trace, hi)[add_row], Felt::ZERO, "i32 high limb");
            column(&trace, lo)[add_row]
        })
        .collect::<Vec<_>>();
    assert_eq!(operands, vec![Felt::new(5), Felt::new(7)]);

    // The sum 12 lands in the add's write column on the next row.
    let write = record.writes.first().copied().expect("add writes a result");
    assert_eq!(write.value, Word::I32(12));
    let (lo, _hi) = trace.register_columns(write.reg).unwrap();
    assert_eq!(column(&trace, lo)[add_row + 1], Felt::new(12));
}

#[test]
fn i64_values_above_the_prime_survive_as_faithful_limbs() {
    // `0xFFFF_FFFF_FFFF_FFFF` exceeds the Goldilocks prime, so a single-residue
    // encoding would alias it to `0xFFFF_FFFE` and drop the high half. The
    // two-limb bank must carry `(lo, hi) = (0xFFFF_FFFF, 0xFFFF_FFFF)`, which
    // reconstructs the true value `lo + hi * 2^32`.
    let trace = trace_of(&wat_from_str(
        "(module (func (result i64)
            (i64.add (i64.const -1) (i64.const 0))))",
    ));

    let add_row = trace
        .records()
        .iter()
        .position(|r| r.opcode == OpCode::I64Add)
        .expect("add present");
    let operand = trace.records()[add_row]
        .reads
        .iter()
        .find(|read| read.value == Word::I64(u64::MAX))
        .expect("the -1 operand is read by the add");

    let (lo, hi) = trace
        .register_columns(operand.reg)
        .expect("register in range");
    assert_eq!(column(&trace, lo)[add_row], Felt::new(0xFFFF_FFFF));
    assert_eq!(column(&trace, hi)[add_row], Felt::new(0xFFFF_FFFF));
    // The high limb is non-zero: the lossy residue `(0xFFFF_FFFE, 0)` is refused.
    assert_ne!(column(&trace, hi)[add_row], Felt::ZERO);
}
