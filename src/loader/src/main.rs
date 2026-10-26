#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod form;
mod highlight;
mod script;
mod theme;

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
