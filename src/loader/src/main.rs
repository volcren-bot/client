#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod account;
mod accounts;
mod api;
mod app;
mod breaks;
mod counters;
#[cfg(test)]
mod fake_api;
mod feedback;
#[cfg(test)]
mod flow_tests;
mod fold;
mod form;
mod highlight;
mod http;
mod icons;
mod inventory;
mod layouts;
mod libs;
mod log_view;
mod logo;
mod mcp;
mod packet_defs;
mod packets;
mod paths;
mod pipe;
mod queue;
mod script;
mod script_log;
mod script_names;
mod session;
mod store;
mod terminal;
mod theme;
mod transports;
mod update;
mod vars;
mod whitelist;
mod widgets;
mod workspace;
mod xp;

use eframe::egui;

pub const APP_ID: &str = "Loader";

fn main() -> eframe::Result {
    if std::env::args().nth(1).as_deref() == Some("mcp") {
        let _instance = update::Instance::hold();
        mcp::serve();
        return Ok(());
    }
    #[cfg(windows)]
    if update::finish_if_asked() {
        return Ok(());
    }
    let instance = update::Instance::hold();
    if instance.alone() {
        update::cleanup();
    }
    transports::start();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id(APP_ID)
            .with_inner_size([900.0, 640.0])
            .with_min_inner_size([520.0, 300.0]),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Volcren",
        options,
        Box::new(|cc| Ok(Box::new(app::LoaderApp::new(cc, instance)))),
    );
    #[cfg(not(windows))]
    update::relaunch_after_exit();
    result
}
