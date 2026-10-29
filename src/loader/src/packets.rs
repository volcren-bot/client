//! The packet monitor on the Development page: what the client's packet log returned, kept and filtered for display.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use injector::{Packet, PacketPage};

/// Packets kept for display; the oldest go first.
pub const KEEP: usize = 5000;
/// How long after a read finished the next one starts.
pub const POLL_EVERY: Duration = Duration::from_millis(250);

/// Opcodes to show or hide, from text like `40, 2` (only these) or `!40 !2` (all but these).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Filter {
    only: Vec<u16>,
    hide: Vec<u16>,
}

impl Filter {
    pub fn parse(text: &str) -> Self {
        let mut filter = Self::default();
        for word in text.split(|c: char| c == ',' || c.is_whitespace()).filter(|word| !word.is_empty()) {
            match word.strip_prefix('!') {
                Some(opcode) => filter.hide.extend(opcode.parse::<u16>()),
                None => filter.only.extend(word.parse::<u16>()),
            }
        }
        filter
    }

    pub fn allows(&self, opcode: u16) -> bool {
        (self.only.is_empty() || self.only.contains(&opcode)) && !self.hide.contains(&opcode)
    }
}

pub struct Monitor {
    pub capture: bool,
    pub received: bool,
    pub sent: bool,
    pub filter: String,
    /// Packets lost to the log's ring overwriting them before the loader read them, since the last clear.
    pub dropped: u64,
    pub error: Option<String>,
    /// The packet picked in the list (its sequence number), shown in full in the detail pane.
    pub selected: Option<u64>,
    /// Keep the list scrolled to the newest packet.
    pub follow: bool,
    packets: VecDeque<Packet>,
    /// The client the packets are from, and the sequence number to ask it for next.
    pid: Option<u32>,
    next: u64,
    reading: bool,
    next_poll: Option<Instant>,
}

impl Default for Monitor {
    fn default() -> Self {
        Self { capture: false, received: true, sent: true, filter: String::new(), dropped: 0, error: None, selected: None, follow: true, packets: VecDeque::new(), pid: None, next: 0, reading: false, next_poll: None }
    }
}

impl Monitor {
    pub fn kept(&self) -> usize {
        self.packets.len()
    }

    pub fn clear(&mut self) {
        self.packets.clear();
        self.selected = None;
        self.dropped = 0;
    }

    /// Whether a read of `pid`'s log should start now.
    pub fn due(&self, pid: u32, now: Instant) -> bool {
        self.capture && (!self.reading || self.pid != Some(pid)) && self.next_poll.is_none_or(|at| now >= at)
    }

    /// Marks a read started and says which sequence number to ask for. Another client than last time starts from its oldest packet.
    pub fn start(&mut self, pid: u32) -> u64 {
        if self.pid != Some(pid) {
            self.pid = Some(pid);
            self.next = 0;
            self.clear();
        }
        self.reading = true;
        self.next
    }

    pub fn finish(&mut self, pid: u32, now: Instant, result: Result<PacketPage, String>) {
        self.reading = false;
        self.next_poll = Some(now + POLL_EVERY);
        if self.pid != Some(pid) {
            return;
        }
        match result {
            Ok(page) => {
                self.error = None;
                self.next = page.next;
                self.dropped += page.dropped;
                // sequence numbers going backwards mean the client has a new agent with a new log
                if let (Some(last), Some(first)) = (self.packets.back(), page.packets.first())
                    && first.sequence <= last.sequence
                {
                    self.packets.clear();
                }
                self.packets.extend(page.packets);
                while self.packets.len() > KEEP {
                    self.packets.pop_front();
                }
            }
            Err(error) => self.error = Some(error),
        }
    }

    /// The kept packets that pass the direction boxes and the opcode filter, oldest first.
    pub fn visible(&self) -> Vec<&Packet> {
        let filter = Filter::parse(&self.filter);
        self.packets.iter().filter(|p| (if p.sent { self.sent } else { self.received }) && filter.allows(p.opcode)).collect()
    }
}

/// The selected packet, if it is still kept.
impl Monitor {
    pub fn selected_packet(&self) -> Option<&Packet> {
        let sequence = self.selected?;
        self.packets.iter().rev().find(|p| p.sequence == sequence)
    }
}

/// The payload bytes the log kept.
pub fn payload_bytes(packet: &Packet) -> Vec<u8> {
    packet.payload.as_bytes().chunks(2).filter_map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()).collect()
}

/// The payload as `offset  16 hex bytes  ascii` lines, like a hex editor.
pub fn hex_dump(bytes: &[u8]) -> Vec<String> {
    bytes
        .chunks(16)
        .enumerate()
        .map(|(row, chunk)| {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            let ascii: String = chunk.iter().map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' }).collect();
            format!("{:04x}  {:<47}  {ascii}", row * 16, hex.join(" "))
        })
        .collect()
}

/// The first bytes read as the client's big endian numbers, for spotting ids, coordinates and flags by eye.
pub fn leading_values(bytes: &[u8]) -> Vec<(&'static str, String)> {
    let mut values = Vec::new();
    if let Some(&b) = bytes.first() {
        values.push(("byte", format!("{b} ({})", b as i8)));
    }
    if let Some(b) = bytes.get(..2) {
        values.push(("short", format!("{}", u16::from_be_bytes([b[0], b[1]]))));
    }
    if let Some(b) = bytes.get(..4) {
        values.push(("int", format!("{}", i32::from_be_bytes([b[0], b[1], b[2], b[3]]))));
    }
    values
}

/// The payload hex in bytes separated by spaces, with an ellipsis when the log kept only the start of it.
pub fn payload_text(packet: &Packet) -> String {
    let mut text = String::with_capacity(packet.payload.len() * 3 / 2 + 1);
    for (i, pair) in packet.payload.as_bytes().chunks(2).enumerate() {
        if i > 0 {
            text.push(' ');
        }
        text.push_str(std::str::from_utf8(pair).unwrap_or("??"));
    }
    if (packet.payload.len() / 2) < packet.length as usize {
        text.push_str(" …");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(sequence: u64, sent: bool, opcode: u16, payload: &str, length: u32) -> Packet {
        Packet { sequence, time_ms: sequence * 10, sent, opcode, length, payload: payload.into() }
    }

    fn page(packets: Vec<Packet>, dropped: u64) -> PacketPage {
        PacketPage { next: packets.last().map_or(0, |p| p.sequence + 1), packets, dropped }
    }

    #[test]
    fn filter_text_selects_or_hides_opcodes() {
        assert!(Filter::parse("").allows(40));
        let only = Filter::parse("40, 2");
        assert!(only.allows(40) && only.allows(2) && !only.allows(3));
        let hide = Filter::parse("!40 !2");
        assert!(!hide.allows(40) && !hide.allows(2) && hide.allows(3));
        // text that isn't a number is ignored rather than hiding everything
        assert!(Filter::parse("abc").allows(40));
        assert!(!Filter::parse("40 !40").allows(40));
    }

    #[test]
    fn polls_only_when_capturing_and_idle_and_resumes_where_it_stopped() {
        let now = Instant::now();
        let mut monitor = Monitor::default();
        assert!(!monitor.due(32, now));
        monitor.capture = true;
        assert!(monitor.due(32, now));
        assert_eq!(monitor.start(32), 0);
        assert!(!monitor.due(32, now));
        monitor.finish(32, now, Ok(page(vec![packet(0, false, 40, "0102", 2), packet(1, true, 75, "", 0)], 0)));
        assert!(!monitor.due(32, now));
        assert!(monitor.due(32, now + POLL_EVERY));
        assert_eq!(monitor.start(32), 2);
        assert_eq!(monitor.kept(), 2);
    }

    #[test]
    fn another_client_starts_over() {
        let now = Instant::now();
        let mut monitor = Monitor { capture: true, ..Monitor::default() };
        monitor.start(32);
        monitor.finish(32, now, Ok(page(vec![packet(7, false, 40, "", 0)], 3)));
        assert_eq!((monitor.kept(), monitor.dropped), (1, 3));
        assert_eq!(monitor.start(44), 0);
        assert_eq!((monitor.kept(), monitor.dropped), (0, 0));
        // an answer for the client that was left is not mixed in
        monitor.finish(32, now, Ok(page(vec![packet(8, false, 40, "", 0)], 0)));
        assert_eq!(monitor.kept(), 0);
    }

    #[test]
    fn a_reloaded_agent_replaces_the_old_packets() {
        let now = Instant::now();
        let mut monitor = Monitor { capture: true, ..Monitor::default() };
        monitor.start(32);
        monitor.finish(32, now, Ok(page(vec![packet(100, false, 40, "", 0), packet(101, false, 40, "", 0)], 0)));
        monitor.start(32);
        monitor.finish(32, now, Ok(page(vec![packet(0, true, 75, "", 0)], 0)));
        assert_eq!(monitor.visible().iter().map(|p| p.opcode).collect::<Vec<_>>(), [75]);
    }

    #[test]
    fn keeps_only_the_newest_packets() {
        let now = Instant::now();
        let mut monitor = Monitor { capture: true, ..Monitor::default() };
        monitor.start(32);
        let many = (0..KEEP as u64 + 10).map(|i| packet(i, false, 40, "", 0)).collect();
        monitor.finish(32, now, Ok(page(many, 0)));
        assert_eq!(monitor.kept(), KEEP);
        assert_eq!(monitor.visible()[0].sequence, 10);
    }

    #[test]
    fn visible_applies_direction_and_filter() {
        let now = Instant::now();
        let mut monitor = Monitor { capture: true, ..Monitor::default() };
        monitor.start(32);
        monitor.finish(32, now, Ok(page(vec![packet(0, false, 40, "", 0), packet(1, false, 2, "", 0), packet(2, true, 75, "", 0)], 0)));
        assert_eq!(monitor.visible().len(), 3);
        monitor.sent = false;
        assert_eq!(monitor.visible().len(), 2);
        monitor.filter = "!2".into();
        assert_eq!(monitor.visible().iter().map(|p| p.opcode).collect::<Vec<_>>(), [40]);
    }

    #[test]
    fn errors_are_kept_until_the_next_good_read() {
        let now = Instant::now();
        let mut monitor = Monitor { capture: true, ..Monitor::default() };
        monitor.start(32);
        monitor.finish(32, now, Err("no log".into()));
        assert_eq!(monitor.error.as_deref(), Some("no log"));
        monitor.start(32);
        monitor.finish(32, now, Ok(page(vec![], 0)));
        assert_eq!(monitor.error, None);
    }

    #[test]
    fn selection_follows_the_kept_packets() {
        let now = Instant::now();
        let mut monitor = Monitor { capture: true, ..Monitor::default() };
        monitor.start(32);
        monitor.finish(32, now, Ok(page(vec![packet(4, false, 40, "", 0), packet(5, true, 75, "", 0)], 0)));
        monitor.selected = Some(5);
        assert_eq!(monitor.selected_packet().map(|p| p.opcode), Some(75));
        monitor.clear();
        assert!(monitor.selected_packet().is_none());
    }

    #[test]
    fn dumps_and_reads_payload_bytes() {
        let p = packet(0, false, 40, "00010203414243", 7);
        let bytes = payload_bytes(&p);
        assert_eq!(bytes, [0, 1, 2, 3, 0x41, 0x42, 0x43]);
        assert_eq!(hex_dump(&bytes), ["0000  00 01 02 03 41 42 43                             ....ABC"]);
        let values = leading_values(&bytes);
        assert_eq!(values[1].1, "1");
        assert_eq!(values[2].1, "66051");
        assert!(leading_values(&[]).is_empty());
    }

    #[test]
    fn payload_text_spaces_bytes_and_marks_truncation() {
        assert_eq!(payload_text(&packet(0, false, 40, "850c0000", 4)), "85 0c 00 00");
        assert_eq!(payload_text(&packet(0, false, 40, "850c", 200)), "85 0c …");
        assert_eq!(payload_text(&packet(0, false, 2, "", 0)), "");
    }
}
