//! Deterministic minimal `wasi_snapshot_preview1` for the Ananse zkVM.
//!
//! Ananse forbids nondeterministic syscalls, so its WASI surface is exactly the
//! functions whose behaviour is a pure function of the guest's own linear memory:
//! currently `fd_write` and `proc_exit`. Every other WASI import is
//! rejected by [`ananse_decoder`] at validation time.
//!
//! Unlike a native runtime, [`WasiSnapshotPreview1`] never touches an operating
//! system file descriptor. `fd_write` gathers the bytes the guest points at and
//! appends them to an in-memory [journal](WasiSnapshotPreview1::journal) the
//! caller reads after the run; `proc_exit` halts the program with its status
//! code through [`HostAction::Exit`]. The record stream an execution emits is
//! therefore a deterministic function of `(module, entry, arguments)` alone.

#![forbid(unsafe_code)]

use ananse_decoder::{ImportEntry, WASI_MODULE};
use ananse_executor::{ExecuteError, Host, HostAction, Result, Word};

/// The WASI success `errno`, returned to the guest by a completed `fd_write`.
const ERRNO_SUCCESS: u32 = 0;

/// Bytes per `ciovec`: a `(buf: u32, buf_len: u32)` little-endian pair.
const CIOVEC_LEN: usize = 8;

/// A deterministic in-memory `wasi_snapshot_preview1` host.
#[derive(Clone, Debug, Default)]
pub struct WasiSnapshotPreview1 {
    journal: Vec<u8>,
}

impl WasiSnapshotPreview1 {
    /// A host with an empty journal.
    pub fn new() -> Self {
        Self::default()
    }

    /// The bytes the guest has written through `fd_write`, in write order.
    pub fn journal(&self) -> &[u8] {
        &self.journal
    }

    /// Gathers the `iovec` array described by `args = [fd, iovs, iovs_len,
    /// nwritten]` into the journal and reports the byte count back to the guest
    /// at the `nwritten` pointer.
    fn fd_write(
        &mut self,
        import: &ImportEntry,
        args: &[Word],
        memory: &mut [u8],
    ) -> Result<HostAction> {
        let [_fd, iovs, iovs_len, nwritten] = arguments(import, args)?;
        let base = iovs as usize;
        let mut total: u32 = 0;
        for i in 0..iovs_len {
            let entry = (i as usize)
                .checked_mul(CIOVEC_LEN)
                .and_then(|offset| base.checked_add(offset))
                .ok_or_else(|| fault(import))?;
            let (ptr, len) = read_ciovec(memory, entry).ok_or_else(|| fault(import))?;
            let bytes = slice(memory, ptr, len).ok_or_else(|| fault(import))?;
            self.journal.extend_from_slice(bytes);
            total = total.checked_add(len).ok_or_else(|| fault(import))?;
        }
        write_u32(memory, nwritten as usize, total).ok_or_else(|| fault(import))?;
        Ok(HostAction::Return(vec![Word::I32(ERRNO_SUCCESS)]))
    }
}

impl Host for WasiSnapshotPreview1 {
    fn call(
        &mut self,
        import: &ImportEntry,
        args: &[Word],
        memory: &mut [u8],
    ) -> Result<HostAction> {
        match (import.module.as_str(), import.name.as_str()) {
            (WASI_MODULE, "fd_write") => self.fd_write(import, args, memory),
            (WASI_MODULE, "proc_exit") => proc_exit(import, args),
            _ => Err(unsupported(import)),
        }
    }
}

/// Halts the program with the status code carried by `args = [code]`.
fn proc_exit(import: &ImportEntry, args: &[Word]) -> Result<HostAction> {
    match args {
        [code] => Ok(HostAction::Exit(as_u32(*code) as i32)),
        _ => Err(arity(import, "proc_exit", 1, args.len())),
    }
}

/// Extracts `fd_write`'s four `i32` arguments. The decoder validates the import
/// name but not its signature, so a malformed module can still reach the host
/// with the wrong arity; that is rejected here rather than indexed blindly.
fn arguments(import: &ImportEntry, args: &[Word]) -> Result<[u32; 4]> {
    match args {
        [fd, iovs, iovs_len, nwritten] => Ok([
            as_u32(*fd),
            as_u32(*iovs),
            as_u32(*iovs_len),
            as_u32(*nwritten),
        ]),
        _ => Err(arity(import, "fd_write", 4, args.len())),
    }
}

fn as_u32(word: Word) -> u32 {
    match word {
        Word::I32(bits) => bits,
        Word::I64(bits) => bits as u32,
    }
}

fn read_ciovec(memory: &[u8], at: usize) -> Option<(u32, u32)> {
    let buf = read_u32(memory, at)?;
    let buf_len = read_u32(memory, at.checked_add(4)?)?;
    Some((buf, buf_len))
}

fn read_u32(memory: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    let bytes: [u8; 4] = memory.get(at..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn slice(memory: &[u8], ptr: u32, len: u32) -> Option<&[u8]> {
    let start = ptr as usize;
    let end = start.checked_add(len as usize)?;
    memory.get(start..end)
}

fn write_u32(memory: &mut [u8], at: usize, value: u32) -> Option<()> {
    let end = at.checked_add(4)?;
    memory
        .get_mut(at..end)?
        .copy_from_slice(&value.to_le_bytes());
    Some(())
}

fn fault(import: &ImportEntry) -> ExecuteError {
    host_error(import, "memory access outside linear memory".into())
}

fn arity(import: &ImportEntry, func: &str, expected: usize, actual: usize) -> ExecuteError {
    host_error(
        import,
        format!("{func} expects {expected} arguments, got {actual}"),
    )
}

fn unsupported(import: &ImportEntry) -> ExecuteError {
    host_error(import, "unsupported WASI import".into())
}

fn host_error(import: &ImportEntry, message: String) -> ExecuteError {
    ExecuteError::Host {
        module: import.module.clone(),
        name: import.name.clone(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wasi(name: &str) -> ImportEntry {
        ImportEntry {
            module: WASI_MODULE.to_string(),
            name: name.to_string(),
        }
    }

    #[test]
    fn fd_write_gathers_every_iovec_into_the_journal() {
        // iovec table at 0: (ptr=16, len=5), (ptr=21, len=6); data at 16.
        let mut memory = vec![0u8; 64];
        memory[0..4].copy_from_slice(&16u32.to_le_bytes());
        memory[4..8].copy_from_slice(&5u32.to_le_bytes());
        memory[8..12].copy_from_slice(&21u32.to_le_bytes());
        memory[12..16].copy_from_slice(&6u32.to_le_bytes());
        memory[16..21].copy_from_slice(b"hello");
        memory[21..27].copy_from_slice(b" world");

        let mut host = WasiSnapshotPreview1::new();
        let action = host
            .call(
                &wasi("fd_write"),
                &[Word::I32(1), Word::I32(0), Word::I32(2), Word::I32(32)],
                &mut memory,
            )
            .expect("fd_write succeeds");

        assert_eq!(action, HostAction::Return(vec![Word::I32(0)]));
        assert_eq!(host.journal(), b"hello world");
        assert_eq!(read_u32(&memory, 32), Some(11));
    }

    #[test]
    fn fd_write_rejects_an_out_of_bounds_buffer() {
        let mut memory = vec![0u8; 16];
        memory[0..4].copy_from_slice(&8u32.to_le_bytes());
        memory[4..8].copy_from_slice(&100u32.to_le_bytes());

        let err = WasiSnapshotPreview1::new()
            .call(
                &wasi("fd_write"),
                &[Word::I32(1), Word::I32(0), Word::I32(1), Word::I32(8)],
                &mut memory,
            )
            .unwrap_err();

        assert!(matches!(err, ExecuteError::Host { .. }));
    }

    #[test]
    fn fd_write_rejects_mismatched_arity() {
        let err = WasiSnapshotPreview1::new()
            .call(
                &wasi("fd_write"),
                &[Word::I32(1), Word::I32(0), Word::I32(1)],
                &mut [],
            )
            .unwrap_err();

        assert!(matches!(err, ExecuteError::Host { .. }));
    }

    #[test]
    fn proc_exit_halts_with_its_status() {
        let action = WasiSnapshotPreview1::new()
            .call(&wasi("proc_exit"), &[Word::I32(7)], &mut [])
            .expect("proc_exit succeeds");
        assert_eq!(action, HostAction::Exit(7));
    }
}
