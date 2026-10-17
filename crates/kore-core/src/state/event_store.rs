use crate::error::{K8sError, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing::{error, info};

#[derive(Debug, Clone, Serialize)]
pub struct StoredEvent {
    pub uid: String,
    pub kind: String,
    pub name: String,
    pub namespace: String,
    pub reason: String,
    pub message: String,
    pub event_type: String,
    pub involved_object: String,
    pub count: i64,
    pub first_seen: String,
    pub last_seen: String,
    pub context: String,
}

/// SQLite-backed event history. All queries run on tokio's blocking pool.
#[derive(Clone)]
pub struct EventStore {
    db: Arc<Mutex<Connection>>,
}

impl EventStore {
    /// Run a closure against the connection on the blocking pool.
    async fn with_db<R, F>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&Connection) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db
                .lock()
                .map_err(|_| K8sError::Validation("SQLite connection poisoned".into()))?;
            f(&conn)
        })
        .await
        .map_err(|e| K8sError::Validation(format!("SQLite task failed: {e}")))?
    }

    pub fn new(data_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&data_dir).map_err(K8sError::Io)?;

        let db_path = data_dir.join("events.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| K8sError::Validation(format!("SQLite open failed: {e}")))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .map_err(|e| K8sError::Validation(format!("SQLite pragma failed: {e}")))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                uid TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                namespace TEXT NOT NULL DEFAULT '',
                reason TEXT NOT NULL DEFAULT '',
                message TEXT NOT NULL DEFAULT '',
                event_type TEXT NOT NULL DEFAULT 'Normal',
                involved_object TEXT NOT NULL DEFAULT '',
                count INTEGER NOT NULL DEFAULT 1,
                first_seen TEXT NOT NULL,
                last_seen TEXT NOT NULL,
                context TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_events_last_seen ON events(last_seen);
            CREATE INDEX IF NOT EXISTS idx_events_namespace ON events(namespace);
            CREATE INDEX IF NOT EXISTS idx_events_context ON events(context);",
        )
        .map_err(|e| K8sError::Validation(format!("SQLite schema failed: {e}")))?;

        info!(path = %db_path.display(), "Initialized event store");

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// Upsert an event. Updates count and last_seen if already exists.
    pub async fn upsert_event(&self, event: &StoredEvent) -> Result<()> {
        self.upsert_events(vec![event.clone()]).await
    }

    /// Upsert many events in one transaction.
    pub async fn upsert_events(&self, events: Vec<StoredEvent>) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        self.with_db(move |db| {
            db.execute_batch("BEGIN")
                .map_err(|e| K8sError::Validation(format!("SQLite begin failed: {e}")))?;
            for event in &events {
                if let Err(e) = Self::upsert_one(db, event) {
                    let _ = db.execute_batch("ROLLBACK");
                    return Err(e);
                }
            }
            db.execute_batch("COMMIT")
                .map_err(|e| K8sError::Validation(format!("SQLite commit failed: {e}")))?;
            Ok(())
        })
        .await
    }

    fn upsert_one(db: &Connection, event: &StoredEvent) -> Result<()> {
        db.execute(
            "INSERT INTO events (uid, kind, name, namespace, reason, message, event_type, involved_object, count, first_seen, last_seen, context)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(uid) DO UPDATE SET
                count = ?9,
                last_seen = ?11,
                message = ?6",
            params![
                event.uid,
                event.kind,
                event.name,
                event.namespace,
                event.reason,
                event.message,
                event.event_type,
                event.involved_object,
                event.count,
                event.first_seen,
                event.last_seen,
                event.context,
            ],
        )
        .map_err(|e| K8sError::Validation(format!("SQLite insert failed: {e}")))?;
        Ok(())
    }

    /// Query events within a time range.
    pub async fn query_events(
        &self,
        since: &str,
        until: &str,
        namespace: Option<&str>,
        context: Option<&str>,
    ) -> Result<Vec<StoredEvent>> {
        let since = normalize_ts(since);
        let until = normalize_ts(until);
        let namespace = namespace.map(str::to_string);
        let context = context.map(str::to_string);
        self.with_db(move |db| {
            Self::query_sync(db, &since, &until, namespace.as_deref(), context.as_deref())
        })
        .await
    }

    fn query_sync(
        db: &Connection,
        since: &str,
        until: &str,
        namespace: Option<&str>,
        context: Option<&str>,
    ) -> Result<Vec<StoredEvent>> {
        let mut sql = String::from(
            "SELECT uid, kind, name, namespace, reason, message, event_type, involved_object, count, first_seen, last_seen, context
             FROM events WHERE last_seen >= ?1 AND last_seen <= ?2",
        );
        let mut param_values: Vec<String> = vec![since.to_string(), until.to_string()];

        if let Some(ns) = namespace {
            sql.push_str(&format!(" AND namespace = ?{}", param_values.len() + 1));
            param_values.push(ns.to_string());
        }
        if let Some(ctx) = context {
            sql.push_str(&format!(" AND context = ?{}", param_values.len() + 1));
            param_values.push(ctx.to_string());
        }

        sql.push_str(" ORDER BY last_seen DESC LIMIT 500");

        let mut stmt = db
            .prepare(&sql)
            .map_err(|e| K8sError::Validation(format!("SQLite query failed: {e}")))?;

        let params_refs: Vec<&dyn rusqlite::ToSql> = param_values
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();

        let events = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(StoredEvent {
                    uid: row.get(0)?,
                    kind: row.get(1)?,
                    name: row.get(2)?,
                    namespace: row.get(3)?,
                    reason: row.get(4)?,
                    message: row.get(5)?,
                    event_type: row.get(6)?,
                    involved_object: row.get(7)?,
                    count: row.get(8)?,
                    first_seen: row.get(9)?,
                    last_seen: row.get(10)?,
                    context: row.get(11)?,
                })
            })
            .map_err(|e| K8sError::Validation(format!("SQLite query failed: {e}")))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(events)
    }

    /// Prune events older than retention_days.
    pub async fn prune(&self, retention_days: u32) -> Result<u64> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
        // Same format Kubernetes uses for timestamps, so string comparison works.
        let cutoff_str = cutoff.format("%Y-%m-%dT%H:%M:%SZ").to_string();

        let count = self
            .with_db(move |db| {
                db.execute(
                    "DELETE FROM events WHERE last_seen < ?1",
                    params![cutoff_str],
                )
                .map_err(|e| K8sError::Validation(format!("SQLite prune failed: {e}")))
            })
            .await?;

        info!(pruned = count, "Pruned old events from store");
        Ok(count as u64)
    }

    /// Store a batch of raw Event objects (from a watch or list).
    pub async fn store_events(&self, events: Vec<serde_json::Value>, context: String) {
        let stored: Vec<StoredEvent> = events
            .iter()
            .filter_map(|e| Self::to_stored(e, &context))
            .collect();
        if let Err(e) = self.upsert_events(stored).await {
            error!(error = %e, "Failed to store events");
        }
    }

    /// Store a batch of events from a watch stream.
    pub async fn store_watch_event(&self, event_json: &serde_json::Value, context: &str) {
        if let Some(stored) = Self::to_stored(event_json, context) {
            if let Err(e) = self.upsert_event(&stored).await {
                error!(error = %e, "Failed to store event");
            }
        }
    }

    /// Convert a raw core/v1 Event into a [`StoredEvent`]. `None` without a uid.
    pub fn to_stored(event_json: &serde_json::Value, context: &str) -> Option<StoredEvent> {
        let uid = event_json
            .pointer("/metadata/uid")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if uid.is_empty() {
            return None;
        }
        let ts = |keys: &[&str]| -> String {
            keys.iter()
                .filter_map(|k| {
                    if k.starts_with('/') {
                        event_json.pointer(k)
                    } else {
                        event_json.get(*k)
                    }
                })
                .filter_map(|v| v.as_str())
                .find(|s| !s.is_empty())
                .map(normalize_ts)
                .unwrap_or_default()
        };

        Some(StoredEvent {
            uid: uid.to_string(),
            kind: event_json
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("Normal")
                .to_string(),
            name: event_json
                .pointer("/metadata/name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            namespace: event_json
                .pointer("/metadata/namespace")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            reason: event_json
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            message: event_json
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            event_type: event_json
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("Normal")
                .to_string(),
            involved_object: event_json
                .pointer("/involvedObject")
                .map(|io| {
                    format!(
                        "{}/{}",
                        io.get("kind").and_then(|v| v.as_str()).unwrap_or(""),
                        io.get("name").and_then(|v| v.as_str()).unwrap_or("")
                    )
                })
                .unwrap_or_default(),
            count: event_json
                .get("count")
                .and_then(|v| v.as_i64())
                .unwrap_or(1),
            first_seen: ts(&["firstTimestamp", "eventTime", "/metadata/creationTimestamp"]),
            last_seen: ts(&[
                "lastTimestamp",
                "/series/lastObservedTime",
                "eventTime",
                "/metadata/creationTimestamp",
            ]),
            context: context.to_string(),
        })
    }
}

/// Normalize RFC3339 timestamps (`eventTime` has microseconds) to the
/// second-precision `…Z` form Kubernetes uses elsewhere so lexical range
/// comparisons in SQL are consistent.
fn normalize_ts(s: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(s) {
        Ok(dt) => dt
            .with_timezone(&chrono::Utc)
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string(),
        Err(_) => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(uid: &str, ns: &str, last: &str) -> serde_json::Value {
        json!({
            "metadata": {"uid": uid, "name": format!("{uid}.x"), "namespace": ns, "creationTimestamp": last},
            "involvedObject": {"kind": "Pod", "name": "p"},
            "reason": "BackOff", "message": "m", "type": "Warning", "count": 3,
            "firstTimestamp": last, "lastTimestamp": last
        })
    }

    #[tokio::test]
    async fn store_query_prune_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = EventStore::new(dir.path().to_path_buf()).unwrap();
        let now = chrono::Utc::now();
        let recent = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let old = (now - chrono::Duration::days(40))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        store
            .store_events(
                vec![ev("a", "ns1", &recent), ev("b", "ns2", &old)],
                "ctx".into(),
            )
            .await;
        // Upsert again (idempotent on uid)
        store
            .store_events(vec![ev("a", "ns1", &recent)], "ctx".into())
            .await;

        let since = (now - chrono::Duration::days(60)).to_rfc3339();
        let until = (now + chrono::Duration::minutes(1)).to_rfc3339();
        let all = store
            .query_events(&since, &until, None, Some("ctx"))
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        let ns1 = store
            .query_events(&since, &until, Some("ns1"), None)
            .await
            .unwrap();
        assert_eq!(ns1.len(), 1);
        assert_eq!(ns1[0].involved_object, "Pod/p");
        assert_eq!(ns1[0].count, 3);
        let other_ctx = store
            .query_events(&since, &until, None, Some("nope"))
            .await
            .unwrap();
        assert!(other_ctx.is_empty());

        let pruned = store.prune(30).await.unwrap();
        assert_eq!(pruned, 1);
        let left = store
            .query_events(&since, &until, None, None)
            .await
            .unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].uid, "a");
    }

    #[test]
    fn event_time_only_events_get_timestamps() {
        let e = json!({
            "metadata": {"uid": "u", "name": "n", "namespace": "ns", "creationTimestamp": "2026-10-15T10:00:00Z"},
            "eventTime": "2026-10-15T10:00:05.123456Z",
            "reason": "Scheduled", "type": "Normal"
        });
        let s = EventStore::to_stored(&e, "c").unwrap();
        assert_eq!(s.last_seen, "2026-10-15T10:00:05Z");
        assert_eq!(s.first_seen, "2026-10-15T10:00:05Z");
    }
}
