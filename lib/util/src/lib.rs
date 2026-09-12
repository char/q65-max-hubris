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
