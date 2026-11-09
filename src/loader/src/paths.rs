pub const USER_AGENT: &str = concat!("volcren-loader/", env!("VOLCREN_VERSION"));

use std::time::{SystemTime, UNIX_EPOCH};

pub fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("com", "volcren", "Volcren")
}

pub fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
