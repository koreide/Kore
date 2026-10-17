//! Persistent key/value store (string keys, JSON values), stored at
//! `<data dir>/kore/local-storage.json`. Unit tests get a per-thread in-memory
//! store instead (each `#[gpui::test]` runs on its own thread), so they never
//! read or write the user's file and don't see each other's state.

use serde::{de::DeserializeOwned, Serialize};
use serde_json::{Map, Value};
#[cfg(not(test))]
use std::path::PathBuf;
use std::sync::Mutex;
#[cfg(not(test))]
use std::sync::OnceLock;

#[cfg(not(test))]
fn path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".kore"))
        .join("kore")
        .join("local-storage.json")
}

#[cfg(not(test))]
fn store() -> &'static Mutex<Map<String, Value>> {
    static STORE: OnceLock<Mutex<Map<String, Value>>> = OnceLock::new();
    STORE.get_or_init(|| {
        let map = std::fs::read(path())
            .ok()
            .and_then(|b| serde_json::from_slice::<Map<String, Value>>(&b).ok())
            .unwrap_or_default();
        Mutex::new(map)
    })
}

#[cfg(test)]
fn store() -> &'static Mutex<Map<String, Value>> {
    thread_local! {
        static STORE: &'static Mutex<Map<String, Value>> = Box::leak(Box::new(Mutex::new(Map::new())));
    }
    STORE.with(|s| *s)
}

pub fn get<T: DeserializeOwned>(key: &str) -> Option<T> {
    let s = store().lock().ok()?;
    serde_json::from_value(s.get(key)?.clone()).ok()
}

pub fn set<T: Serialize>(key: &str, value: &T) {
    let Ok(v) = serde_json::to_value(value) else { return };
    let Ok(mut s) = store().lock() else { return };
    s.insert(key.to_string(), v);
    persist(&s);
}

pub fn remove(key: &str) {
    let Ok(mut s) = store().lock() else { return };
    s.remove(key);
    persist(&s);
}

#[cfg(test)]
fn persist(_: &Map<String, Value>) {}

#[cfg(not(test))]
fn persist(map: &Map<String, Value>) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(map) {
        // Write-then-rename so a crash never leaves a truncated file.
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}
