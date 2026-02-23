use crate::error::{K8sError, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
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

#[derive(Clone)]
pub struct EventStore {
    db: Arc<Mutex<Connection>>,
}

#[allow(dead_code)]
impl EventStore {
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&data_dir).map_err(K8sError::Io)?;

        let db_path = data_dir.join("events.db");
        let conn = Connection::open(&db_path)
            .map_err(|e| K8sError::Validation(format!("SQLite open failed: {e}")))?;

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
        let db = self.db.lock().await;
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
        let db = self.db.lock().await;

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
        let db = self.db.lock().await;
        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
        let cutoff_str = cutoff.to_rfc3339();

        let count = db
            .execute(
                "DELETE FROM events WHERE last_seen < ?1",
                params![cutoff_str],
            )
            .map_err(|e| K8sError::Validation(format!("SQLite prune failed: {e}")))?;

        info!(pruned = count, "Pruned old events from store");
        Ok(count as u64)
    }

    /// Store a batch of events from a watch stream.
    pub async fn store_watch_event(&self, event_json: &serde_json::Value, context: &str) {
        let uid = event_json
            .pointer("/metadata/uid")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if uid.is_empty() {
            return;
        }

        let stored = StoredEvent {
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
            first_seen: event_json
                .get("firstTimestamp")
                .or_else(|| event_json.pointer("/metadata/creationTimestamp"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            last_seen: event_json
                .get("lastTimestamp")
                .or_else(|| event_json.pointer("/metadata/creationTimestamp"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            context: context.to_string(),
        };

        if let Err(e) = self.upsert_event(&stored).await {
            error!(error = %e, "Failed to store event");
        }
    }
}
