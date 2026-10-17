//! JSON helpers.
//!
//! GPUI enables serde_json's `preserve_order` feature and Cargo unifies
//! features across the workspace, so `serde_json::Value` objects keep the API
//! server's key order. Describe/YAML/diff output lists keys alphabetically, so
//! values that kore-core returns for display or diffing go through
//! [`sort_keys`].

use serde_json::Value;

/// Recursively sort object keys (alphabetically, like a BTreeMap).
pub fn sort_keys(v: &mut Value) {
    match v {
        Value::Object(map) => {
            map.sort_keys();
            for (_, child) in map.iter_mut() {
                sort_keys(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(sort_keys),
        _ => {}
    }
}

/// By-value form of [`sort_keys`].
pub fn sorted(mut v: Value) -> Value {
    sort_keys(&mut v);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sorts_nested_keys() {
        let mut v = json!({"kind": "Pod", "apiVersion": "v1", "metadata": {"uid": "u", "name": "n"}, "list": [{"b": 1, "a": 2}]});
        sort_keys(&mut v);
        assert_eq!(
            serde_json::to_string(&v).unwrap(),
            r#"{"apiVersion":"v1","kind":"Pod","list":[{"a":2,"b":1}],"metadata":{"name":"n","uid":"u"}}"#
        );
        let y = serde_yaml::to_string(&v).unwrap();
        assert!(y.starts_with("apiVersion: v1\nkind: Pod\nlist:"), "{y}");
    }
}
