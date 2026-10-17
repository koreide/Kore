//! User settings, stored under `kore-settings` (spec 01 §19).

use super::storage;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const KEY: &str = "kore-settings";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub event_retention: String,
    pub default_namespaces: BTreeMap<String, String>,
    pub accent_color: String,
    pub default_view: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            event_retention: "7d".into(),
            default_namespaces: BTreeMap::new(),
            accent_color: "#58d0ff".into(),
            default_view: "chat".into(),
        }
    }
}

impl Settings {
    pub fn load() -> Settings {
        storage::get(KEY).unwrap_or_default()
    }

    pub fn save(&self) {
        storage::set(KEY, self);
    }

    pub fn accent_color(&self) -> u32 {
        u32::from_str_radix(self.accent_color.trim_start_matches('#'), 16).unwrap_or(crate::theme::DEFAULT_ACCENT)
    }

    /// Event retention in days (`"7d"` → 7). Unparsable or zero values fall back to 7.
    pub fn event_retention_days(&self) -> u32 {
        self.event_retention.trim().strip_suffix('d').and_then(|n| n.parse().ok()).filter(|&n| n > 0).unwrap_or(7)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_retention_days_parses_setting() {
        let with = |v: &str| Settings { event_retention: v.into(), ..Settings::default() }.event_retention_days();
        assert_eq!(with("1d"), 1);
        assert_eq!(with("30d"), 30);
        assert_eq!(Settings::default().event_retention_days(), 7);
        assert_eq!(with("0d"), 7);
        assert_eq!(with("bogus"), 7);
    }
}
