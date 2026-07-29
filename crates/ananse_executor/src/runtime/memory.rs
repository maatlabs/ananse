use ananse_decoder::WASM32_PAGE_SIZE;

use crate::{Result, Trap};

pub struct Memory {
    bytes: Vec<u8>,
    max_pages: Option<u64>,
}

impl Memory {
    pub fn new(bytes: Vec<u8>, max_pages: Option<u64>) -> Self {
        Self { bytes, max_pages }
    }

    /// Reads `width` little-endian bytes at `base + offset`, returning the
    /// resolved address and raw zero-extended value.
    pub fn read(&self, base: u32, offset: u64, width: usize) -> Result<(u64, u64)> {
        let address = u64::from(base)
            .checked_add(offset)
            .ok_or(Trap::MemoryOutOfBounds)?;
        let end = address
            .checked_add(width as u64)
            .filter(|&e| e <= self.bytes.len() as u64)
            .ok_or(Trap::MemoryOutOfBounds)? as usize;
        let raw = self.bytes[address as usize..end]
            .iter()
            .enumerate()
            .fold(0u64, |acc, (i, &byte)| acc | (u64::from(byte) << (8 * i)));
        Ok((address, raw))
    }

    /// Writes the low `width` bytes of `raw` at `base + offset`, returning
    /// the resolved address.
    pub fn write(&mut self, base: u32, offset: u64, width: usize, raw: u64) -> Result<u64> {
        let address = u64::from(base)
            .checked_add(offset)
            .ok_or(Trap::MemoryOutOfBounds)?;
        let end = address
            .checked_add(width as u64)
            .filter(|&e| e <= self.bytes.len() as u64)
            .ok_or(Trap::MemoryOutOfBounds)? as usize;
        for (i, slot) in self.bytes[address as usize..end].iter_mut().enumerate() {
            *slot = (raw >> (8 * i)) as u8;
        }
        Ok(address)
    }

    pub fn size_pages(&self) -> u32 {
        (self.bytes.len() / WASM32_PAGE_SIZE) as u32
    }

    /// Grows by `delta` pages, returning the previous page count, or
    /// `u32::MAX` if the grow would exceed the module or instance limit.
    pub fn grow(&mut self, delta: usize) -> u32 {
        let old_pages = self.bytes.len() / WASM32_PAGE_SIZE;
        let grown = old_pages
            .checked_add(delta)
            .filter(|&n| n <= WASM32_PAGE_SIZE)
            .filter(|&n| self.max_pages.is_none_or(|m| n as u64 <= m))
            .and_then(|n| n.checked_mul(WASM32_PAGE_SIZE).map(|bytes| (n, bytes)));
        match grown {
            Some((_, bytes)) => {
                self.bytes.resize(bytes, 0);
                old_pages as u32
            }
            None => u32::MAX,
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.bytes
    }
}
