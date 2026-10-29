#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod api;
mod account;
mod app;
mod breaks;
mod counters;
#[cfg(test)]
mod fake_api;
#[cfg(test)]
mod flow_tests;
mod form;
mod fold;
mod highlight;
mod script;
mod session;
mod store;
mod terminal;
mod theme;
mod libs;
mod logo;
mod update;
mod xp;

use eframe::egui;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // Keeps the saved settings, which eframe stores under the app id (the old window title).
            .with_app_id("Loader")
            .with_inner_size([900.0, 640.0])
            .with_min_inner_size([520.0, 300.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Volcren",
        options,
        Box::new(|cc| Ok(Box::new(app::LoaderApp::new(cc)))),
    )
}
