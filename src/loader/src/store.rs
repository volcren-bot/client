//! The Scripts tab: the scripts the account can see, and what it takes to run each.

use std::collections::BTreeMap;

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::api::{
    Me, StoreScript,
    models::{credits_to_eur, eur_per_hour, normalize_share_code},
};

/// How many community scripts the loader remembers warning about. Old entries are only a missed "don't ask again".
const MAX_CONFIRMED: usize = 500;

/// The community scripts the user was warned about and chose to run, and the revision they confirmed. A new revision
/// of the same script, or a different script, warns again. Kept in the loader's local storage, per machine.
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

    /// For a script that left the account: it would be warned about again if it ever came back.
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
    /// Testers and above: list every community script instead of the default list.
    pub community: bool,
    /// The share code being typed, and what happened to the last one (`true` when it worked).
    pub code: String,
    pub redeeming: bool,
    pub redeem_result: Option<(bool, String)>,
    /// The script whose "remove from my account" is waiting for a second click.
    pub removing: Option<String>,
}

/// What the user asked for in the list this frame.
pub enum Action {
    Refresh,
    Run(Box<StoreScript>),
    /// A share code that already passed `normalize_share_code`.
    Redeem(String),
    RemoveGrant(Box<StoreScript>),
}

/// The tag next to a script's title: where it comes from.
pub fn origin_label(script: &StoreScript) -> &'static str {
    if script.community { "community" } else { "official" }
}

pub fn price_label(script: &StoreScript) -> String {
    if script.is_free() {
        "Free".into()
    } else {
        format!("{} credits/min (€{:.2}/h)", script.price, eur_per_hour(script.price))
    }
}

pub fn balance_label(me: &Me) -> String {
    format!("{} credits (€{:.2})", me.credits, credits_to_eur(me.credits))
}

impl Store {
    pub fn categories(&self) -> Vec<String> {
        let mut all: Vec<String> = self.scripts.iter().flat_map(|s| s.categories.iter().cloned()).collect();
        all.sort();
        all.dedup();
        all
    }

    pub fn visible(&self) -> impl Iterator<Item = &StoreScript> {
        let search = self.search.trim().to_lowercase();
        self.scripts.iter().filter(move |s| {
            (self.category.is_empty() || s.categories.contains(&self.category))
                && (search.is_empty() || s.title.to_lowercase().contains(&search) || s.description.to_lowercase().contains(&search))
        })
    }

    /// `run_block` is why Run is disabled for reasons outside the script (no client, a script already running).
    pub fn ui(&mut self, ui: &mut egui::Ui, me: &Me, run_block: Option<&str>) -> Option<Action> {
        let mut action = None;
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search scripts").desired_width(220.0));
            let categories = self.categories();
            egui::ComboBox::from_id_salt("store-category")
                .selected_text(if self.category.is_empty() { "All categories" } else { &self.category })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.category, String::new(), "All categories");
                    for category in categories {
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
        ui.horizontal(|ui| {
            let editing = ui.add_enabled(
                !self.redeeming,
                egui::TextEdit::singleline(&mut self.code).hint_text("Paste a share code").desired_width(420.0).password(false),
            );
            let submitted = editing.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.add_enabled(!self.redeeming && !self.code.trim().is_empty(), egui::Button::new("Redeem")).clicked() || submitted)
                && !self.redeeming
            {
                match normalize_share_code(&self.code) {
                    Some(code) => action = Some(Action::Redeem(code)),
                    None => self.redeem_result = Some((false, "A share code is 64 characters: the digits 0-9 and the letters a-f.".into())),
                }
            }
            if self.redeeming {
                ui.spinner();
            }
            if let Some((ok, message)) = &self.redeem_result {
                let color = if *ok { egui::Color32::from_rgb(0x9e, 0xce, 0x6a) } else { ui.visuals().error_fg_color };
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
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for script in self.visible() {
                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&script.title).strong().size(16.0));
                        ui.label(egui::RichText::new(format!("by {}", script.author)).weak());
                        ui.label(egui::RichText::new(&script.stage).small());
                        let tag = egui::RichText::new(origin_label(script)).small();
                        if script.community {
                            ui.label(tag.color(egui::Color32::from_rgb(0xff, 0x9e, 0x64)))
                                .on_hover_text("Written by a community member, not reviewed by staff. You're warned before it runs.");
                        } else {
                            ui.label(tag.color(egui::Color32::from_rgb(0x7a, 0xa2, 0xf7))).on_hover_text("Written by a scripter.");
                        }
                        if script.granted {
                            ui.label(egui::RichText::new("shared with you").small().color(egui::Color32::from_rgb(0x9e, 0xce, 0x6a)));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let blocked = run_block.map(str::to_owned).or_else(|| script.blocked(me.credits, me.min_balance_minutes));
                            let button = ui.add_enabled(blocked.is_none(), egui::Button::new("Run"));
                            let button = match &blocked {
                                Some(reason) => button.on_disabled_hover_text(reason),
                                None => button,
                            };
                            if button.clicked() {
                                run = Some(Box::new(script.clone()));
                            }
                            ui.label(format!("{} online", script.online));
                            ui.label(price_label(script));
                        });
                    });
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
                });
            }
        });
        if start_remove.is_some() {
            self.removing = start_remove;
        }
        if cancel_remove {
            self.removing = None;
        }
        run.map(Action::Run).or(remove.map(Action::RemoveGrant)).or(action)
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
        assert!(confirmed.has("a", 2) && !confirmed.has("a", 1), "only the latest confirmed revision is kept");
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
        let again: Confirmed = serde_json::from_str(&serde_json::to_string(&confirmed).unwrap()).unwrap();
        assert_eq!(again, confirmed);
        // Confirming a script that is already remembered never evicts another one
        let before = confirmed.0.len();
        confirmed.confirm("script-0019", 2);
        assert_eq!(confirmed.0.len(), before);
    }

    #[test]
    fn prices_read_as_credits_and_euros() {
        assert_eq!(price_label(&script("a", 0, &[])), "Free");
        assert_eq!(price_label(&script("a", 30, &[])), "30 credits/min (€0.18/h)");
    }

    #[test]
    fn search_and_category_filter_the_list() {
        let mut store = Store { scripts: vec![script("Tempoross", 0, &["fishing"]), script("Miner", 5, &["mining"])], ..Store::default() };
        assert_eq!(store.visible().count(), 2);
        store.search = "temp".into();
        assert_eq!(store.visible().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Tempoross"]);
        store.search.clear();
        store.category = "mining".into();
        assert_eq!(store.visible().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Miner"]);
        assert_eq!(store.categories(), ["fishing", "mining"]);
    }
}
