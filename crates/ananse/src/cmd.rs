//! Per-command implementations for the `ananse` command-line interface.

use std::io::{self, Write};
use std::path::Path;
use std::process;
use std::time::Instant;

use ananse::decoder::{ExportKind, WordType};
use ananse::prelude::*;

/// Prints an `error: ...` line and exits the process with a non-zero status.
fn die(message: impl AsRef<str>) -> ! {
    eprintln!("error: {}", message.as_ref());
    process::exit(1);
}

/// Rejects a module path whose extension is not one `command` accepts.
fn require_extension(path: &Path, allowed: &[&str], command: &str) {
    let actual = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !allowed.contains(&actual) {
        let expected = allowed
            .iter()
            .map(|e| format!(".{e}"))
            .collect::<Vec<_>>()
            .join(" or ");
        die(format!(
            "`ananse {command}` expects a {expected} file, got '{}'",
            path.display()
        ));
    }
}

/// Writes the guest journal to standard output, then each returned value on its
/// own line. Command-mode runs (a bare `_start`) pass no returns, since a
/// command's output is its journal and its `_start` result is not a value.
fn emit(journal: &[u8], returns: &[Word]) -> io::Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(journal)?;
    for value in returns {
        writeln!(out, "{}", format_word(*value))?;
    }
    out.flush()
}

/// Decodes, lifts, and executes a module through the deterministic WASI host.
///
/// A bare `_start` invocation runs in command mode: its journal is written to
/// standard output and the process adopts the guest's `proc_exit` status.
/// Otherwise (an explicit `--invoke`, or an `_start`-less module whose entry
/// carries a result) the entry's returned values follow the journal on standard
/// output. `--verbose` adds a step-count and timing summary on standard error.
pub fn run(module_path: &Path, invoke: Option<&str>, verbose: bool, raw_args: &[String]) {
    require_extension(module_path, &["wasm", "wat"], "run");
    let bytes = wat::parse_file(module_path)
        .unwrap_or_else(|e| die(format!("cannot load '{}': {e}", module_path.display())));

    let mut runtime = Runtime::instantiate_with_wasi(&bytes)
        .unwrap_or_else(|e| die(format!("{}: {e}", module_path.display())));

    let entry = invoke
        .map(|name| Entry::Export(name.to_string()))
        .unwrap_or(Entry::Auto);
    let command_mode = invoke.is_none() && exports_start(runtime.module());
    let args = coerce_arguments(&runtime, &entry, raw_args);

    let started = Instant::now();
    let execution = runtime
        .call(&entry, &args)
        .unwrap_or_else(|e| die(format!("{}: {e}", module_path.display())));
    let elapsed = started.elapsed();

    let returns = if command_mode {
        &[]
    } else {
        &execution.returns[..]
    };
    emit(runtime.host().journal(), returns)
        .unwrap_or_else(|e| die(format!("cannot write output: {e}")));

    if verbose {
        eprintln!("{} steps in {elapsed:.2?}", execution.steps);
    }
    if let Some(code) = execution.exit {
        process::exit(code);
    }
}

/// Whether the module exports a `_start` function---the WASI command entry a
/// bare `ananse run` targets, whose result is not a returned value.
fn exports_start(module: &Module) -> bool {
    module
        .exports()
        .iter()
        .any(|export| export.name == "_start" && export.kind == ExportKind::Function)
}

/// Coerces the raw string arguments to the [`Word`] each parameter of `entry`
/// expects, rejecting an argument count or value that does not fit.
fn coerce_arguments(
    runtime: &Runtime<WasiSnapshotPreview1>,
    entry: &Entry,
    raw: &[String],
) -> Vec<Word> {
    let parameters = runtime
        .parameters(entry)
        .unwrap_or_else(|e| die(format!("{e}")));
    if raw.len() != parameters.len() {
        die(format!(
            "{} expects {}, got {}",
            entry_subject(entry),
            expected_arguments(&parameters),
            raw.len()
        ));
    }
    parameters
        .iter()
        .zip(raw)
        .map(|(&ty, text)| coerce_word(ty, text))
        .collect()
}

/// Names the entry in a diagnostic: the export when one was requested, else a
/// generic reference to the module's default entry.
fn entry_subject(entry: &Entry) -> String {
    match entry {
        Entry::Export(name) => format!("`{name}`"),
        _ => "the module entry".to_string(),
    }
}

/// A phrase for the arguments an entry expects: `no arguments`,
/// `1 argument (i32)`, or `2 arguments (i32, i64)`.
fn expected_arguments(parameters: &[WordType]) -> String {
    if parameters.is_empty() {
        return "no arguments".to_string();
    }
    let types = parameters
        .iter()
        .map(|ty| match ty {
            WordType::I32 => "i32",
            WordType::I64 => "i64",
        })
        .collect::<Vec<_>>()
        .join(", ");
    let noun = if parameters.len() == 1 {
        "argument"
    } else {
        "arguments"
    };
    format!("{} {noun} ({types})", parameters.len())
}

fn coerce_word(ty: WordType, text: &str) -> Word {
    match ty {
        WordType::I32 => Word::I32(parse_u32(text)),
        WordType::I64 => Word::I64(parse_u64(text)),
    }
}

/// Parses an `i32` argument from a signed or unsigned decimal, or a `0x` hex
/// literal, keeping its 32-bit two's-complement bit pattern.
fn parse_u32(text: &str) -> u32 {
    if let Some(bits) = parse_hex(text) {
        return u32::try_from(bits)
            .unwrap_or_else(|_| die(format!("i32 argument '{text}' is out of range")));
    }
    text.parse::<i32>()
        .map(|value| value as u32)
        .or_else(|_| text.parse::<u32>())
        .unwrap_or_else(|_| die(format!("'{text}' is not a valid i32 argument")))
}

/// Parses an `i64` argument from a signed or unsigned decimal, or a `0x` hex
/// literal, keeping its 64-bit two's-complement bit pattern.
fn parse_u64(text: &str) -> u64 {
    if let Some(bits) = parse_hex(text) {
        return bits;
    }
    text.parse::<i64>()
        .map(|value| value as u64)
        .or_else(|_| text.parse::<u64>())
        .unwrap_or_else(|_| die(format!("'{text}' is not a valid i64 argument")))
}

fn parse_hex(text: &str) -> Option<u64> {
    text.strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .map(|hex| {
            u64::from_str_radix(hex, 16)
                .unwrap_or_else(|_| die(format!("'{text}' is not a valid hexadecimal argument")))
        })
}

fn format_word(word: Word) -> String {
    match word {
        Word::I32(value) => value.to_string(),
        Word::I64(value) => value.to_string(),
    }
}
