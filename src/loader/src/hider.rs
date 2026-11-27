use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use injector::hide;

pub const POLL_EVERY: Duration = Duration::from_secs(2);

pub struct Category {
    pub bit: u64,
    pub label: &'static str,
}

const fn category(bit: u64, label: &'static str) -> Category {
    Category { bit, label }
}

pub const GROUPS: [(&str, &[Category]); 3] = [
    (
        "Players",
        &[
            category(hide::OTHERS, "Others"),
            category(hide::OTHERS_2D, "Others' 2D"),
            category(hide::FRIENDS, "Friends"),
            category(hide::FRIENDS_CHAT, "Friends chat"),
            category(hide::CLAN, "Clan"),
            category(hide::IGNORED, "Ignored"),
            category(hide::LOCAL, "Yourself"),
            category(hide::LOCAL_2D, "Your 2D"),
            category(hide::ATTACKERS, "Attackers"),
        ],
    ),
    (
        "NPCs",
        &[
            category(hide::NPCS, "NPCs"),
            category(hide::NPCS_2D, "NPCs' 2D"),
            category(hide::PETS, "Others' pets"),
            category(hide::THRALLS, "Thralls"),
            category(hide::DEAD_NPCS, "Dead NPCs"),
            category(hide::RANDOM_EVENTS, "Others' random events"),
        ],
    ),
    (
        "World",
        &[
            category(hide::PROJECTILES, "Projectiles"),
            category(hide::SPOT_ANIMS, "Spot animations"),
            category(hide::WALLS, "Walls"),
            category(hide::WALL_DECORATIONS, "Wall decorations"),
            category(hide::GROUND_DECORATIONS, "Floor decorations"),
            category(hide::OBJECTS, "Objects"),
            category(hide::GROUND_ITEMS, "Ground items"),
        ],
    ),
];

pub fn labels(mask: u64) -> Vec<&'static str> {
    GROUPS
        .iter()
        .flat_map(|(_, categories)| categories.iter())
        .filter(|category| mask & category.bit != 0)
        .map(|category| category.label)
        .collect()
}

#[derive(Default)]
pub struct Hiders {
    masks: HashMap<u32, u64>,
    asking: bool,
    last_poll: Option<Instant>,
    changes: u64,
}

impl Hiders {
    pub fn mask(&self, pid: u32) -> Option<u64> {
        self.masks.get(&pid).copied()
    }

    pub fn due(&self, now: Instant) -> Option<Duration> {
        if self.asking {
            return None;
        }
        let next = self.last_poll.map_or(now, |last| last + POLL_EVERY);
        Some(next.saturating_duration_since(now))
    }

    pub fn poll(&mut self, now: Instant) -> u64 {
        self.asking = true;
        self.last_poll = Some(now);
        self.changes
    }

    pub fn change(&mut self) -> u64 {
        self.changes += 1;
        self.changes
    }

    pub fn answered(&mut self, pid: u32, asked_at: u64, polled: bool, mask: Option<u64>) -> u64 {
        if polled {
            self.asking = false;
        }
        if asked_at != self.changes {
            return 0;
        }
        match mask {
            Some(mask) => self.masks.insert(pid, mask).unwrap_or(0) & !mask,
            None => {
                self.masks.remove(&pid);
                0
            }
        }
    }

    pub fn forget(&mut self, pid: u32) {
        self.masks.remove(&pid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_has_one_box() {
        let mut seen = 0;
        for (_, categories) in GROUPS {
            for category in categories {
                assert_eq!(seen & category.bit, 0, "{} twice", category.label);
                seen |= category.bit;
            }
        }
        assert_eq!(seen, hide::ALL);
    }

    #[test]
    fn answers_report_the_boxes_that_turned_off() {
        let mut hiders = Hiders::default();
        let set = hiders.change();
        assert_eq!(hiders.answered(7, set, false, Some(hide::NPCS | hide::WALLS)), 0);
        let polled = hiders.poll(Instant::now());
        assert_eq!(hiders.answered(7, polled, true, Some(hide::WALLS)), hide::NPCS);
        assert_eq!(labels(hide::NPCS), ["NPCs"]);
        assert_eq!(hiders.answered(7, polled, true, None), 0);
        assert_eq!(hiders.mask(7), None);
    }

    #[test]
    fn a_poll_asked_before_a_change_is_dropped() {
        let mut hiders = Hiders::default();
        let polled = hiders.poll(Instant::now());
        let set = hiders.change();
        assert_eq!(hiders.answered(7, set, false, Some(hide::NPCS)), 0);
        assert_eq!(hiders.answered(7, polled, true, Some(0)), 0);
        assert_eq!(hiders.mask(7), Some(hide::NPCS));
        assert_eq!(hiders.due(Instant::now() + POLL_EVERY), Some(Duration::ZERO));
    }

    #[test]
    fn polls_wait_for_the_last_answer() {
        let mut hiders = Hiders::default();
        let now = Instant::now();
        assert_eq!(hiders.due(now), Some(Duration::ZERO));
        let polled = hiders.poll(now);
        assert_eq!(hiders.due(now), None);
        hiders.answered(7, polled, true, Some(0));
        assert_eq!(hiders.due(now), Some(POLL_EVERY));
    }
}
