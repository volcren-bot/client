//! The Scripts tab: the scripts the account can see, and what it takes to run each.

use eframe::egui;

use crate::api::{
    Me, StoreScript,
    models::{credits_to_eur, eur_per_hour},
};

#[derive(Default)]
pub struct Store {
    pub scripts: Vec<StoreScript>,
    pub loading: bool,
    pub loaded: bool,
    pub error: Option<String>,
    pub search: String,
    pub category: String,
}

/// What the user asked for in the list this frame.
pub enum Action {
    Refresh,
    Run(Box<StoreScript>),
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
            if self.loading {
                ui.spinner();
            }
        });
        ui.add_space(4.0);
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if self.loaded && self.scripts.is_empty() && self.error.is_none() {
            ui.label("No scripts available.");
        }

        let mut run = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for script in self.visible() {
                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&script.title).strong().size(16.0));
                        ui.label(egui::RichText::new(format!("by {}", script.author)).weak());
                        ui.label(egui::RichText::new(&script.stage).small());
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
                });
            }
        });
        run.map(Action::Run).or(action)
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
        }
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
