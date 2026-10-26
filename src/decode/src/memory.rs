pub use ipc::read::{Memory, read_string};

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

#[cfg(test)]
mod tests {
    use super::*;

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
