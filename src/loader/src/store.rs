use std::collections::BTreeMap;

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::api::{
    StoreScript,
    models::{credits_label, normalize_share_code, rate_label},
};

const MAX_CONFIRMED: usize = 500;

#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Confirmed(BTreeMap<String, i64>);

impl Confirmed {
    pub fn has(&self, script_id: &str, revision: i64) -> bool {
        self.0.get(script_id) == Some(&revision)
    }

    pub fn confirm(&mut self, script_id: &str, revision: i64) {
        if self.0.len() >= MAX_CONFIRMED
            && !self.0.contains_key(script_id)
            && let Some(first) = self.0.keys().next().cloned()
        {
            self.0.remove(&first);
        }
        self.0.insert(script_id.to_owned(), revision);
    }

    pub fn forget(&mut self, script_id: &str) {
        self.0.remove(script_id);
    }
}

#[derive(Default)]
pub struct Store {
    pub scripts: Vec<StoreScript>,
    pub loading: bool,
    pub loaded: bool,
    pub error: Option<String>,
    pub search: String,
    pub category: String,
    pub community: bool,
    pub code: String,
    pub redeeming: bool,
    pub redeem_result: Option<(bool, String)>,
    pub removing: Option<String>,
}

pub enum Action {
    Refresh,
    Run(Box<StoreScript>),
    Redeem(String),
    RemoveGrant(Box<StoreScript>),
    OpenSite(String),
}

const MIN_DETAILS_WIDTH: f32 = 240.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Placement {
    Beside,
    Stacked,
    Below,
}

impl Placement {
    fn of(room: f32, buttons: usize, button_width: f32) -> Self {
        let row = crate::theme::button_group_width(buttons, button_width);
        let column = crate::theme::button_group_width(1, button_width);
        if room - row >= MIN_DETAILS_WIDTH {
            Self::Beside
        } else if buttons > 1 && room - column >= MIN_DETAILS_WIDTH {
            Self::Stacked
        } else {
            Self::Below
        }
    }
}
const ICON_WEBSITE: &str = "\u{f0ac}";
const ICON_ADD_FUNDS: &str = "\u{f09d}";

pub fn origin_label(script: &StoreScript) -> &'static str {
    if script.community {
        "community"
    } else {
        "official"
    }
}

pub fn price_texts(script: &StoreScript) -> Option<(String, String)> {
    (!script.is_free()).then(|| {
        (
            rate_label(script.price as f64),
            credits_label(script.price as f64 / 60.0),
        )
    })
}

pub fn price_ui(ui: &mut egui::Ui, script: &StoreScript) {
    match price_texts(script) {
        Some((hour, minute)) => {
            crate::theme::credits(ui, egui::RichText::new(hour).strong().size(16.0), "/h");
            crate::theme::credits(ui, egui::RichText::new(minute).small(), "/min");
        }
        None => {
            ui.label(
                egui::RichText::new("Free")
                    .strong()
                    .color(crate::theme::GREEN),
            );
        }
    }
}

impl Store {
    pub fn categories(&self) -> Vec<String> {
        let mut all: Vec<String> = self
            .scripts
            .iter()
            .flat_map(|s| s.categories.iter().cloned())
            .collect();
        all.sort();
        all.dedup();
        all
    }

    pub fn visible(&self) -> impl Iterator<Item = &StoreScript> {
        let search = self.search.trim().to_lowercase();
        self.scripts.iter().filter(move |s| {
            (self.category.is_empty() || s.categories.contains(&self.category))
                && (search.is_empty()
                    || s.title.to_lowercase().contains(&search)
                    || s.description.to_lowercase().contains(&search))
        })
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        me: &crate::api::Me,
        run_block: Option<&str>,
        has_site: bool,
    ) -> Option<Action> {
        let mut action = None;
        let width = ui.available_width();
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search scripts").desired_width((width * 0.4).clamp(100.0, 220.0)));
            egui::ComboBox::from_id_salt("store-category")
                .selected_text(if self.category.is_empty() { "All categories" } else { &self.category })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.category, String::new(), "All categories");
                    for category in self.categories() {
                        ui.selectable_value(&mut self.category, category.clone(), category);
                    }
                });
            if ui.add_enabled(!self.loading, egui::Button::new("Refresh")).clicked() {
                action = Some(Action::Refresh);
            }
            if me.can_browse_community()
                && ui
                    .checkbox(&mut self.community, "Community")
                    .on_hover_text("List every community script you can see. They stay out of the default list.")
                    .changed()
            {
                action = Some(Action::Refresh);
            }
            if self.loading {
                ui.spinner();
            }
        });
        ui.horizontal_wrapped(|ui| {
            let editing = ui.add_enabled(
                !self.redeeming,
                egui::TextEdit::singleline(&mut self.code)
                    .hint_text("Paste a share code")
                    .desired_width((width - 190.0).clamp(120.0, 420.0))
                    .password(false),
            );
            let submitted = editing.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui
                .add_enabled(
                    !self.redeeming && !self.code.trim().is_empty(),
                    egui::Button::new("Redeem"),
                )
                .clicked()
                || submitted)
                && !self.redeeming
            {
                match normalize_share_code(&self.code) {
                    Some(code) => action = Some(Action::Redeem(code)),
                    None => {
                        self.redeem_result = Some((
                            false,
                            "A share code is 64 characters: the digits 0-9 and the letters a-f."
                                .into(),
                        ))
                    }
                }
            }
            if self.redeeming {
                ui.spinner();
            }
            if let Some((ok, message)) = &self.redeem_result {
                let color = if *ok {
                    crate::theme::GREEN
                } else {
                    ui.visuals().error_fg_color
                };
                ui.colored_label(color, message);
            }
        });
        ui.add_space(4.0);
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if self.loaded && self.scripts.is_empty() && self.error.is_none() {
            ui.label("No scripts available.");
        }

        let (mut run, mut remove, mut start_remove, mut cancel_remove) = (None, None, None, false);
        let mut open_site = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for script in self.visible() {
                ui.add_space(8.0);
                crate::theme::card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let blocked = run_block.map(str::to_owned).or_else(|| script.blocked(me.microcredits, me.min_balance_minutes));
                    let add_funds = has_site && script.short_of_funds(me.microcredits, me.min_balance_minutes);
                    let button_width = crate::theme::icon_button_width(ui, &[(ICON_WEBSITE, "Website"), (crate::app::ICON_START, "Run"), (ICON_ADD_FUNDS, "Add funds")]);
                    let buttons = 1 + usize::from(has_site);
                    let full = ui.available_width();
                    let spacing = ui.spacing().item_spacing;
                    let placement = Placement::of(full - spacing.x, buttons, button_width);
                    let vertical = placement == Placement::Stacked;
                    let size = crate::theme::button_group_size(ui, buttons, button_width, vertical);
                    let details_width = if placement == Placement::Below { full } else { (full - size.x - spacing.x).max(0.0) };
                    let top_left = ui.cursor().min;
                    let details = ui
                        .allocate_ui_with_layout(egui::vec2(details_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_width(details_width);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new(&script.title).strong().size(16.0));
                                ui.label(egui::RichText::new(format!("by {}", script.author)).weak());
                                ui.label(egui::RichText::new(&script.stage).small());
                                let tag = egui::RichText::new(origin_label(script)).small();
                                if script.community {
                                    ui.label(tag.color(egui::Color32::from_rgb(0xff, 0x9e, 0x64)))
                                        .on_hover_text("Written by a community member, not reviewed by staff. You're warned before it runs.");
                                } else {
                                    ui.label(tag.color(crate::theme::BLUE)).on_hover_text("Written by a scripter.");
                                }
                                if script.granted {
                                    ui.label(egui::RichText::new("shared with you").small().color(crate::theme::GREEN));
                                }
                            });
                            ui.label(format!("{} online", script.online));
                            price_ui(ui, script);
                            if !script.description.is_empty() {
                                ui.label(egui::RichText::new(&script.description).weak());
                            }
                            if script.granted {
                                ui.horizontal(|ui| {
                                    if self.removing.as_deref() == Some(script.id.as_str()) {
                                        ui.label("Remove it from your account? You'll need a new code to get it back.");
                                        if ui.button("Remove").clicked() {
                                            remove = Some(Box::new(script.clone()));
                                        }
                                        if ui.button("Keep").clicked() {
                                            cancel_remove = true;
                                        }
                                    } else if ui.small_button("Remove from my account").clicked() {
                                        start_remove = Some(script.id.clone());
                                    }
                                });
                            }
                        })
                        .response
                        .rect;
                    let at = match placement {
                        Placement::Below => egui::pos2(top_left.x, details.bottom() + spacing.y),
                        _ => egui::pos2(top_left.x + full - size.x, details.bottom().max(top_left.y + size.y) - size.y),
                    };
                    crate::theme::button_group_at(ui, egui::Rect::from_min_size(at, size), vertical, |ui| {
                            if has_site && crate::theme::icon_button(ui, ICON_WEBSITE, "Website", button_width, true).on_hover_text("Opens the script's page on the website.").clicked() {
                                open_site = Some(format!("/scripts/{}", script.url));
                            }
                            if add_funds {
                                let reason = script.blocked(me.microcredits, me.min_balance_minutes).unwrap_or_default();
                                if crate::theme::icon_button(ui, ICON_ADD_FUNDS, "Add funds", button_width, true).on_hover_text(reason).clicked() {
                                    open_site = Some("/credits".to_owned());
                                }
                                return;
                            }
                            let button = crate::theme::icon_button(ui, crate::app::ICON_START, "Run", button_width, blocked.is_none());
                            let button = match &blocked {
                                Some(reason) => button.on_disabled_hover_text(reason),
                                None => button,
                            };
                            if button.clicked() {
                                run = Some(Box::new(script.clone()));
                            }
                    });
                });
            }
        });
        if start_remove.is_some() {
            self.removing = start_remove;
        }
        if cancel_remove {
            self.removing = None;
        }
        run.map(Action::Run)
            .or(remove.map(Action::RemoveGrant))
            .or(open_site.map(Action::OpenSite))
            .or(action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(title: &str, price: i64, categories: &[&str]) -> StoreScript {
        StoreScript {
            id: title.into(),
            url: title.into(),
            title: title.into(),
            description: String::new(),
            categories: categories.iter().map(|c| (*c).into()).collect(),
            stage: "stable".into(),
            price,
            revision: Some(1),
            cover_url: None,
            can_access: true,
            author: "a".into(),
            online: 0,
            community: false,
            granted: false,
        }
    }

    #[test]
    fn scripts_are_tagged_by_where_they_come_from() {
        let mut community = script("c", 0, &[]);
        community.community = true;
        assert_eq!(origin_label(&community), "community");
        assert_eq!(origin_label(&script("o", 0, &[])), "official");
    }

    #[test]
    fn a_warning_holds_for_one_revision_of_one_script() {
        let mut confirmed = Confirmed::default();
        assert!(!confirmed.has("a", 1));
        confirmed.confirm("a", 1);
        assert!(confirmed.has("a", 1));
        assert!(!confirmed.has("a", 2), "a new revision warns again");
        assert!(!confirmed.has("b", 1), "so does a different script");
        confirmed.confirm("a", 2);
        assert!(
            confirmed.has("a", 2) && !confirmed.has("a", 1),
            "only the latest confirmed revision is kept"
        );
        confirmed.forget("a");
        assert!(!confirmed.has("a", 2));
    }

    #[test]
    fn confirmations_survive_storage_and_stay_bounded() {
        let mut confirmed = Confirmed::default();
        for i in 0..(MAX_CONFIRMED + 20) {
            confirmed.confirm(&format!("script-{i:04}"), 1);
        }
        assert_eq!(confirmed.0.len(), MAX_CONFIRMED);
        let again: Confirmed =
            serde_json::from_str(&serde_json::to_string(&confirmed).unwrap()).unwrap();
        assert_eq!(again, confirmed);
        let before = confirmed.0.len();
        confirmed.confirm("script-0019", 2);
        assert_eq!(confirmed.0.len(), before);
    }

    #[test]
    fn prices_read_as_credit_amounts() {
        assert_eq!(price_texts(&script("a", 0, &[])), None);
        assert_eq!(
            price_texts(&script("a", 180_000, &[])),
            Some(("0.18\u{a0}CREDIT".into(), "0.003\u{a0}CREDIT".into()))
        );
        assert_eq!(
            price_texts(&script("a", 70_000, &[])),
            Some(("0.07\u{a0}CREDIT".into(), "0.001167\u{a0}CREDIT".into()))
        );
    }

    #[test]
    fn search_and_category_filter_the_list() {
        let mut store = Store {
            scripts: vec![
                script("Tempoross", 0, &["fishing"]),
                script("Miner", 5, &["mining"]),
            ],
            ..Store::default()
        };
        assert_eq!(store.visible().count(), 2);
        store.search = "temp".into();
        assert_eq!(
            store
                .visible()
                .map(|s| s.title.as_str())
                .collect::<Vec<_>>(),
            ["Tempoross"]
        );
        store.search.clear();
        store.category = "mining".into();
        assert_eq!(
            store
                .visible()
                .map(|s| s.title.as_str())
                .collect::<Vec<_>>(),
            ["Miner"]
        );
        assert_eq!(store.categories(), ["fishing", "mining"]);
    }
}
