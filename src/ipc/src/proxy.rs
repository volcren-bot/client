use alloc::{boxed::Box, format, string::String};
use core::{
    ptr::{addr_of, addr_of_mut, NonNull},
    sync::atomic::{AtomicI32, AtomicU32, Ordering},
};

use crate::state::{seqlock_read, seqlock_write};

pub const VERSION: u32 = 1;
pub const MEMORY_SIZE: usize = 4096;
const FIELD_LEN: usize = 255;

pub fn memory_name(pid: u32) -> String {
    format!("AGENT_PROXY_{pid}")
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Field {
    len: u8,
    bytes: [u8; FIELD_LEN],
}

impl Field {
    const EMPTY: Self = Self {
        len: 0,
        bytes: [0; FIELD_LEN],
    };

    pub(crate) fn new(text: &str) -> Option<Self> {
        let len = u8::try_from(text.len()).ok()?;
        let mut bytes = [0; FIELD_LEN];
        bytes[..text.len()].copy_from_slice(text.as_bytes());
        Some(Self { len, bytes })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(self.bytes()).into_owned()
    }
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Request {
    pub enabled: u32,
    pub port: u16,
    pub reserved: u16,
    pub host: Field,
    pub username: Field,
    pub password: Field,
}

impl Request {
    pub const OFF: Self = Self {
        enabled: 0,
        port: 0,
        reserved: 0,
        host: Field::EMPTY,
        username: Field::EMPTY,
        password: Field::EMPTY,
    };

    pub fn new(host: &str, port: u16, username: &str, password: &str) -> Result<Self, String> {
        let field = |text: &str, what: &str| {
            Field::new(text).ok_or_else(|| format!("The {what} is longer than {FIELD_LEN} bytes."))
        };
        if host.is_empty() {
            return Err("The proxy needs a host.".into());
        }
        if port == 0 {
            return Err("The proxy's port can't be 0.".into());
        }
        if username.is_empty() && !password.is_empty() {
            return Err("A password needs a username.".into());
        }
        Ok(Self {
            enabled: 1,
            port,
            reserved: 0,
            host: field(host, "host")?,
            username: field(username, "username")?,
            password: field(password, "password")?,
        })
    }

    pub fn logs_in(&self) -> bool {
        self.username.len > 0
    }
}

pub mod state {
    pub const OFF: i32 = 0;
    pub const ON: i32 = 1;
    pub const UNREADABLE: i32 = -1;
    pub const UNHOOKED: i32 = -2;
}

pub mod error {
    use alloc::{format, string::String};

    pub const NONE: i32 = 0;
    pub const RESOLVE: i32 = 1;
    pub const CONNECT: i32 = 2;
    pub const TIMEOUT: i32 = 3;
    pub(crate) const METHOD: i32 = 4;
    pub(crate) const AUTH: i32 = 5;
    pub const REPLY: i32 = 6;
    pub const FAMILY: i32 = 7;
    pub const DATAGRAM: i32 = 8;
    pub const UNREADABLE: i32 = 9;
    pub const CHANGED: i32 = 10;
    pub(crate) const REFUSED: i32 = 0x100;

    pub fn describe(code: i32) -> String {
        let text = match code {
            NONE => "no error",
            RESOLVE => "the proxy's host didn't resolve",
            CONNECT => "couldn't connect to the proxy",
            TIMEOUT => "the proxy didn't answer in time",
            METHOD => "the proxy wants a login it wasn't given, or refused the one offered",
            AUTH => "the proxy refused the username or password",
            REPLY => "the proxy didn't answer like a SOCKS5 proxy",
            FAMILY => {
                "the proxy has no address of the kind the client connects with (IPv4 or IPv6)"
            }
            DATAGRAM => "blocked a UDP connection, which a SOCKS5 connect can't carry",
            UNREADABLE => "blocked a connection because the proxy setting couldn't be read",
            CHANGED => "the proxy setting changed while connecting",
            _ if code & REFUSED != 0 => {
                let reason = match code & 0xff {
                    1 => "general failure",
                    2 => "not allowed by its rules",
                    3 => "network unreachable",
                    4 => "host unreachable",
                    5 => "connection refused",
                    6 => "timed out",
                    7 => "command not supported",
                    8 => "address type not supported",
                    _ => "unknown reply",
                };
                return format!("the proxy couldn't reach the game server ({reason})");
            }
            _ => "unknown error",
        };
        text.into()
    }
}

#[repr(C)]
pub struct Status {
    pub adopted: AtomicU32,
    pub state: AtomicI32,
    pub proxied: AtomicU32,
    pub failed: AtomicU32,
    pub dropped: AtomicU32,
    pub last_error: AtomicI32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatusView {
    pub adopted: u32,
    pub state: i32,
    pub proxied: u32,
    pub failed: u32,
    pub dropped: u32,
    pub last_error: i32,
}

impl Status {
    pub fn view(&self) -> StatusView {
        StatusView {
            adopted: self.adopted.load(Ordering::Acquire),
            state: self.state.load(Ordering::Acquire),
            proxied: self.proxied.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            last_error: self.last_error.load(Ordering::Relaxed),
        }
    }

    pub fn adopt(&self, sequence: u32, state: i32) {
        self.state.store(state, Ordering::Release);
        self.adopted.store(sequence, Ordering::Release);
    }

    pub fn fail(&self, error: i32) {
        self.last_error.store(error, Ordering::Relaxed);
        self.failed.fetch_add(1, Ordering::Relaxed);
    }
}

#[repr(C)]
struct ProxyMemory {
    version: u32,
    sequence: u32,
    request: Request,
    status: Status,
}

#[derive(Clone)]
pub enum Setting {
    Off,
    On(Box<Request>),
    Unreadable,
}

#[derive(Clone, Copy)]
pub struct Shared {
    ptr: NonNull<ProxyMemory>,
}

unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

impl Shared {
    pub unsafe fn attach(ptr: NonNull<u8>) -> Self {
        Self { ptr: ptr.cast() }
    }

    fn this(self) -> *mut ProxyMemory {
        self.ptr.as_ptr()
    }

    pub fn write(self, request: &Request) -> u32 {
        let this = self.this();
        unsafe {
            AtomicU32::from_ptr(addr_of_mut!((*this).version)).store(VERSION, Ordering::Release);
            seqlock_write(
                addr_of_mut!((*this).sequence),
                addr_of_mut!((*this).request),
                request,
            )
        }
    }

    pub fn read(self) -> (u32, Setting) {
        let this = self.this();
        unsafe {
            let version =
                AtomicU32::from_ptr(addr_of_mut!((*this).version)).load(Ordering::Acquire);
            let sequence =
                AtomicU32::from_ptr(addr_of_mut!((*this).sequence)).load(Ordering::Acquire);
            if version == 0 && sequence == 0 {
                return (0, Setting::Off);
            }
            if version != VERSION {
                return (sequence, Setting::Unreadable);
            }
            match seqlock_read(addr_of!((*this).sequence), addr_of!((*this).request), 1000) {
                Some((sequence, request)) if request.enabled != 0 => {
                    (sequence, Setting::On(Box::new(request)))
                }
                Some((sequence, _)) => (sequence, Setting::Off),
                None => (sequence, Setting::Unreadable),
            }
        }
    }

    pub fn status<'a>(self) -> &'a Status {
        unsafe { &*addr_of!((*self.this()).status) }
    }
}

pub mod socks {
    use super::error;
    use alloc::{vec, vec::Vec};

    const VERSION: u8 = 5;
    const NO_LOGIN: u8 = 0;
    const LOGIN: u8 = 2;
    const LOGIN_VERSION: u8 = 1;
    const CONNECT: u8 = 1;
    const IPV4: u8 = 1;
    const DOMAIN: u8 = 3;
    const IPV6: u8 = 4;

    pub const REPLY_HEAD: usize = 5;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Address {
        V4([u8; 4]),
        V6([u8; 16]),
    }

    pub fn greeting(login: bool) -> Vec<u8> {
        if login {
            vec![VERSION, 2, NO_LOGIN, LOGIN]
        } else {
            vec![VERSION, 1, NO_LOGIN]
        }
    }

    pub fn method(reply: [u8; 2], login: bool) -> Result<bool, i32> {
        match reply {
            [VERSION, NO_LOGIN] => Ok(false),
            [VERSION, LOGIN] if login => Ok(true),
            [VERSION, _] => Err(error::METHOD),
            _ => Err(error::REPLY),
        }
    }

    pub fn login(username: &[u8], password: &[u8]) -> Vec<u8> {
        let mut message = Vec::with_capacity(3 + username.len() + password.len());
        message.push(LOGIN_VERSION);
        message.push(username.len() as u8);
        message.extend_from_slice(username);
        message.push(password.len() as u8);
        message.extend_from_slice(password);
        message
    }

    pub fn logged_in(reply: [u8; 2]) -> Result<(), i32> {
        match reply[1] {
            0 => Ok(()),
            _ => Err(error::AUTH),
        }
    }

    pub fn connect(address: Address, port: u16) -> Vec<u8> {
        let mut message = vec![VERSION, CONNECT, 0];
        match address {
            Address::V4(bytes) => {
                message.push(IPV4);
                message.extend_from_slice(&bytes);
            }
            Address::V6(bytes) => {
                message.push(IPV6);
                message.extend_from_slice(&bytes);
            }
        }
        message.extend_from_slice(&port.to_be_bytes());
        message
    }

    pub fn reply_rest(head: [u8; REPLY_HEAD]) -> Result<usize, i32> {
        if head[0] != VERSION {
            return Err(error::REPLY);
        }
        if head[1] != 0 {
            return Err(error::REFUSED | i32::from(head[1]));
        }
        match head[3] {
            IPV4 => Ok(4 - 1 + 2),
            IPV6 => Ok(16 - 1 + 2),
            DOMAIN => Ok(usize::from(head[4]) + 2),
            _ => Err(error::REPLY),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use core::mem::size_of;

    fn memory() -> Box<[u8; MEMORY_SIZE]> {
        Box::new([0; MEMORY_SIZE])
    }

    fn shared(memory: &mut [u8; MEMORY_SIZE]) -> Shared {
        unsafe { Shared::attach(NonNull::new(memory.as_mut_ptr()).unwrap()) }
    }

    #[test]
    fn fits_one_page() {
        assert!(size_of::<ProxyMemory>() <= MEMORY_SIZE);
        assert_eq!(size_of::<Field>(), 256);
        assert_eq!(size_of::<Request>(), 8 + 3 * 256);
    }

    #[test]
    fn a_fresh_mapping_is_off_and_a_write_turns_it_on() {
        let mut memory = memory();
        let shared = shared(&mut memory);
        assert!(matches!(shared.read(), (0, Setting::Off)));
        let request = Request::new("proxy.example.com", 1080, "user", "secret").unwrap();
        let sequence = shared.write(&request);
        match shared.read() {
            (read, Setting::On(got)) => {
                assert_eq!(read, sequence);
                assert!(*got == request);
                assert_eq!(got.host.bytes(), b"proxy.example.com");
                assert!(got.logs_in());
            }
            _ => panic!("expected the proxy on"),
        }
        let off = shared.write(&Request::OFF);
        assert!(matches!(shared.read(), (read, Setting::Off) if read == off));
    }

    #[test]
    fn another_layout_version_is_unreadable() {
        let mut memory = memory();
        memory[0..4].copy_from_slice(&(VERSION + 1).to_ne_bytes());
        memory[4..8].copy_from_slice(&2u32.to_ne_bytes());
        assert!(matches!(
            shared(&mut memory).read(),
            (2, Setting::Unreadable)
        ));
    }

    #[test]
    fn requests_are_checked() {
        assert!(Request::new("", 1080, "", "").is_err());
        assert!(Request::new("host", 0, "", "").is_err());
        assert!(Request::new("host", 1080, "", "pass").is_err());
        assert!(Request::new(&"h".repeat(256), 1080, "", "").is_err());
        assert!(!Request::new("host", 1080, "", "").unwrap().logs_in());
    }

    #[test]
    fn socks_handshake_messages() {
        assert_eq!(socks::greeting(false), [5, 1, 0]);
        assert_eq!(socks::greeting(true), [5, 2, 0, 2]);
        assert_eq!(socks::method([5, 0], true), Ok(false));
        assert_eq!(socks::method([5, 2], true), Ok(true));
        assert_eq!(socks::method([5, 2], false), Err(error::METHOD));
        assert_eq!(socks::method([5, 0xff], true), Err(error::METHOD));
        assert_eq!(socks::method([4, 0], false), Err(error::REPLY));
        assert_eq!(
            socks::login(b"ab", b"xyz"),
            [1, 2, b'a', b'b', 3, b'x', b'y', b'z']
        );
        assert_eq!(socks::logged_in([1, 0]), Ok(()));
        assert_eq!(socks::logged_in([1, 1]), Err(error::AUTH));
        assert_eq!(
            socks::connect(socks::Address::V4([1, 2, 3, 4]), 43594),
            [5, 1, 0, 1, 1, 2, 3, 4, 0xaa, 0x4a]
        );
        let v6 = socks::connect(socks::Address::V6([0; 16]), 443);
        assert_eq!(v6.len(), 4 + 16 + 2);
        assert_eq!(v6[3], 4);
    }

    #[test]
    fn socks_replies() {
        assert_eq!(socks::reply_rest([5, 0, 0, 1, 9]), Ok(5));
        assert_eq!(socks::reply_rest([5, 0, 0, 4, 9]), Ok(17));
        assert_eq!(socks::reply_rest([5, 0, 0, 3, 7]), Ok(9));
        assert_eq!(socks::reply_rest([5, 5, 0, 1, 0]), Err(error::REFUSED | 5));
        assert_eq!(socks::reply_rest([5, 0, 0, 9, 0]), Err(error::REPLY));
        assert_eq!(socks::reply_rest([1, 0, 0, 1, 0]), Err(error::REPLY));
        assert!(error::describe(error::REFUSED | 5).contains("connection refused"));
    }
}
