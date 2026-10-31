use std::time::Duration;

/// One skill of a game state read: its real level and total experience.
#[derive(Clone)]
pub struct SkillXp {
    pub name: String,
    pub level: i64,
    pub xp: i64,
}

/// The experience a script has gained since its first `track_xp` snapshot.
#[derive(Default)]
pub struct XpTracker {
    first: Vec<SkillXp>,
    latest: Vec<SkillXp>,
    /// The run time when `latest` was read; rates are worked out against it, so they hold still between reads.
    read_at: Duration,
}

pub struct XpRow {
    pub name: String,
    pub level: i64,
    pub gained: i64,
    pub per_hour: Option<i64>,
    pub to_level: Option<Duration>,
}

/// Experience needed for `level`, as in the game.
pub fn xp_for_level(level: i64) -> i64 {
    let points: f64 = (1..level)
        .map(|l| (l as f64 + 300.0 * 2f64.powf(l as f64 / 7.0)).floor())
        .sum();
    (points / 4.0).floor() as i64
}

impl XpTracker {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether no read of the skills has come in yet.
    pub fn is_empty(&self) -> bool {
        self.first.is_empty()
    }

    pub fn update(&mut self, skills: Vec<SkillXp>, at: Duration) {
        if self.first.is_empty() {
            self.first = skills.clone();
        }
        // a skill that shows up later started from its first appearance
        for skill in &skills {
            if !self.first.iter().any(|first| first.name == skill.name) {
                self.first.push(skill.clone());
            }
        }
        self.latest = skills;
        self.read_at = at;
    }

    pub fn total(&self) -> i64 {
        self.latest
            .iter()
            .filter_map(|skill| {
                Some(skill.xp - self.first.iter().find(|first| first.name == skill.name)?.xp)
                    .filter(|&gained| gained > 0)
            })
            .sum()
    }

    /// Skills that gained experience, biggest gain first, with rates over the run up to the last read.
    pub fn rows(&self) -> Vec<XpRow> {
        let hours = self.read_at.as_secs_f64() / 3600.0;
        let mut rows: Vec<XpRow> = self
            .latest
            .iter()
            .filter_map(|skill| {
                let first = self.first.iter().find(|first| first.name == skill.name)?;
                let gained = skill.xp - first.xp;
                if gained <= 0 {
                    return None;
                }
                let per_hour = (hours > 0.0).then(|| (gained as f64 / hours).round() as i64);
                let to_level = per_hour
                    .filter(|&rate| rate > 0 && skill.level < 99)
                    .map(|rate| {
                        let missing = (xp_for_level(skill.level + 1) - skill.xp).max(0) as f64;
                        Duration::from_secs_f64(missing / rate as f64 * 3600.0)
                    });
                Some(XpRow {
                    name: skill.name.clone(),
                    level: skill.level,
                    gained,
                    per_hour,
                    to_level,
                })
            })
            .collect();
        rows.sort_by_key(|row| std::cmp::Reverse(row.gained));
        rows
    }
}

pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

pub fn duration(d: Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(name: &str, level: i64, xp: i64) -> SkillXp {
        SkillXp {
            name: name.into(),
            level,
            xp,
        }
    }

    #[test]
    fn level_table_matches_the_game() {
        assert_eq!(xp_for_level(1), 0);
        assert_eq!(xp_for_level(2), 83);
        assert_eq!(xp_for_level(92), 6_517_253);
        assert_eq!(xp_for_level(99), 13_034_431);
    }

    #[test]
    fn tracks_gains_from_the_first_snapshot() {
        let mut tracker = XpTracker::default();
        assert!(tracker.is_empty());
        tracker.update(
            vec![skill("Fishing", 70, 737_627), skill("Attack", 50, 100)],
            Duration::ZERO,
        );
        assert!(tracker.rows().is_empty());
        tracker.update(
            vec![skill("Fishing", 70, 747_627), skill("Attack", 50, 100)],
            Duration::from_secs(3600),
        );
        let rows = tracker.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].gained, rows[0].per_hour), (10_000, Some(10_000)));
        assert_eq!(tracker.total(), 10_000);
        let missing = xp_for_level(71) - 747_627;
        assert_eq!(
            rows[0].to_level.unwrap().as_secs(),
            (missing as f64 / 10_000.0 * 3600.0) as u64
        );
        tracker.reset();
        assert!(tracker.is_empty());
    }

    #[test]
    fn formats() {
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(-5), "-5");
        assert_eq!(duration(Duration::from_secs(3725)), "1:02:05");
    }
}
