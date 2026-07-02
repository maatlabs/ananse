use ananse_air::{pack_edge, program_rom};
use ananse_decoder::Module;
use ananse_executor::{Entry, Transition, execute, function_opcodes};
use ananse_lift::{Register, lift};
use ananse_tests::{SINGLE_FRAME_FIXTURES, TestHost, wat_from_file};
use ananse_trace::selector::{SEL_PADDING, opcode_index};

/// Every intra-body control-flow edge an execution actually takes must be a member
/// of the program ROM the verifier reconstructs; otherwise the lookup that binds
/// the trace to the module could not close. Terminal transitions (return, host
/// exit) leave the body and are handled by the AIR's halt gating, not here.
#[test]
fn program_rom_contains_every_executed_edge() {
    let mut checked = 0usize;
    for name in SINGLE_FRAME_FIXTURES {
        let module = Module::decode(&wat_from_file(name)).expect("decode");
        let program = lift(&module).expect("lift");
        let mut host = TestHost::default();
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
            let schedule = &function.instrs[pc];
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
                let height = function.instrs[pc].height_in;
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
        let halt = u32::try_from(function.instrs.len()).expect("body fits u32");
        let halt_loop = pack_edge(halt, SEL_PADDING, halt, 0, 0).expect("halt edge packs");
        assert!(rom.contains(&halt_loop), "{name}: halt self-loop absent");
    }
    assert!(checked > 0, "no executed edges were checked");
}
