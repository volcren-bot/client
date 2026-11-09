use std::time::{Duration, Instant};

use injector::{Packet, PacketPage};

use crate::{
    log_view::{Filtered, Log, Poller, View},
    packet_defs::{self, Table},
};

pub const KEEP: usize = 5000;
pub const POLL_EVERY: Duration = Duration::from_millis(250);

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

pub const UNKNOWN: &str = "UNKNOWN";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Filter {
    only: Vec<Key>,
    hide: Vec<Key>,
    only_unknown: bool,
    hide_unknown: bool,
}

pub fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|word| !word.is_empty())
}

impl Filter {
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
                Err(_) => list.extend(table.by_name(word).map(|(sent, opcode)| Key {
                    sent: Some(sent),
                    opcode,
                })),
            }
        }
        filter
    }

    pub fn allows(&self, sent: bool, opcode: u16, named: bool) -> bool {
        let wanted = (self.only.is_empty() && !self.only_unknown)
            || self.only.iter().any(|k| k.matches(sent, opcode))
            || (self.only_unknown && !named);
        wanted
            && !self.hide.iter().any(|k| k.matches(sent, opcode))
            && !(self.hide_unknown && !named)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameState {
    Off,
    Only,
    Hidden,
}

impl NameState {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Only,
            Self::Only => Self::Hidden,
            Self::Hidden => Self::Off,
        }
    }
}

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

pub fn with_name(text: &str, name: &str, state: NameState) -> String {
    let mut kept: Vec<String> = words(text)
        .filter(|w| !w.trim_start_matches('!').eq_ignore_ascii_case(name))
        .map(str::to_owned)
        .collect();
    match state {
        NameState::Off => {}
        NameState::Only => kept.push(name.to_owned()),
        NameState::Hidden => kept.push(format!("!{name}")),
    }
    kept.join(", ")
}

pub fn named_words(text: &str, table: &Table) -> usize {
    words(text)
        .map(|w| w.trim_start_matches('!'))
        .filter(|w| {
            w.parse::<u16>().is_err()
                && (w.eq_ignore_ascii_case(UNKNOWN) || table.by_name(w).next().is_some())
        })
        .count()
}

pub struct Monitor {
    pub capture: bool,
    pub received: bool,
    pub sent: bool,
    pub filter: String,
    pub name_search: String,
    pub dropped: u64,
    pub error: Option<String>,
    pub selected: Option<u64>,
    pub follow: bool,
    layouts: Option<Table>,
    layouts_pid: Option<u32>,
    layouts_retry_at: Option<Instant>,
    packets: Log<Packet>,
    shown: Filtered<(bool, bool, String, bool)>,
    poller: Poller,
    next: u64,
}

impl Default for Monitor {
    fn default() -> Self {
        Self {
            capture: false,
            received: true,
            sent: true,
            filter: String::new(),
            name_search: String::new(),
            dropped: 0,
            error: None,
            selected: None,
            follow: true,
            layouts: None,
            layouts_pid: None,
            layouts_retry_at: None,
            packets: Log::default(),
            shown: Filtered::default(),
            poller: Poller::default(),
            next: 0,
        }
    }
}

impl Monitor {
    pub fn table(&self) -> &Table {
        self.layouts
            .as_ref()
            .unwrap_or_else(|| packet_defs::shipped())
    }

    pub fn wants_layouts(&self, pid: u32, now: Instant) -> bool {
        self.capture
            && self.layouts.is_none()
            && (self.layouts_pid != Some(pid) || self.layouts_retry_at.is_some_and(|at| now >= at))
    }

    pub fn start_layouts(&mut self, pid: u32) {
        if self.layouts_pid != Some(pid) {
            self.layouts = None;
        }
        self.layouts_pid = Some(pid);
        self.layouts_retry_at = Some(Instant::now() + Duration::from_secs(5));
    }

    pub fn finish_layouts(
        &mut self,
        pid: u32,
        result: Result<Option<Vec<injector::PacketLayout>>, String>,
    ) {
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

    pub fn due(&self, pid: u32, now: Instant) -> bool {
        self.poller.due(self.capture, pid, now)
    }

    pub fn start(&mut self, pid: u32) -> u64 {
        if self.poller.start(pid) {
            self.next = 0;
            self.layouts = None;
            self.clear();
        }
        self.next
    }

    pub fn finish(&mut self, pid: u32, now: Instant, result: Result<PacketPage, String>) {
        if !self.poller.finish(pid, Some(now + POLL_EVERY)) {
            return;
        }
        match result {
            Ok(page) => {
                self.error = None;
                self.next = page.next;
                self.dropped += page.dropped;
                if let Some(first) = page.packets.first() {
                    self.packets
                        .restart_if_behind(first, |last, first| first.sequence <= last.sequence);
                }
                self.packets.extend(page.packets);
                self.packets.keep(KEEP);
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn visible(&self) -> View<'_, Packet, (bool, bool, String, bool)> {
        let filter = Filter::parse(&self.filter, self.table());
        let (sent, received) = (self.sent, self.received);
        let table = self.table();
        self.shown.view(
            &self.packets,
            (sent, received, self.filter.clone(), self.layouts.is_some()),
            |p| {
                let named = table
                    .lookup(p.sent, p.opcode)
                    .is_some_and(|d| !d.name.is_empty());
                (if p.sent { sent } else { received }) && filter.allows(p.sent, p.opcode, named)
            },
        )
    }
}

impl Monitor {
    pub fn selected_packet(&self) -> Option<&Packet> {
        let sequence = self.selected?;
        self.packets.iter().rev().find(|p| p.sequence == sequence)
    }
}

pub fn payload_bytes(packet: &Packet) -> Vec<u8> {
    packet
        .payload
        .as_bytes()
        .chunks(2)
        .filter_map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

pub fn hex_dump(bytes: &[u8]) -> Vec<String> {
    bytes
        .chunks(16)
        .enumerate()
        .map(|(row, chunk)| {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            let ascii: String = chunk
                .iter()
                .map(|&b| {
                    if (0x20..0x7f).contains(&b) {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            format!("{:04x}  {:<47}  {ascii}", row * 16, hex.join(" "))
        })
        .collect()
}

pub fn leading_values(bytes: &[u8]) -> Vec<(&'static str, String)> {
    let mut values = Vec::new();
    if let Some(&b) = bytes.first() {
        values.push(("byte", format!("{b} ({})", b as i8)));
    }
    if let Some(b) = bytes.first_chunk() {
        values.push(("short", u16::from_be_bytes(*b).to_string()));
    }
    if let Some(b) = bytes.first_chunk() {
        values.push(("int", i32::from_be_bytes(*b).to_string()));
    }
    values
}

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
        Packet {
            sequence,
            time_ms: sequence * 10,
            sent,
            opcode,
            length,
            payload: payload.into(),
        }
    }

    fn page(packets: Vec<Packet>, dropped: u64) -> PacketPage {
        PacketPage {
            next: packets.last().map_or(0, |p| p.sequence + 1),
            packets,
            dropped,
        }
    }

    #[test]
    fn filter_text_selects_or_hides_opcodes() {
        let table = packet_defs::shipped();
        let parse = |text: &str| Filter::parse(text, table);
        assert!(parse("").allows(false, 40, true));
        let only = parse("40, 2");
        assert!(
            only.allows(false, 40, true)
                && only.allows(true, 2, true)
                && !only.allows(false, 3, true)
        );
        let hide = parse("!40 !2");
        assert!(
            !hide.allows(false, 40, true)
                && !hide.allows(false, 2, true)
                && hide.allows(false, 3, true)
        );
        assert!(parse("abc").allows(false, 40, true));
        assert!(!parse("40 !40").allows(false, 40, true));
    }

    #[test]
    fn names_filter_one_direction_in_any_case() {
        let table = packet_defs::shipped();
        let tick = Filter::parse("tick", table);
        assert!(
            tick.allows(false, 2, true)
                && !tick.allows(false, 40, true)
                && !tick.allows(true, 2, true)
        );
        let hide = Filter::parse("!NO_TIMEOUT", table);
        assert!(!hide.allows(true, 75, true) && hide.allows(false, 75, true));
        let mixed = Filter::parse("VARP_LARGE, 3", table);
        assert!(
            mixed.allows(false, 40, true)
                && mixed.allows(false, 3, true)
                && !mixed.allows(false, 2, true)
        );
    }

    #[test]
    fn unknown_stands_for_the_packets_without_a_name() {
        let table = packet_defs::shipped();
        let only = Filter::parse("UNKNOWN", table);
        assert!(only.allows(false, 9, false) && !only.allows(false, 40, true));
        let hidden = Filter::parse("!unknown", table);
        assert!(!hidden.allows(false, 9, false) && hidden.allows(false, 40, true));
        let both = Filter::parse("UNKNOWN, TICK", table);
        assert!(
            both.allows(false, 9, false)
                && both.allows(false, 2, true)
                && !both.allows(false, 40, true)
        );
        assert_eq!(named_words("UNKNOWN, !TICK, 5", table), 2);
    }

    #[test]
    fn the_names_menu_edits_the_filter_text_in_place() {
        assert_eq!(
            name_state("40, VARP_LARGE, !TICK", "varp_large"),
            NameState::Only
        );
        assert_eq!(
            name_state("40, VARP_LARGE, !TICK", "tick"),
            NameState::Hidden
        );
        assert_eq!(name_state("40", "TICK"), NameState::Off);
        assert_eq!(with_name("40", "TICK", NameState::Only), "40, TICK");
        assert_eq!(
            with_name("40, TICK", "tick", NameState::Hidden),
            "40, !tick"
        );
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
        monitor.finish(
            32,
            now,
            Ok(page(
                vec![packet(0, false, 40, "0102", 2), packet(1, true, 75, "", 0)],
                0,
            )),
        );
        assert!(!monitor.due(32, now));
        assert!(monitor.due(32, now + POLL_EVERY));
        assert_eq!(monitor.start(32), 2);
        assert_eq!(monitor.kept(), 2);
    }

    #[test]
    fn another_client_starts_over() {
        let now = Instant::now();
        let mut monitor = Monitor {
            capture: true,
            ..Monitor::default()
        };
        monitor.start(32);
        monitor.finish(32, now, Ok(page(vec![packet(7, false, 40, "", 0)], 3)));
        assert_eq!((monitor.kept(), monitor.dropped), (1, 3));
        assert_eq!(monitor.start(44), 0);
        assert_eq!((monitor.kept(), monitor.dropped), (0, 0));
        monitor.finish(32, now, Ok(page(vec![packet(8, false, 40, "", 0)], 0)));
        assert_eq!(monitor.kept(), 0);
    }

    #[test]
    fn a_reloaded_agent_replaces_the_old_packets() {
        let now = Instant::now();
        let mut monitor = Monitor {
            capture: true,
            ..Monitor::default()
        };
        monitor.start(32);
        monitor.finish(
            32,
            now,
            Ok(page(
                vec![packet(100, false, 40, "", 0), packet(101, false, 40, "", 0)],
                0,
            )),
        );
        monitor.start(32);
        monitor.finish(32, now, Ok(page(vec![packet(0, true, 75, "", 0)], 0)));
        assert_eq!(
            monitor
                .visible()
                .iter()
                .map(|p| p.opcode)
                .collect::<Vec<_>>(),
            [75]
        );
    }

    #[test]
    fn keeps_only_the_newest_packets() {
        let now = Instant::now();
        let mut monitor = Monitor {
            capture: true,
            ..Monitor::default()
        };
        monitor.start(32);
        let many = (0..KEEP as u64 + 10)
            .map(|i| packet(i, false, 40, "", 0))
            .collect();
        monitor.finish(32, now, Ok(page(many, 0)));
        assert_eq!(monitor.kept(), KEEP);
        assert_eq!(monitor.visible()[0].sequence, 10);
    }

    #[test]
    fn visible_applies_direction_and_filter() {
        let now = Instant::now();
        let mut monitor = Monitor {
            capture: true,
            ..Monitor::default()
        };
        monitor.start(32);
        monitor.finish(
            32,
            now,
            Ok(page(
                vec![
                    packet(0, false, 40, "", 0),
                    packet(1, false, 2, "", 0),
                    packet(2, true, 75, "", 0),
                ],
                0,
            )),
        );
        assert_eq!(monitor.visible().len(), 3);
        monitor.sent = false;
        assert_eq!(monitor.visible().len(), 2);
        monitor.filter = "!2".into();
        assert_eq!(
            monitor
                .visible()
                .iter()
                .map(|p| p.opcode)
                .collect::<Vec<_>>(),
            [40]
        );
        monitor.filter = "!TICK".into();
        assert_eq!(
            monitor
                .visible()
                .iter()
                .map(|p| p.opcode)
                .collect::<Vec<_>>(),
            [40]
        );
    }

    #[test]
    fn errors_are_kept_until_the_next_good_read() {
        let now = Instant::now();
        let mut monitor = Monitor {
            capture: true,
            ..Monitor::default()
        };
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
        let mut monitor = Monitor {
            capture: true,
            ..Monitor::default()
        };
        monitor.start(32);
        monitor.finish(
            32,
            now,
            Ok(page(
                vec![packet(4, false, 40, "", 0), packet(5, true, 75, "", 0)],
                0,
            )),
        );
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
        assert_eq!(
            hex_dump(&bytes),
            ["0000  00 01 02 03 41 42 43                             ....ABC"]
        );
        let values = leading_values(&bytes);
        assert_eq!(values[1].1, "1");
        assert_eq!(values[2].1, "66051");
        assert!(leading_values(&[]).is_empty());
    }

    #[test]
    fn payload_text_spaces_bytes_and_marks_truncation() {
        assert_eq!(
            payload_text(&packet(0, false, 40, "850c0000", 4)),
            "85 0c 00 00"
        );
        assert_eq!(payload_text(&packet(0, false, 40, "850c", 200)), "85 0c …");
        assert_eq!(payload_text(&packet(0, false, 2, "", 0)), "");
    }
}
