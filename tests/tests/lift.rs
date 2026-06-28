use ananse_decoder::Module;
use ananse_lift::{Reg, Successors, lift};
use ananse_tests::{WAT_FILES, WAT_SNIPPETS, wasm_features, wat_from_file, wat_from_str};
use wasmparser::{Parser, ValidPayload, Validator};

/// Operand-stack height entering each operator, per defined function, computed by
/// wasmparser's own validator. This is the independent static ground truth the
/// lift must reproduce.
fn validator_heights(bytes: &[u8]) -> Vec<Vec<u32>> {
    let mut validator = Validator::new_with_features(wasm_features());
    let mut per_func = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        let payload = payload.expect("fixture parses");
        if let ValidPayload::Func(func, body) =
            validator.payload(&payload).expect("fixture validates")
        {
            let mut fv = func.into_validator(Default::default());
            let mut locals = body.get_binary_reader();
            fv.read_locals(&mut locals).expect("locals validate");
            let mut heights = Vec::new();
            let mut ops = body.get_operators_reader().expect("operators reader");
            while !ops.eof() {
                let offset = ops.original_position();
                heights.push(fv.operand_stack_height());
                let op = ops.read().expect("operator parses");
                fv.op(offset, &op).expect("operator validates");
            }
            per_func.push(heights);
        }
    }
    per_func
}

fn assert_heights_match(name: &str, bytes: &[u8]) {
    let module = Module::decode(bytes).unwrap_or_else(|e| panic!("{name} decodes: {e}"));
    let program = lift(&module).unwrap_or_else(|e| panic!("{name} lifts: {e}"));
    let oracle = validator_heights(bytes);
    assert_eq!(
        program.functions.len(),
        oracle.len(),
        "{name}: lifted function count vs validator",
    );
    for (f, (func, heights)) in program.functions.iter().zip(&oracle).enumerate() {
        assert_eq!(
            func.instrs.len(),
            heights.len(),
            "{name} fn{f}: instruction count vs validator",
        );
        for (i, instr) in func.instrs.iter().enumerate() {
            assert_eq!(instr.pc as usize, i, "{name} fn{f}: pc equals body index");
            assert_eq!(
                instr.height_in, heights[i],
                "{name} fn{f} pc{i}: lift height {} != validator height {}",
                instr.height_in, heights[i],
            );
        }
    }
}

#[test]
fn lift_heights_match_validator_on_fixtures() {
    for name in WAT_FILES {
        assert_heights_match(name, &wat_from_file(name));
    }
}

#[test]
fn lift_heights_match_validator_on_control_flow() {
    for (name, wat) in WAT_SNIPPETS {
        assert_heights_match(name, &wat_from_str(wat));
    }
}

#[test]
fn func_add_register_schedule() {
    let module = Module::decode(&wat_from_file("func_add.wat")).expect("decodes");
    let program = lift(&module).expect("lifts");
    assert_eq!(program.functions.len(), 1);
    let f = &program.functions[0];
    assert_eq!(f.locals_count, 2, "two params, no declared locals");
    assert_eq!(f.globals_count, 0);
    assert_eq!(f.max_stack_height, 2);
    assert_eq!(f.reg_file_width, 4, "2 locals + 0 globals + 2 stack");

    // local.get 0, local.get 1, i32.add, end
    assert_eq!(f.instrs.len(), 4);
    assert_eq!(f.instrs[0].reads, [Reg::Local(0)]);
    assert_eq!(f.instrs[0].writes, [Reg::Stack(0)]);
    assert_eq!(f.instrs[1].reads, [Reg::Local(1)]);
    assert_eq!(f.instrs[1].writes, [Reg::Stack(1)]);
    // operands are popped top-first: the second-pushed value leads.
    assert_eq!(f.instrs[2].reads, [Reg::Stack(1), Reg::Stack(0)]);
    assert_eq!(f.instrs[2].writes, [Reg::Stack(0)]);
    assert_eq!(f.instrs[2].successors, Successors::Fallthrough);
    assert_eq!(f.instrs[3].successors, Successors::Return);
}

#[test]
fn local_set_reads_stack_writes_local() {
    let module = Module::decode(&wat_from_file("local_set.wat")).expect("decodes");
    let program = lift(&module).expect("lifts");
    let f = &program.functions[0];
    assert_eq!(f.locals_count, 1, "no params, one declared local");
    // i32.const 42, local.set 0, local.get 0, end
    assert_eq!(f.instrs[1].reads, [Reg::Stack(0)]);
    assert_eq!(f.instrs[1].writes, [Reg::Local(0)]);
    assert_eq!(f.instrs[2].reads, [Reg::Local(0)]);
    assert_eq!(f.instrs[2].writes, [Reg::Stack(0)]);
}

#[test]
fn imported_function_shifts_defined_index() {
    // fd_write is imported (function index 0), so the defined function is index 1.
    let module = Module::decode(&wat_from_file("hello_world.wat")).expect("decodes");
    let program = lift(&module).expect("lifts");
    assert_eq!(program.functions.len(), 1);
    assert_eq!(program.functions[0].func_index, 1);
    assert_eq!(
        program.functions[0].locals_count, 1,
        "one declared local $iovs"
    );
}

#[test]
fn call_resolves_callee_arity() {
    // call_doubler calls $double (params 1, results 1).
    let module = Module::decode(&wat_from_file("func_call.wat")).expect("decodes");
    let program = lift(&module).expect("lifts");
    assert_eq!(program.functions.len(), 2);
    assert_eq!(program.functions[0].func_index, 0);
    assert_eq!(program.functions[1].func_index, 1);
    // call_doubler: local.get 0, call $double, end
    let caller = &program.functions[0];
    assert_eq!(caller.instrs[1].reads, [Reg::Stack(0)]);
    assert_eq!(caller.instrs[1].writes, [Reg::Stack(0)]);
    assert_eq!(caller.instrs[1].successors, Successors::Fallthrough);
}

#[test]
fn fibonacci_if_branches_around_then_arm() {
    let module = Module::decode(&wat_from_file("fibonacci.wat")).expect("decodes");
    let program = lift(&module).expect("lifts");
    let f = &program.functions[0];
    // local.get, i32.const, i32.lt_s, if(pc3), i32.const, return(pc5), end(pc6), ...
    match &f.instrs[3].successors {
        Successors::Branch { taken, not_taken } => {
            assert_eq!(*taken, 4, "then-arm begins right after the `if`");
            assert_eq!(
                *not_taken, 7,
                "false condition skips past the then-arm's `end`"
            );
        }
        other => panic!("expected a Branch at the `if`, got {other:?}"),
    }
    assert_eq!(
        f.instrs[5].successors,
        Successors::Return,
        "then-arm `return`"
    );
    assert_eq!(
        f.instrs.last().expect("non-empty body").successors,
        Successors::Return,
        "function's final `end`",
    );
}

#[test]
fn if_else_wires_then_else_and_join() {
    let module = Module::decode(&wat_from_str(WAT_SNIPPETS[4].1)).expect("decodes");
    let program = lift(&module).expect("lifts");
    let f = &program.functions[0];
    // local.get(0), if(1), i32.const(2), else(3), i32.const(4), end(5), end(6)
    assert_eq!(
        f.instrs[1].successors,
        Successors::Branch {
            taken: 2,
            not_taken: 4
        },
        "`if` enters then-arm or jumps to the else body",
    );
    assert_eq!(
        f.instrs[3].successors,
        Successors::Jump(6),
        "`else` skips the else body's `end` to the join",
    );
}

#[test]
fn loop_branch_targets_header() {
    let module = Module::decode(&wat_from_str(WAT_SNIPPETS[1].1)).expect("decodes");
    let program = lift(&module).expect("lifts");
    let f = &program.functions[0];
    // loop(0), local.get(1), br_if(2), end(3), end(4)
    assert_eq!(
        f.instrs[2].successors,
        Successors::Branch {
            taken: 1,
            not_taken: 3
        },
        "`br_if` re-enters the loop header (pc 1) or falls through",
    );
}

#[test]
fn forward_branch_targets_continuation() {
    let module = Module::decode(&wat_from_str(WAT_SNIPPETS[2].1)).expect("decodes");
    let program = lift(&module).expect("lifts");
    let f = &program.functions[0];
    // block(0), br(1), end(2), end(3): br exits the block to pc 3
    assert_eq!(f.instrs[1].successors, Successors::Jump(3));
}
