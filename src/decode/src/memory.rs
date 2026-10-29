pub use ipc::read::{Memory, read_string};

/// One object's bytes, copied with a single read so its fields cost no more reads; reads outside the copy, and every read
/// when the copy failed, go to the memory underneath. Each read is a syscall when reading another process (and in the agent).
pub struct Snapshot<'a, M: Memory> {
    memory: &'a M,
    base: u64,
    bytes: Vec<u8>,
    len: usize,
}

impl<'a, M: Memory> Snapshot<'a, M> {
    /// Snapshots of `len` bytes over `memory`, taken with `load`; with `len` 0 every read goes through.
    pub fn new(memory: &'a M, len: usize) -> Self {
        Self { memory, base: 0, bytes: vec![0; len], len: 0 }
    }

    /// Copies the span at `base`, replacing the last one.
    pub fn load(&mut self, base: u64) {
        self.base = base;
        self.len = if !self.bytes.is_empty() && self.memory.read(base, &mut self.bytes) { self.bytes.len() } else { 0 };
    }
}

impl<M: Memory> Memory for Snapshot<'_, M> {
    fn read(&self, address: u64, buffer: &mut [u8]) -> bool {
        let start = address.wrapping_sub(self.base) as usize;
        match start.checked_add(buffer.len()) {
            Some(end) if address >= self.base && end <= self.len => {
                buffer.copy_from_slice(&self.bytes[start..end]);
                true
            }
            _ => self.memory.read(address, buffer),
        }
    }

    fn write(&self, address: u64, bytes: &[u8]) -> bool {
        self.memory.write(address, bytes)
    }
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct Fake(pub std::cell::RefCell<std::collections::HashMap<u64, u8>>);

#[cfg(any(test, feature = "test-support"))]
impl Fake {
    pub fn put(&mut self, address: u64, bytes: &[u8]) {
        for (i, &b) in bytes.iter().enumerate() {
            self.0.get_mut().insert(address + i as u64, b);
        }
    }
    pub fn put_u64(&mut self, address: u64, value: u64) {
        self.put(address, &value.to_le_bytes());
    }
    pub fn put_i32(&mut self, address: u64, value: i32) {
        self.put(address, &value.to_le_bytes());
    }
    pub fn put_short_string(&mut self, address: u64, text: &str) {
        let mut b = [0u8; 24];
        b[..text.len()].copy_from_slice(text.as_bytes());
        b[23] = (23 - text.len()) as u8;
        self.put(address, &b);
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Memory for Fake {
    fn read(&self, address: u64, buffer: &mut [u8]) -> bool {
        for (i, b) in buffer.iter_mut().enumerate() {
            match self.0.borrow().get(&(address + i as u64)) {
                Some(&v) => *b = v,
                None => return false,
            }
        }
        true
    }

    fn write(&self, address: u64, bytes: &[u8]) -> bool {
        let mut map = self.0.borrow_mut();
        if (0..bytes.len() as u64).any(|i| !map.contains_key(&(address + i))) {
            return false;
        }
        for (i, &b) in bytes.iter().enumerate() {
            map.insert(address + i as u64, b);
        }
        true
    }
}

// Counts the reads that reach it.
#[cfg(test)]
pub(crate) struct Counting<'a>(pub &'a Fake, pub std::cell::Cell<usize>);

#[cfg(test)]
impl Memory for Counting<'_> {
    fn read(&self, address: u64, buffer: &mut [u8]) -> bool {
        self.1.set(self.1.get() + 1);
        self.0.read(address, buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_serves_its_span_and_passes_the_rest_through() {
        let mut m = Fake::default();
        m.put(0x100, &(0..32).collect::<Vec<u8>>());
        m.put_i32(0x200, 7);
        let counting = Counting(&m, Default::default());
        let mut snapshot = Snapshot::new(&counting, 32);
        snapshot.load(0x100);
        assert_eq!(counting.1.get(), 1);
        assert_eq!(snapshot.i32(0x104), Some(i32::from_le_bytes([4, 5, 6, 7])));
        assert_eq!(snapshot.u64(0x118), Some(u64::from_le_bytes([24, 25, 26, 27, 28, 29, 30, 31])));
        assert_eq!(counting.1.get(), 1);
        // past the end, straddling it and before it: read through
        assert_eq!(snapshot.i32(0x200), Some(7));
        assert_eq!(snapshot.i32(0x11e), None);
        assert_eq!(snapshot.i32(0xfe), None);
        assert_eq!(counting.1.get(), 4);
    }

    #[test]
    fn snapshot_of_unreadable_or_empty_span_reads_through() {
        let mut m = Fake::default();
        m.put_i32(0x100, 7);
        let mut unreadable = Snapshot::new(&m, 64);
        unreadable.load(0x100);
        assert_eq!(unreadable.i32(0x100), Some(7));
        let mut empty = Snapshot::new(&m, 0);
        empty.load(0x100);
        assert_eq!(empty.i32(0x100), Some(7));
    }

    #[test]
    fn reads_short_and_long_strings() {
        let mut m = Fake::default();
        let mut b = [0u8; 24];
        b[..8].copy_from_slice(b"Banker\0k");
        b[23] = 17;
        m.put(0x100, &b);
        assert_eq!(read_string(&m, 0x100).as_deref(), Some("Banker"));

        m.put(0x1000, b"Grand Exchange Clerk and more");
        m.put_u64(0x200, 0x1000);
        m.put_u64(0x208, 20);
        m.put_u64(0x210, 0x8000_0000_0000_0020);
        assert_eq!(read_string(&m, 0x200).as_deref(), Some("Grand Exchange Clerk"));
    }
}
