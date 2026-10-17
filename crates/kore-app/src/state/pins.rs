//! Pinned resources, stored under `kore-pinned-resources` (spec 01 §9): newest
//! first, max 50.

use super::storage;
use serde::{Deserialize, Serialize};

pub const KEY: &str = "kore-pinned-resources";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub kind: String,
    pub name: String,
    pub namespace: String,
}

pub fn load() -> Vec<Pin> {
    storage::get(KEY).unwrap_or_default()
}

pub fn save(pins: &[Pin]) {
    storage::set(KEY, &pins);
}
