//! The packet monitor on the Development page: what the client's packet log returned, kept and filtered for display.

use std::time::{Duration, Instant};

use injector::{Packet, PacketPage};

use crate::{
    log_view::{Filtered, Log, View},
    packet_defs::{self, Table},
};

/// Packets kept for display; the oldest go first.
pub const KEEP: usize = 5000;
/// How long after a read finished the next one starts.
pub const POLL_EVERY: Duration = Duration::from_millis(250);

/// A packet to show or hide: an opcode in one direction (what a name stands for), or typed as a number, in both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    sent: Option<bool>,
    opcode: u16,
}

impl Key {
    fn matches(&self, sent: bool, opcode: u16) -> bool {
        self.opcode == opcode && self.sent.is_none_or(|s| s == sent)
    }
}

/// The word that stands for every packet without a name.
pub const UNKNOWN: &str = "UNKNOWN";

/// Packets to show or hide, from text like `40, VARP_LARGE` (only these) or `!40 !PLAYER_INFO` (all but these): opcodes or names.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Filter {
    only: Vec<Key>,
    hide: Vec<Key>,
    only_unknown: bool,
    hide_unknown: bool,
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c == ',' || c.is_whitespace()).filter(|word| !word.is_empty())
}

impl Filter {
    /// Names are looked up in `table`; a word that is neither a number nor a known name is ignored.
    pub fn parse(text: &str, table: &Table) -> Self {
        let mut filter = Self::default();
        for word in words(text) {
            let (list, unknown, word) = match word.strip_prefix('!') {
                Some(word) => (&mut filter.hide, &mut filter.hide_unknown, word),
                None => (&mut filter.only, &mut filter.only_unknown, word),
            };
            if word.eq_ignore_ascii_case(UNKNOWN) {
                *unknown = true;
                continue;
            }
            match word.parse::<u16>() {
                Ok(opcode) => list.push(Key { sent: None, opcode }),
                Err(_) => list.extend(table.by_name(word).map(|(sent, opcode)| Key { sent: Some(sent), opcode })),
            }
        }
        filter
    }

    /// `named` is whether the table has a name for the packet.
    pub fn allows(&self, sent: bool, opcode: u16, named: bool) -> bool {
        let wanted = (self.only.is_empty() && !self.only_unknown) || self.only.iter().any(|k| k.matches(sent, opcode)) || (self.only_unknown && !named);
        wanted && !self.hide.iter().any(|k| k.matches(sent, opcode)) && !(self.hide_unknown && !named)
    }
}

/// What the filter text does with a named packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameState {
    Off,
    Only,
    Hidden,
}

impl NameState {
    /// The next state when the name is clicked: off, show only, hide, off.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Only,
            Self::Only => Self::Hidden,
            Self::Hidden => Self::Off,
        }
    }
}

/// What `text` does with `name`.
pub fn name_state(text: &str, name: &str) -> NameState {
    for word in words(text) {
        match word.strip_prefix('!') {
            Some(hidden) if hidden.eq_ignore_ascii_case(name) => return NameState::Hidden,
            None if word.eq_ignore_ascii_case(name) => return NameState::Only,
            _ => {}
        }
    }
    NameState::Off
}

/// `text` with `name` in `state`: its word replaced, added at the end or taken out, the rest as it was.
pub fn with_name(text: &str, name: &str, state: NameState) -> String {
    let mut kept: Vec<String> = words(text).filter(|w| !w.trim_start_matches('!').eq_ignore_ascii_case(name)).map(str::to_owned).collect();
    match state {
        NameState::Off => {}
        NameState::Only => kept.push(name.to_owned()),
        NameState::Hidden => kept.push(format!("!{name}")),
    }
    kept.join(", ")
}

/// How many of the filter text's words are names the table knows.
pub fn named_words(text: &str, table: &Table) -> usize {
    words(text)
        .map(|w| w.trim_start_matches('!'))
        .filter(|w| w.parse::<u16>().is_err() && (w.eq_ignore_ascii_case(UNKNOWN) || table.by_name(w).next().is_some()))
        .count()
}

pub struct Monitor {
    pub capture: bool,
    pub received: bool,
    pub sent: bool,
    pub filter: String,
    /// The search box of the names menu.
    pub name_search: String,
    /// Packets lost to the log's ring overwriting them before the loader read them, since the last clear.
    pub dropped: u64,
    pub error: Option<String>,
    /// The packet picked in the list (its sequence number), shown in full in the detail pane.
    pub selected: Option<u64>,
    /// Keep the list scrolled to the newest packet.
    pub follow: bool,
    /// What the client's agent published about its packets, when it has; the shipped table until then.
    layouts: Option<Table>,
    /// The client the layouts were asked of, and when to ask again after an answer without any.
    layouts_pid: Option<u32>,
    layouts_retry_at: Option<Instant>,
    packets: Log<Packet>,
    /// The rows `visible` shows, for the direction boxes and filter text they were picked with.
    shown: Filtered<(bool, bool, String, bool)>,
    /// The client the packets are from, and the sequence number to ask it for next.
    pid: Option<u32>,
    next: u64,
    reading: bool,
    next_poll: Option<Instant>,
}

impl Default for Monitor {
    fn default() -> Self {
        Self { capture: false, received: true, sent: true, filter: String::new(), name_search: String::new(), dropped: 0, error: None, selected: None, follow: true, layouts: None, layouts_pid: None, layouts_retry_at: None, packets: Log::default(), shown: Filtered::default(), pid: None, next: 0, reading: false, next_poll: None }
    }
}

impl Monitor {
    /// The packets the inspector can name and decode: what the client's agent published, else the shipped table.
    pub fn table(&self) -> &Table {
        self.layouts.as_ref().unwrap_or_else(|| packet_defs::shipped())
    }

    /// Whether `pid`'s layouts should be asked for now.
    pub fn wants_layouts(&self, pid: u32, now: Instant) -> bool {
        self.capture && self.layouts.is_none() && (self.layouts_pid != Some(pid) || self.layouts_retry_at.is_some_and(|at| now >= at))
    }

    /// Marks the layouts asked for.
    pub fn start_layouts(&mut self, pid: u32) {
        if self.layouts_pid != Some(pid) {
            self.layouts = None;
        }
        self.layouts_pid = Some(pid);
        self.layouts_retry_at = Some(Instant::now() + Duration::from_secs(5));
    }

    /// An answer for `pid`; one that holds nothing (the agent hasn't published, or can't) is asked again later.
    pub fn finish_layouts(&mut self, pid: u32, result: Result<Option<Vec<injector::PacketLayout>>, String>) {
        if self.layouts_pid != Some(pid) {
            return;
        }
        if let Ok(Some(layouts)) = result {
            self.layouts = Some(Table::from_client(&layouts));
            self.layouts_retry_at = None;
        }
    }

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
            self.layouts = None;
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
                self.packets.keep(KEEP);
            }
            Err(error) => self.error = Some(error),
        }
    }

    /// The kept packets that pass the direction boxes and the opcode filter, oldest first.
    pub fn visible(&self) -> View<'_, Packet, (bool, bool, String, bool)> {
        let filter = Filter::parse(&self.filter, self.table());
        let (sent, received) = (self.sent, self.received);
        // what a name stands for changes when the client's own table arrives
        let table = self.table();
        self.shown.view(&self.packets, (sent, received, self.filter.clone(), self.layouts.is_some()), |p| {
            let named = table.lookup(p.sent, p.opcode).is_some_and(|d| !d.name.is_empty());
            (if p.sent { sent } else { received }) && filter.allows(p.sent, p.opcode, named)
        })
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
        let table = packet_defs::shipped();
        let parse = |text: &str| Filter::parse(text, table);
        assert!(parse("").allows(false, 40, true));
        let only = parse("40, 2");
        assert!(only.allows(false, 40, true) && only.allows(true, 2, true) && !only.allows(false, 3, true));
        let hide = parse("!40 !2");
        assert!(!hide.allows(false, 40, true) && !hide.allows(false, 2, true) && hide.allows(false, 3, true));
        // text that isn't a number or a name is ignored rather than hiding everything
        assert!(parse("abc").allows(false, 40, true));
        assert!(!parse("40 !40").allows(false, 40, true));
    }

    #[test]
    fn names_filter_one_direction_in_any_case() {
        let table = packet_defs::shipped();
        // TICK is received opcode 2; NO_TIMEOUT is sent opcode 75, and a received 75 is another packet
        let tick = Filter::parse("tick", table);
        assert!(tick.allows(false, 2, true) && !tick.allows(false, 40, true) && !tick.allows(true, 2, true));
        let hide = Filter::parse("!NO_TIMEOUT", table);
        assert!(!hide.allows(true, 75, true) && hide.allows(false, 75, true));
        // names and numbers mix
        let mixed = Filter::parse("VARP_LARGE, 3", table);
        assert!(mixed.allows(false, 40, true) && mixed.allows(false, 3, true) && !mixed.allows(false, 2, true));
    }

    #[test]
    fn unknown_stands_for_the_packets_without_a_name() {
        let table = packet_defs::shipped();
        let only = Filter::parse("UNKNOWN", table);
        assert!(only.allows(false, 9, false) && !only.allows(false, 40, true));
        let hidden = Filter::parse("!unknown", table);
        assert!(!hidden.allows(false, 9, false) && hidden.allows(false, 40, true));
        // with a name beside it: both
        let both = Filter::parse("UNKNOWN, TICK", table);
        assert!(both.allows(false, 9, false) && both.allows(false, 2, true) && !both.allows(false, 40, true));
        assert_eq!(named_words("UNKNOWN, !TICK, 5", table), 2);
    }

    #[test]
    fn the_names_menu_edits_the_filter_text_in_place() {
        assert_eq!(name_state("40, VARP_LARGE, !TICK", "varp_large"), NameState::Only);
        assert_eq!(name_state("40, VARP_LARGE, !TICK", "tick"), NameState::Hidden);
        assert_eq!(name_state("40", "TICK"), NameState::Off);
        assert_eq!(with_name("40", "TICK", NameState::Only), "40, TICK");
        assert_eq!(with_name("40, TICK", "tick", NameState::Hidden), "40, !tick");
        assert_eq!(with_name("40, !TICK, 2", "TICK", NameState::Off), "40, 2");
        assert_eq!(NameState::Off.next().next().next(), NameState::Off);
        let table = packet_defs::shipped();
        assert_eq!(named_words("40, TICK, !VARP_LARGE, nonsense", table), 2);
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
        // a name does the same
        monitor.filter = "!TICK".into();
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
