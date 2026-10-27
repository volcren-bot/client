use std::time::Duration;

use injector::form::Counter;

/// What the script last said it had counted. Totals are the script's own since it started; the rates come from the run clock.
#[derive(Default)]
pub struct CountersTracker {
    counters: Vec<Counter>,
    /// The run time when the script last reported; rates are worked out against it, so they hold still between reports.
    read_at: Duration,
}

pub struct CounterRow {
    pub label: String,
    pub total: f64,
    pub per_hour: Option<f64>,
    /// "per Games" or "of Games", with the number.
    pub ratio: Option<String>,
}

impl CountersTracker {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn update(&mut self, counters: Vec<Counter>, at: Duration) {
        self.counters = counters;
        self.read_at = at;
    }

    pub fn is_empty(&self) -> bool {
        self.counters.is_empty()
    }

    fn label_of(&self, id: &str) -> Option<(&str, f64)> {
        self.counters.iter().find(|c| c.id == id).map(|c| (c.label.as_str(), c.value))
    }

    /// The rows with rates over the run up to the last report, in the order the script sent them.
    pub fn rows(&self) -> Vec<CounterRow> {
        let hours = self.read_at.as_secs_f64() / 3600.0;
        self.counters
            .iter()
            .map(|c| {
                let per_hour = (c.hourly && hours > 0.0).then(|| c.value / hours);
                let ratio = if let Some(of) = c.percent_of.as_deref() {
                    Some(match self.label_of(of) {
                        Some((label, whole)) if whole > 0.0 => format!("{} of {label}", percent(c.value * 100.0 / whole)),
                        _ => "-".into(),
                    })
                } else {
                    c.per.as_deref().map(|per| match self.label_of(per) {
                        Some((label, by)) if by > 0.0 => format!("{} per {label}", number(c.value / by)),
                        _ => "-".into(),
                    })
                };
                CounterRow { label: c.label.clone(), total: c.value, per_hour, ratio }
            })
            .collect()
    }
}

/// 1234567.8 -> "1,234,568"; one decimal below 100.
pub fn number(n: f64) -> String {
    if n.abs() < 100.0 && n != n.round() {
        return format!("{n:.1}");
    }
    crate::xp::thousands(n.round() as i64)
}

fn percent(n: f64) -> String {
    format!("{}%", number(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counter(id: &str, label: &str, value: f64) -> Counter {
        Counter { id: id.into(), label: label.into(), value, hourly: true, per: None, percent_of: None }
    }

    #[test]
    fn rates_come_from_the_latest_totals_and_the_run_clock() {
        let mut tracker = CountersTracker::default();
        let mut wins = counter("wins", "Wins", 9.0);
        wins.percent_of = Some("games".into());
        let mut points = counter("points", "Points", 9_000.0);
        points.per = Some("games".into());
        points.hourly = false;
        tracker.update(vec![counter("games", "Games", 12.0), wins, points], Duration::from_secs(1800));

        let rows = tracker.rows();
        assert_eq!((rows[0].total, rows[0].per_hour), (12.0, Some(24.0)));
        assert_eq!(rows[1].ratio.as_deref(), Some("75% of Games"));
        assert_eq!(rows[2].per_hour, None);
        assert_eq!(rows[2].ratio.as_deref(), Some("750 per Games"));

        // rates hold still until the next report, which brings new totals and a new clock
        assert_eq!(tracker.rows()[0].per_hour, Some(24.0));
        tracker.update(vec![counter("games", "Games", 24.0)], Duration::from_secs(3600));
        assert_eq!(tracker.rows()[0].per_hour, Some(24.0));
        tracker.update(vec![counter("games", "Games", 24.0)], Duration::ZERO);
        assert_eq!(tracker.rows()[0].per_hour, None);
    }

    #[test]
    fn a_missing_or_zero_denominator_shows_a_dash() {
        let mut tracker = CountersTracker::default();
        let mut wins = counter("wins", "Wins", 1.0);
        wins.percent_of = Some("games".into());
        let mut points = counter("points", "Points", 5.0);
        points.per = Some("nothing".into());
        tracker.update(vec![wins, points, counter("games", "Games", 0.0)], Duration::from_secs(60));
        let rows = tracker.rows();
        assert_eq!(rows[0].ratio.as_deref(), Some("-"));
        assert_eq!(rows[1].ratio.as_deref(), Some("-"));
    }

    #[test]
    fn formats_numbers() {
        assert_eq!(number(1234.6), "1,235");
        assert_eq!(number(7.26), "7.3");
        assert_eq!(number(12.0), "12");
    }
}
