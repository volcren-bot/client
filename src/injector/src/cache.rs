
use serde::{Deserialize, Serialize};

pub const CONFIG_INDEX: u32 = 2;
pub const MAPS_INDEX: u32 = 5;
pub const LOC_ARCHIVE: i32 = 6;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheIds {
    pub ids: Vec<u32>,
    pub fingerprint: u64,
}
