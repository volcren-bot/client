use std::path::{Path, PathBuf};

use injector::Client;
use serde::{Deserialize, Serialize};

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::{
    Location, connection, console, draw, enum_def, game_val_names, input, item_changes,
    item_container, item_containers, item_doses, item_icons, item_names, item_params,
    item_stackable, load, npcs, packet_layouts, packets, players, scan, scene_tiles_next_frame,
    script_command, script_runs, set_cursor, set_display_name, shutdown, sprites, state, unload,
    varbit, varbits_of, varc, varc_changes, varp, varps, widget, widget_tree,
};

#[cfg(not(windows))]
mod wine;
#[cfg(not(windows))]
pub use wine::{
    Location, connection, console, draw, enum_def, game_val_names, input, item_changes,
    item_container, item_containers, item_doses, item_icons, item_names, item_params,
    item_stackable, load, npcs, packet_layouts, packets, players, scan, scene_tiles_next_frame,
    script_command, script_runs, set_cursor, set_display_name, shutdown, sprites, state, unload,
    varbit, varbits_of, varc, varc_changes, varp, varps, widget, widget_tree,
};

/// How many lines an output tab may keep: each new line lays out all of them again, which costs about 6 ms and 13 MB per thousand.
pub const LOG_LINES: std::ops::RangeInclusive<usize> = 2000..=10_000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub dll: String,
    pub helper: String,
    /// Your own lib folder: local script files read `require("../lib/...")` from it instead of the downloaded lib.
    pub lib_dir: String,
    /// The breaks and sleeps every script takes, sent to the script when it starts and when this changes.
    pub antiban: injector::antiban::Config,
    /// The types of transport the user turned off, sent to the script when it starts and when this changes.
    pub transports: injector::transports::Settings,
    /// Whether a bug report carries the OS, Wine, CPU, memory and GPU besides the versions.
    pub include_system_info: bool,
    /// How many lines each output tab keeps, within `LOG_LINES`.
    pub log_lines: usize,
}

impl Default for Settings {
    fn default() -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        Self {
            dll: detect(&exe_dir, "agent.dll"),
            helper: detect(&exe_dir, "injector.exe"),
            lib_dir: String::new(),
            antiban: Default::default(),
            transports: Default::default(),
            include_system_info: true,
            log_lines: *LOG_LINES.start(),
        }
    }
}

/// Finds `name` next to the loader (an installed release), then in the build output next to it (`target/x86_64-pc-windows-gnu/...`).
fn detect(exe_dir: &Path, name: &str) -> String {
    // The loader sits in target/<profile>, so prefer the agent and injector built with the same profile.
    let profile = exe_dir.file_name().map(PathBuf::from).unwrap_or_default();
    [
        exe_dir.to_path_buf(),
        exe_dir.join("../x86_64-pc-windows-gnu").join(&profile),
        exe_dir.join("../x86_64-pc-windows-gnu/release"),
        exe_dir.join("../x86_64-pc-windows-gnu/debug"),
    ]
    .iter()
    .map(|dir| dir.join(name))
    .find(|p| p.is_file())
    .and_then(|p| p.canonicalize().ok())
    .map(|p| p.display().to_string())
    .unwrap_or_default()
}

impl Settings {
    /// An install always uses its own files, so a path saved by an older install can't pair them with another version. A dev build keeps a saved path while its file exists. `log_lines` is brought into `LOG_LINES`.
    pub fn fill_missing(&mut self, dev: bool) {
        self.resolve_paths(Self::default(), dev);
        self.log_lines = self.log_lines.clamp(*LOG_LINES.start(), *LOG_LINES.end());
    }

    fn resolve_paths(&mut self, detected: Self, dev: bool) {
        if !detected.dll.is_empty() && (!dev || !Path::new(&self.dll).is_file()) {
            self.dll = detected.dll;
        }
        if !detected.helper.is_empty() && (!dev || !Path::new(&self.helper).is_file()) {
            self.helper = detected.helper;
        }
    }

    pub fn dll_version(&self) -> Result<String, String> {
        std::fs::read(&self.dll)
            .map(|dll| injector::dll_version(&dll))
            .map_err(|e| format!("Cannot read {}: {e}", self.dll))
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Target {
    pub client: Client,
    pub location: Location,
}

impl Target {
    pub fn key(&self) -> String {
        format!(
            "{}:{}",
            self.location.label().unwrap_or_default(),
            self.client.pid
        )
    }
}

#[derive(Default)]
pub struct Scan {
    pub targets: Vec<Target>,
    pub errors: Vec<String>,
    pub dll_version: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_install_ignores_saved_paths_and_a_dev_build_keeps_those_that_exist() {
        let dir = std::env::temp_dir().join(format!("volcren-paths-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("old.dll").display().to_string();
        std::fs::write(&old, b"").unwrap();
        let with = |dll: &str| Settings {
            dll: dll.into(),
            helper: String::new(),
            ..Default::default()
        };

        let mut saved = with(&old);
        saved.resolve_paths(with("detected.dll"), false);
        assert_eq!(saved.dll, "detected.dll");

        let mut saved = with(&old);
        saved.resolve_paths(with("detected.dll"), true);
        assert_eq!(saved.dll, old);

        let mut saved = with("gone.dll");
        saved.resolve_paths(with("detected.dll"), true);
        assert_eq!(saved.dll, "detected.dll");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_installed_release_finds_its_files_next_to_the_executable() {
        let dir = std::env::temp_dir().join(format!("volcren-detect-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("agent.dll"), b"").unwrap();
        std::fs::write(dir.join("injector.exe"), b"").unwrap();
        // No `target/` anywhere near it.
        assert!(detect(&dir, "agent.dll").ends_with("agent.dll"));
        assert!(detect(&dir, "injector.exe").ends_with("injector.exe"));
        assert_eq!(detect(&dir, "missing.dll"), "");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
