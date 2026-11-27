use alloc::{format, string::String, vec::Vec};
use core::{
    mem::size_of,
    ptr::{addr_of, addr_of_mut, copy_nonoverlapping, read_volatile, write_volatile},
};

use crate::{
    state::{seqlock_read_with, seqlock_write_with},
    PROTOCOL_VERSION,
};

pub fn mapping_name(pid: u32) -> String {
    format!("AGENT_WORLD_{pid}_{PROTOCOL_VERSION}")
}

pub const VIEW: usize = 0;
pub const SCRIPT: usize = 1;
pub const CHANNELS: usize = 2;
const CHANNEL_BYTES: usize = 1 << 20;
const FONT_BYTES: usize = 1 << 17;
const READ_ATTEMPTS: usize = 4;

#[repr(C)]
pub struct Block<const N: usize> {
    sequence: u32,
    writer: u32,
    len: u32,
    data: [u8; N],
}

pub type Channel = Block<CHANNEL_BYTES>;
pub type Font = Block<FONT_BYTES>;

#[repr(C)]
pub struct World {
    font: Font,
    channels: [Channel; CHANNELS],
}

pub const SIZE: usize = size_of::<World>();

impl World {
    pub unsafe fn font(this: *mut Self) -> *mut Font {
        addr_of_mut!((*this).font)
    }

    pub unsafe fn channel(this: *mut Self, channel: usize) -> Option<*mut Channel> {
        (channel < CHANNELS).then(|| addr_of_mut!((*this).channels[channel]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetch {
    Same,
    Busy,
    Changed {
        sequence: u32,
        writer: u32,
        bytes: Vec<u8>,
    },
}

impl<const N: usize> Block<N> {
    pub unsafe fn publish(this: *mut Self, writer: u32, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > N {
            return Err(format!(
                "{} bytes of world shapes don't fit in {N}",
                bytes.len()
            ));
        }
        seqlock_write_with(addr_of_mut!((*this).sequence), this, |this| {
            copy_nonoverlapping(
                bytes.as_ptr(),
                addr_of_mut!((*this).data).cast::<u8>(),
                bytes.len(),
            );
            write_volatile(addr_of_mut!((*this).len), bytes.len() as u32);
            write_volatile(addr_of_mut!((*this).writer), writer);
        });
        Ok(())
    }

    pub unsafe fn sequence(this: *const Self) -> u32 {
        read_volatile(addr_of!((*this).sequence))
    }

    pub unsafe fn fetch(this: *const Self, seen: Option<u32>) -> Fetch {
        let sequence = Self::sequence(this);
        if sequence % 2 == 0 && Some(sequence) == seen {
            return Fetch::Same;
        }
        let read = seqlock_read_with(addr_of!((*this).sequence), READ_ATTEMPTS, || {
            let len = (read_volatile(addr_of!((*this).len)) as usize).min(N);
            let mut bytes = Vec::with_capacity(len);
            copy_nonoverlapping(addr_of!((*this).data).cast::<u8>(), bytes.as_mut_ptr(), len);
            bytes.set_len(len);
            (read_volatile(addr_of!((*this).writer)), bytes)
        });
        match read {
            Some((sequence, _)) if Some(sequence) == seen => Fetch::Same,
            Some((sequence, (writer, bytes))) => Fetch::Changed {
                sequence,
                writer,
                bytes,
            },
            None => Fetch::Busy,
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use alloc::{boxed::Box, vec};

    fn zeroed<T>() -> Box<T> {
        let layout = core::alloc::Layout::new::<T>();
        unsafe { Box::from_raw(std::alloc::alloc_zeroed(layout).cast()) }
    }

    #[test]
    fn a_published_channel_reads_back_once() {
        let mut world = zeroed::<World>();
        let world: *mut World = &mut *world;
        unsafe {
            let channel = World::channel(world, SCRIPT).unwrap();
            assert!(
                matches!(Channel::fetch(channel, None), Fetch::Changed { ref bytes, .. } if bytes.is_empty())
            );
            Channel::publish(channel, 42, &[1, 2, 3]).unwrap();
            let Fetch::Changed {
                sequence,
                writer,
                bytes,
            } = Channel::fetch(channel, None)
            else {
                panic!("expected a change");
            };
            assert_eq!((writer, bytes), (42, vec![1, 2, 3]));
            assert_eq!(Channel::fetch(channel, Some(sequence)), Fetch::Same);
        }
    }

    #[test]
    fn too_many_bytes_are_refused_and_leave_the_channel_alone() {
        let mut world = zeroed::<World>();
        let world: *mut World = &mut *world;
        unsafe {
            let font = World::font(world);
            Font::publish(font, 1, &[9]).unwrap();
            let before = Font::sequence(font);
            assert!(Font::publish(font, 1, &vec![0; FONT_BYTES + 1]).is_err());
            assert_eq!(Font::sequence(font), before);
            assert!(World::channel(world, CHANNELS).is_none());
        }
    }

    #[test]
    fn the_mapping_name_carries_the_protocol() {
        assert_eq!(mapping_name(7), format!("AGENT_WORLD_7_{PROTOCOL_VERSION}"));
    }
}
