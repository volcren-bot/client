use serde::{Deserialize, Serialize};
use ipc::state::{ChatLayout, Layout};

use crate::memory::{Memory, read_string};

const TABLE_BUCKETS: u64 = 0x8;
const TABLE_COUNT: u64 = 0x10;
const NODE_VALUE: u64 = 0x10;
const NODE_NEXT: u64 = 0x18;
const MAX_BUCKETS: u64 = 1 << 16;
const MAX_NODES: usize = 1 << 14;
const CYCLE_MS: i64 = 20;

// A chat history message: `kind` is the client's type, `name` and `sender` who and where, `age`/`time` in ms, `time_utc`/`time_local` the client's "HH:MM" stamps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub uid: i32,
    pub kind: i32,
    pub name: String,
    pub sender: String,
    pub text: String,
    pub cycle: i32,
    pub age: i64,
    pub time: i64,
    pub time_utc: String,
    pub time_local: String,
}

fn stamp(memory: &impl Memory, address: u64) -> String {
    let text = read_string(memory, address).unwrap_or_default();
    text.trim().trim_start_matches('[').trim_end_matches(']').to_owned()
}

fn message(memory: &impl Memory, c: &ChatLayout, uid: i32, m: u64, cycle: i32, now: i64) -> Option<ChatMessage> {
    let added = memory.i32(m + c.message_cycle as u64)?;
    let age = (cycle.wrapping_sub(added) as i64).max(0) * CYCLE_MS;
    Some(ChatMessage {
        uid,
        kind: memory.i32(m + c.kind as u64)?,
        name: read_string(memory, m + c.name as u64)?,
        sender: read_string(memory, m + c.sender as u64)?,
        text: read_string(memory, m + c.text as u64)?,
        cycle: added,
        age,
        time: now - age,
        time_utc: stamp(memory, m + c.time_utc as u64),
        time_local: stamp(memory, m + c.time_local as u64),
    })
}

// Every message the client keeps, oldest first; one freed while read is left out.
pub fn messages(memory: &impl Memory, layout: &Layout, now: i64) -> Option<Vec<ChatMessage>> {
    let c = &layout.chat;
    let table = layout.module_base + c.messages as u64;
    let cycle = memory.i32(layout.module_base + c.cycle as u64)?;
    let count = memory.u64(table + TABLE_COUNT)? & 0xffff_ffff;
    if count == 0 {
        return Some(Vec::new());
    }
    if count > MAX_BUCKETS {
        return None;
    }
    let buckets = memory.ptr(table + TABLE_BUCKETS)?;
    let end = memory.u64(buckets + count * 8)?;
    let mut found = Vec::new();
    let mut visited = 0;
    for bucket in 0..count {
        let mut node = memory.u64(buckets + bucket * 8)?;
        while node != 0 && node != end {
            visited += 1;
            if visited > MAX_NODES {
                return None;
            }
            let uid = memory.i32(node)?;
            if let Some(m) = memory.ptr(node + NODE_VALUE) {
                found.extend(message(memory, c, uid, m, cycle, now));
            }
            node = memory.u64(node + NODE_NEXT)?;
        }
    }
    found.sort_by_key(|m| m.uid);
    Some(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::Fake;

    fn put_long_string(memory: &mut Fake, address: u64, data: u64, text: &str) {
        memory.put(data, text.as_bytes());
        memory.put_u64(address, data);
        memory.put_u64(address + 8, text.len() as u64);
        memory.put_u64(address + 16, 0x8000_0000_0000_0000 | (text.len() as u64 + 1));
    }

    #[test]
    fn reads_messages_oldest_first() {
        let chat = ChatLayout {
            messages: 0x1000,
            cycle: 0x2000,
            message_cycle: 0x8,
            kind: 0x40,
            name: 0x48,
            sender: 0x70,
            text: 0xa0,
            time_utc: 0x28,
            time_local: 0x10,
            reserved: 0,
        };
        let layout = Layout { module_base: 0x10_0000, chat, ..Layout::default() };
        let mut m = Fake::default();
        let table = 0x10_1000;
        m.put_i32(0x10_2000, 1000);
        m.put_u64(table + TABLE_BUCKETS, 0x50_0000);
        m.put_u64(table + TABLE_COUNT, 2);
        // bucket 0: uid 4 -> uid 2, bucket 1: empty, sentinel after the buckets
        m.put_u64(0x50_0000, 0x60_0000);
        m.put_u64(0x50_0008, 0);
        m.put_u64(0x50_0010, 0x6f_0000);
        let nodes = [(0x60_0000u64, 4, 0x70_0000u64, 0x60_1000u64), (0x60_1000, 2, 0x71_0000, 0)];
        for (node, uid, message, next) in nodes {
            m.put_i32(node, uid);
            m.put_u64(node + NODE_VALUE, message);
            m.put_u64(node + NODE_NEXT, next);
        }
        for (message, kind, cycle, name, text) in [(0x70_0000u64, 2, 950, "Player", "hi"), (0x71_0000, 0, 900, "", "Welcome to the game.")] {
            m.put_i32(message + 0x8, cycle);
            m.put_i32(message + 0x40, kind);
            m.put_short_string(message + 0x48, name);
            m.put_short_string(message + 0x70, "");
            m.put_short_string(message + 0x10, "[13:21]");
            m.put_short_string(message + 0x28, "[12:21]");
            if text.len() > 22 {
                put_long_string(&mut m, message + 0xa0, message + 0x1000, text);
            } else {
                m.put_short_string(message + 0xa0, text);
            }
        }

        let found = messages(&m, &layout, 1_000_000).unwrap();
        assert_eq!(found.iter().map(|m| m.uid).collect::<Vec<_>>(), vec![2, 4]);
        assert_eq!(found[0].text, "Welcome to the game.");
        assert_eq!((found[0].age, found[0].time), (2000, 998_000));
        assert_eq!((found[1].kind, found[1].name.as_str(), found[1].text.as_str(), found[1].age), (2, "Player", "hi", 1000));
        assert_eq!((found[1].time_utc.as_str(), found[1].time_local.as_str()), ("12:21", "13:21"));
    }
}
