use std::time::{Duration, Instant};

use eframe::egui;
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
    /// Its position in the config, which skipping it names it by.
    pub index: u32,
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
                    index: item.index,
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

const MINUTE_MS: f64 = 60_000.0;
const HOUR_MS: f64 = 3_600_000.0;

/// A number that can be left on Auto, in the units of `scale` (milliseconds per shown unit, or 0.01 for percents).
fn auto_number(
    ui: &mut egui::Ui,
    value: &mut Option<f64>,
    scale: f64,
    manual: f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let mut auto = value.is_none();
        if ui
            .checkbox(&mut auto, "Auto")
            .on_hover_text("Picked from the account's biometrics")
            .changed()
        {
            *value = if auto { None } else { Some(manual) };
            changed = true;
        }
        if let Some(v) = value {
            let mut shown = *v / scale;
            if ui
                .add(egui::DragValue::new(&mut shown).range(range).suffix(suffix))
                .changed()
            {
                *v = shown * scale;
                changed = true;
            }
        }
    });
    changed
}

fn number(
    ui: &mut egui::Ui,
    value: &mut f64,
    scale: f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
) -> bool {
    let mut shown = *value / scale;
    let changed = ui
        .add(egui::DragValue::new(&mut shown).range(range).suffix(suffix))
        .changed();
    if changed {
        *value = shown * scale;
    }
    changed
}

const ROW_HEIGHT: f32 = 28.0;
const ACTIONS_WIDTH: f32 = 70.0;
const VISIBLE_ROWS: usize = 5;

/// What breaks and sleeps are, above the editors.
pub fn header(ui: &mut egui::Ui) {
    ui.add_space(6.0);
    ui.label("Breaks are short pauses taken every so often; sleeps are long ones at night.");
}

/// The section's own switch. Returns whether anything changed.
fn section_bar(ui: &mut egui::Ui, enabled: &mut bool, what: &str) -> bool {
    let changed = ui
        .checkbox(enabled, "Enabled")
        .on_hover_text(format!("Off means no {what} are taken"))
        .changed();
    ui.add_space(4.0);
    changed
}

/// The Add button on the left and the Reset on the right, with room from the list above. Returns (add, reset).
fn footer(ui: &mut egui::Ui, add: &str, what: &str) -> (bool, bool) {
    ui.add_space(8.0);
    let mut result = (false, false);
    ui.horizontal(|ui| {
        result.0 = ui.button(add).clicked();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            result.1 = ui
                .button("Reset")
                .on_hover_text(format!("Put the {what} back to the defaults"))
                .clicked();
        });
    });
    result
}

/// A striped list with `heads` for columns and a last one for the buttons.
fn list(ui: &mut egui::Ui, id: &str, heads: [&str; 4], rows: usize, mut row: impl FnMut(&mut egui_extras::TableRow, usize)) {
    // Up to `VISIBLE_ROWS` rows show, the rest scroll; the list always takes that much room so the panel keeps its height.
    let body = VISIBLE_ROWS as f32 * (ROW_HEIGHT + ui.spacing().item_spacing.y);
    egui_extras::TableBuilder::new(ui)
        .id_salt(id)
        .striped(true)
        .vscroll(true)
        .auto_shrink([false, false])
        .min_scrolled_height(body)
        .max_scroll_height(body)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(egui_extras::Column::remainder().clip(true))
        .column(egui_extras::Column::remainder().clip(true))
        .column(egui_extras::Column::remainder().clip(true))
        .column(egui_extras::Column::remainder().clip(true))
        .column(egui_extras::Column::exact(ACTIONS_WIDTH))
        .header(ROW_HEIGHT, |mut header| {
            for head in heads.into_iter().chain([""]) {
                header.col(|ui| {
                    ui.strong(head);
                });
            }
        })
        .body(|body| {
            body.rows(ROW_HEIGHT, rows, |mut r| {
                let index = r.index();
                row(&mut r, index);
            });
        });
}

/// The editor of the breaks. Returns whether anything changed.
pub fn breaks_editor(ui: &mut egui::Ui, config: &mut injector::antiban::Config) -> bool {
    use injector::antiban::{Break, Config};
    let mut changed = section_bar(ui, &mut config.breaks_enabled, "breaks");
    ui.add_enabled_ui(config.breaks_enabled, |ui| {
        let mut remove = None;
        let breaks = &mut config.breaks;
        let count = breaks.len();
        list(ui, "breaks_edit", ["Every", "Length", "Randomness", "Log out"], count, |row, i| {
            let b = &mut breaks[i];
            row.col(|ui| changed |= number(ui, &mut b.interval, MINUTE_MS, 1.0..=1440.0, " min"));
            row.col(|ui| changed |= number(ui, &mut b.length, MINUTE_MS, 1.0..=1440.0, " min"));
            row.col(|ui| changed |= auto_number(ui, &mut b.randomness, 0.01, 0.33, 0.0..=100.0, "%"));
            row.col(|ui| changed |= auto_number(ui, &mut b.logout_chance, 0.01, 0.05, 0.0..=100.0, "%"));
            row.col(|ui| {
                if ui.button("Remove").clicked() {
                    remove = Some(i);
                }
            });
        });
        if let Some(i) = remove {
            config.breaks.remove(i);
            changed = true;
        }
        let (add, reset) = footer(ui, "Add break", "breaks");
        if reset {
            config.breaks = Config::default().breaks;
            changed = true;
        }
        if add {
            config.breaks.push(Break {
                interval: 60.0 * MINUTE_MS,
                length: 5.0 * MINUTE_MS,
                randomness: None,
                logout_chance: None,
            });
            changed = true;
        }
    });
    changed
}

/// The editor of the sleeps. Returns whether anything changed.
pub fn sleeps_editor(ui: &mut egui::Ui, config: &mut injector::antiban::Config) -> bool {
    use injector::antiban::{Config, Sleep};
    let mut changed = section_bar(ui, &mut config.sleeps_enabled, "sleeps");
    ui.add_enabled_ui(config.sleeps_enabled, |ui| {
        let mut remove = None;
        let sleeps = &mut config.sleeps;
        let count = sleeps.len();
        list(ui, "sleeps_edit", ["At", "Length", "Randomness", "Log out"], count, |row, i| {
            let s = &mut sleeps[i];
            row.col(|ui| {
                let mut auto = s.time.is_none();
                if ui
                    .checkbox(&mut auto, "Auto")
                    .on_hover_text("The account's usual bedtime")
                    .changed()
                {
                    s.time = if auto { None } else { Some("23:00".into()) };
                    changed = true;
                }
                if let Some(time) = &mut s.time {
                    let bad = injector::antiban::parse_time(time).is_none();
                    let mut edit = egui::TextEdit::singleline(time).desired_width(60.0);
                    if bad {
                        edit = edit.text_color(egui::Color32::LIGHT_RED);
                    }
                    changed |= ui.add(edit).on_hover_text("HH:MM or HH:MM:SS").changed();
                }
            });
            row.col(|ui| changed |= auto_number(ui, &mut s.length, HOUR_MS, 8.0, 0.1..=24.0, " h"));
            row.col(|ui| changed |= auto_number(ui, &mut s.randomness, 0.01, 0.1, 0.0..=100.0, "%"));
            row.col(|ui| changed |= auto_number(ui, &mut s.logout_chance, 0.01, 0.8, 0.0..=100.0, "%"));
            row.col(|ui| {
                if ui.button("Remove").clicked() {
                    remove = Some(i);
                }
            });
        });
        if let Some(i) = remove {
            config.sleeps.remove(i);
            changed = true;
        }
        let (add, reset) = footer(ui, "Add sleep", "sleeps");
        if reset {
            config.sleeps = Config::default().sleeps;
            changed = true;
        }
        if add {
            config.sleeps.push(Sleep::default());
            changed = true;
        }
    });
    changed
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
            index: 0,
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
