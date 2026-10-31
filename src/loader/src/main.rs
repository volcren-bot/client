#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod account;
mod accounts;
mod api;
mod app;
mod breaks;
mod counters;
#[cfg(test)]
mod fake_api;
#[cfg(test)]
mod flow_tests;
mod fold;
mod form;
mod highlight;
mod http;
mod inventory;
mod libs;
mod log_view;
mod logo;
mod packet_defs;
mod packets;
mod script;
mod script_log;
mod script_names;
mod session;
mod store;
mod terminal;
mod theme;
mod update;
mod vars;
mod whitelist;
mod widgets;
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
