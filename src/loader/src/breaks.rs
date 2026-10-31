use std::time::{Duration, Instant};

use injector::form::BreakPlan;

/// The breaks and sleeps a script last reported, counted down here between reports.
#[derive(Default)]
pub struct BreaksTracker {
    plan: Option<(BreakPlan, Instant)>,
}

/// One scheduled break or sleep, as of a moment.
pub struct BreakRow {
    pub remaining: Duration,
    pub length: Duration,
    /// Seconds since local midnight, for a sleep.
    pub at: Option<u32>,
}

pub struct ActiveRow {
    pub kind: String,
    pub remaining: Duration,
}

pub struct BreaksView {
    pub breaks: Vec<BreakRow>,
    pub sleeps: Vec<BreakRow>,
    pub active: Option<ActiveRow>,
}

fn ms(value: f64) -> Duration {
    Duration::from_millis(value.max(0.0).round() as u64)
}

impl BreaksTracker {
    pub fn reset(&mut self) {
        self.plan = None;
    }

    pub fn update(&mut self, plan: BreakPlan, at: Instant) {
        self.plan = Some((plan, at));
    }

    /// When the last report came in.
    pub fn received(&self) -> Option<Instant> {
        self.plan.as_ref().map(|(_, at)| *at)
    }

    /// The schedule as of `at`, soonest first. While something is being taken the antiban clock is paused, so the rest stand still.
    pub fn view(&self, at: Instant) -> Option<BreaksView> {
        let (plan, received) = self.plan.as_ref()?;
        let passed = at.saturating_duration_since(*received);
        let counting = if plan.active.is_some() {
            Duration::ZERO
        } else {
            passed
        };
        let rows = |list: &[injector::form::PlannedBreak]| {
            let mut rows: Vec<BreakRow> = list
                .iter()
                .map(|item| BreakRow {
                    remaining: ms(item.remaining).saturating_sub(counting),
                    length: ms(item.length),
                    at: item.at.map(|secs| secs.rem_euclid(86400.0) as u32),
                })
                .collect();
            rows.sort_by_key(|row| row.remaining);
            rows
        };
        Some(BreaksView {
            breaks: rows(&plan.breaks),
            sleeps: rows(&plan.sleeps),
            active: plan.active.as_ref().map(|a| ActiveRow {
                kind: a.kind.clone(),
                remaining: ms(a.remaining).saturating_sub(passed),
            }),
        })
    }
}

pub fn clock(secs: u32) -> String {
    format!("{:02}:{:02}", secs / 3600 % 24, secs / 60 % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use injector::form::{ActiveBreak, PlannedBreak};

    fn planned(remaining: f64, at: Option<f64>) -> PlannedBreak {
        PlannedBreak {
            remaining,
            length: 60_000.0,
            at,
        }
    }

    #[test]
    fn counts_down_between_reports() {
        let start = Instant::now();
        let mut tracker = BreaksTracker::default();
        assert!(tracker.view(start).is_none());
        let plan = BreakPlan {
            breaks: vec![planned(90_000.0, None), planned(10_000.0, None)],
            sleeps: vec![planned(5_000.0, Some(90_000.0))],
            active: None,
        };
        tracker.update(plan, start);
        let view = tracker.view(start + Duration::from_secs(4)).unwrap();
        assert_eq!(view.breaks[0].remaining, Duration::from_secs(6));
        assert_eq!(view.breaks[1].remaining, Duration::from_secs(86));
        assert_eq!(view.sleeps[0].remaining, Duration::from_secs(1));
        assert_eq!(view.sleeps[0].at, Some(3600 * 25 % 86400));
        assert_eq!(
            tracker
                .view(start + Duration::from_secs(60))
                .unwrap()
                .sleeps[0]
                .remaining,
            Duration::ZERO
        );
    }

    #[test]
    fn a_break_in_progress_freezes_the_rest() {
        let start = Instant::now();
        let mut tracker = BreaksTracker::default();
        let active = ActiveBreak {
            kind: "break".into(),
            remaining: 30_000.0,
        };
        tracker.update(
            BreakPlan {
                breaks: vec![planned(10_000.0, None)],
                sleeps: vec![],
                active: Some(active),
            },
            start,
        );
        let view = tracker.view(start + Duration::from_secs(5)).unwrap();
        assert_eq!(view.breaks[0].remaining, Duration::from_secs(10));
        assert_eq!(view.active.unwrap().remaining, Duration::from_secs(25));
        tracker.reset();
        assert!(tracker.view(start).is_none());
    }

    #[test]
    fn formats_the_time_of_day() {
        assert_eq!(clock(3600 * 3 + 60 * 7 + 59), "03:07");
    }
}
