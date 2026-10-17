//! Favorites persistence (`~/.kore/favorites.json`, `{key: [values]}`).

use crate::error::{K8sError, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn favorites_path(dir: Option<&Path>) -> Result<PathBuf> {
    let dir = dir
        .map(Path::to_path_buf)
        .ok_or_else(|| K8sError::other("Cannot determine home directory"))?;
    std::fs::create_dir_all(&dir).map_err(|e| K8sError::Other(e.to_string()))?;
    Ok(dir.join("favorites.json"))
}

fn read_all(dir: Option<&Path>) -> Result<HashMap<String, Vec<String>>> {
    let path = favorites_path(dir)?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let data = std::fs::read_to_string(&path).map_err(|e| K8sError::Other(e.to_string()))?;
    serde_json::from_str(&data).map_err(|e| K8sError::Other(e.to_string()))
}

pub fn load_favorites(dir: Option<&Path>, key: &str) -> Result<Vec<String>> {
    let map = read_all(dir)?;
    Ok(map.get(key).cloned().unwrap_or_default())
}

/// Read-modify-write; written atomically via a temp file + rename.
pub fn save_favorites(dir: Option<&Path>, key: String, values: Vec<String>) -> Result<()> {
    let mut map = read_all(dir)?;
    map.insert(key, values);
    let path = favorites_path(dir)?;
    let data = serde_json::to_string_pretty(&map).map_err(|e| K8sError::Other(e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data).map_err(|e| K8sError::Other(e.to_string()))?;
    std::fs::rename(&tmp, &path).map_err(|e| K8sError::Other(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favorites_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let d = Some(dir.path());
        assert!(load_favorites(d, "contexts").unwrap().is_empty());
        save_favorites(d, "contexts".into(), vec!["a".into(), "b".into()]).unwrap();
        save_favorites(d, "namespaces".into(), vec!["x".into()]).unwrap();
        assert_eq!(load_favorites(d, "contexts").unwrap(), vec!["a", "b"]);
        assert_eq!(load_favorites(d, "namespaces").unwrap(), vec!["x"]);
        let raw = std::fs::read_to_string(dir.path().join("favorites.json")).unwrap();
        assert!(raw.contains("\"contexts\""));
    }
}
