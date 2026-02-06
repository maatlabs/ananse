use std::fs::File;
use std::io::prelude::*;
#[cfg(unix)]
use std::os::fd::FromRawFd;
#[cfg(windows)]
use std::os::windows::io::FromRawHandle;

use super::{Store, Value};

/// WASI snapshot preview1 implementation.
#[derive(Default)]
pub struct WasiSnapshotPreview1 {
    /// Open file descriptors.
    pub file_table: Vec<Box<File>>,
}

impl WasiSnapshotPreview1 {
    /// Creates a new WASI instance with stdin, stdout, and stderr.
    pub fn new() -> Self {
        #[cfg(unix)]
        unsafe {
            Self {
                file_table: vec![
                    Box::new(File::from_raw_fd(0)),
                    Box::new(File::from_raw_fd(1)),
                    Box::new(File::from_raw_fd(2)),
                ],
            }
        }

        #[cfg(windows)]
        unsafe {
            use std::os::windows::io::RawHandle;
            const STD_INPUT_HANDLE: u32 = 0xFFFFFFF6_u32;
            const STD_OUTPUT_HANDLE: u32 = 0xFFFFFFF5_u32;
            const STD_ERROR_HANDLE: u32 = 0xFFFFFFF4_u32;

            extern "system" {
                fn GetStdHandle(nStdHandle: u32) -> RawHandle;
            }

            Self {
                file_table: vec![
                    Box::new(File::from_raw_handle(GetStdHandle(STD_INPUT_HANDLE))),
                    Box::new(File::from_raw_handle(GetStdHandle(STD_OUTPUT_HANDLE))),
                    Box::new(File::from_raw_handle(GetStdHandle(STD_ERROR_HANDLE))),
                ],
            }
        }
    }

    /// Invokes a WASI function by name.
    ///
    /// # Errors
    ///
    /// Returns an error if the function is not supported or execution fails.
    pub fn invoke(
        &mut self,
        store: &mut Store,
        func: &str,
        args: Vec<Value>,
    ) -> anyhow::Result<Option<Value>> {
        match func {
            "fd_write" => self.fd_write(store, args),
            _ => anyhow::bail!("unsupported WASI function: {func}"),
        }
    }

    fn fd_write(&mut self, store: &mut Store, args: Vec<Value>) -> anyhow::Result<Option<Value>> {
        let args: Vec<i32> = args
            .into_iter()
            .map(|v| v.try_into().map_err(|_| anyhow::anyhow!("type mismatch")))
            .collect::<anyhow::Result<Vec<i32>>>()?;

        let fd = args[0];
        let mut iovs = args[1] as usize;
        let iovs_len = args[2];
        let rp = args[3] as usize;

        let file = self
            .file_table
            .get_mut(fd as usize)
            .ok_or(anyhow::anyhow!("not found fd"))?;
        let memory = store
            .memories
            .get_mut(0)
            .ok_or(anyhow::anyhow!("not found memory"))?;

        let mut nwritten = 0;

        for _ in 0..iovs_len {
            let start = memory_read(&memory.data, iovs)? as usize;
            iovs += 4;

            let len = memory_read(&memory.data, iovs)?;
            iovs += 4;

            let end = start + len as usize;
            nwritten += file.write(&memory.data[start..end])?;
        }

        memory_write(&mut memory.data, rp, &nwritten.to_le_bytes())?;

        Ok(Some(0.into()))
    }
}

fn memory_read(buf: &[u8], start: usize) -> anyhow::Result<i32> {
    let end = start + 4;
    Ok(<i32>::from_le_bytes(buf[start..end].try_into()?))
}

fn memory_write(buf: &mut [u8], start: usize, data: &[u8]) -> anyhow::Result<()> {
    let end = start + data.len();
    buf[start..end].copy_from_slice(data);
    Ok(())
}
