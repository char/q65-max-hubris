#![no_std]

pub struct Bytes<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> Default for Bytes<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Bytes<N> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }

    pub const fn push(&mut self, byte: u8) {
        self.bytes[self.len] = byte;
        self.len += 1;
    }

    pub const fn extend(&mut self, data: &[u8]) {
        let mut i = 0;
        while i < data.len() {
            self.push(data[i]);
            i += 1;
        }
    }

    /// # Panics
    /// If fewer than `N` bytes were written.
    #[must_use]
    pub const fn finish(self) -> [u8; N] {
        assert!(self.len == N);
        self.bytes
    }
}

/// A memory-mapped 32-bit register at a fixed address, for where the PAC gets in the way: registers
/// it doesn't know about, or ones that need addressing by index rather than by name.
#[derive(Clone, Copy)]
pub struct Reg(pub usize);

impl Reg {
    #[must_use]
    pub fn read(self) -> u32 {
        unsafe { core::ptr::read_volatile(self.0 as *const u32) }
    }

    pub fn write(self, value: u32) {
        unsafe { core::ptr::write_volatile(self.0 as *mut u32, value) }
    }

    pub fn modify(self, f: impl FnOnce(u32) -> u32) {
        self.write(f(self.read()));
    }

    pub fn set(self, bits: u32) {
        self.modify(|value| value | bits);
    }
}
