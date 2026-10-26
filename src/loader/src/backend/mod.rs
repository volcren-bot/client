use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use injector::Client;

#[cfg(windows)]
mod native;
#[cfg(windows)]
pub use native::{
    Location, connection, console, draw, input, item_container, load, npcs, players, scan, script_command, set_cursor, set_display_name, shutdown,
    scene_tiles, state, unload, varc, varp, varps, widget,
};

#[cfg(not(windows))]
mod wine;
#[cfg(not(windows))]
pub use wine::{
    Location, connection, console, draw, input, item_container, load, npcs, players, scan, script_command, set_cursor, set_display_name, shutdown,
    scene_tiles, state, unload, varc, varp, varps, widget,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub dll: String,
    pub helper: String,
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
    pub fn fill_missing(&mut self) {
        let defaults = Self::default();
        if !Path::new(&self.dll).is_file() && !defaults.dll.is_empty() {
            self.dll = defaults.dll;
        }
        if !Path::new(&self.helper).is_file() && !defaults.helper.is_empty() {
            self.helper = defaults.helper;
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
        format!("{}:{}", self.location.label().unwrap_or_default(), self.client.pid)
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
