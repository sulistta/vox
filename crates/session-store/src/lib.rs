use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("database is corrupt or is not a SQLite database")]
    Corrupt,
    #[error("session not found")]
    SessionNotFound,
    #[error("session has an active run")]
    ActiveRun,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageRecord {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub seq: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunRecord {
    pub id: String,
    pub session_id: String,
    pub state: String,
    pub model_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSummary {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub latest_run_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EffectRecord {
    pub idempotency_key: String,
    pub run_id: String,
    pub call_id: String,
    pub tool: String,
    pub status: String,
    pub side_effect: String,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalRecord {
    pub id: String,
    pub run_id: String,
    pub call_id: String,
    pub args_hash: String,
    pub scope: String,
    pub expires_at: u64,
    pub state: String,
}

pub struct SessionStore {
    connection: Connection,
}

const CURRENT_SCHEMA_VERSION: i64 = 2;

impl SessionStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let connection = Connection::open(path).map_err(classify_sql_error)?;
        let store = Self { connection };
        store.migrate().map_err(classify_store_error)?;
        store.recover_interrupted()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        let connection = Connection::open_in_memory()?;
        let store = Self { connection };
        store.migrate().map_err(classify_store_error)?;
        store.recover_interrupted()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), StoreError> {
        self.connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS migrations(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS sessions(id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS messages(id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE, role TEXT NOT NULL, content TEXT NOT NULL, seq INTEGER NOT NULL, created_at TEXT NOT NULL, UNIQUE(session_id, seq));
             CREATE TABLE IF NOT EXISTS runs(id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE, state TEXT NOT NULL, model_ref TEXT, started_at TEXT NOT NULL, finished_at TEXT);
             CREATE TABLE IF NOT EXISTS effects(idempotency_key TEXT PRIMARY KEY, run_id TEXT NOT NULL, call_id TEXT NOT NULL, tool TEXT NOT NULL, status TEXT NOT NULL, side_effect TEXT NOT NULL, data TEXT NOT NULL, created_at TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS messages_session_seq ON messages(session_id, seq);
             CREATE INDEX IF NOT EXISTS runs_session_state ON runs(session_id, state);",
        )?;
        self.connection.execute("INSERT OR IGNORE INTO migrations(version, applied_at) VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ','now'))", [])?;
        // Version 2 adds the durable, non-secret preference and approval
        // records. Approval rows are audit state only; authorization tokens
        // remain short-lived in memory and are never reconstructed on boot.
        self.connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS preferences(key TEXT PRIMARY KEY, value TEXT NOT NULL, schema_version INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS approvals(id TEXT PRIMARY KEY, run_id TEXT NOT NULL, call_id TEXT NOT NULL, args_hash TEXT NOT NULL, scope TEXT NOT NULL, expires_at TEXT NOT NULL, state TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS approvals_run_state ON approvals(run_id, state);",
        )?;
        self.connection.execute("INSERT OR IGNORE INTO migrations(version, applied_at) VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ','now'))", params![CURRENT_SCHEMA_VERSION])?;
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64, StoreError> {
        Ok(self
            .connection
            .query_row("SELECT MAX(version) FROM migrations", [], |row| row.get(0))?)
    }

    pub fn create_session(&self, title: &str) -> Result<String, StoreError> {
        let id = Uuid::new_v4().to_string();
        self.connection.execute("INSERT INTO sessions(id,title,created_at,updated_at) VALUES (?1,?2,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now'))", params![id, title])?;
        Ok(id)
    }

    pub fn append_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        seq: i64,
    ) -> Result<MessageRecord, StoreError> {
        if !self.session_exists(session_id)? {
            return Err(StoreError::SessionNotFound);
        }
        let record = MessageRecord {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            role: role.into(),
            content: redact_sensitive(content),
            seq,
        };
        self.connection.execute("INSERT INTO messages(id,session_id,role,content,seq,created_at) VALUES (?1,?2,?3,?4,?5,strftime('%Y-%m-%dT%H:%M:%fZ','now'))", params![record.id, record.session_id, record.role, record.content, record.seq])?;
        self.connection.execute(
            "UPDATE sessions SET updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?1",
            params![session_id],
        )?;
        Ok(record)
    }

    pub fn start_run(
        &self,
        session_id: &str,
        model_ref: Option<&str>,
    ) -> Result<RunRecord, StoreError> {
        if !self.session_exists(session_id)? {
            return Err(StoreError::SessionNotFound);
        }
        let active: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM runs WHERE session_id=?1 AND state NOT IN ('completed','failed','cancelled','interrupted')",
            params![session_id],
            |row| row.get(0),
        )?;
        if active > 0 {
            return Err(StoreError::ActiveRun);
        }
        let record = RunRecord {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.into(),
            state: "running".into(),
            model_ref: model_ref.map(str::to_owned),
        };
        self.connection.execute("INSERT INTO runs(id,session_id,state,model_ref,started_at) VALUES (?1,?2,?3,?4,strftime('%Y-%m-%dT%H:%M:%fZ','now'))", params![record.id, record.session_id, record.state, record.model_ref])?;
        Ok(record)
    }

    pub fn finish_run(&self, run_id: &str, state: &str) -> Result<(), StoreError> {
        self.connection.execute("UPDATE runs SET state=?1, finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id=?2", params![state, run_id])?;
        Ok(())
    }

    pub fn recover_interrupted(&self) -> Result<usize, StoreError> {
        let changed = self.connection.execute("UPDATE runs SET state='interrupted', finished_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE state IN ('running','executing','thinking','receiving','cancelling')", [])?;
        self.connection.execute(
            "UPDATE effects SET status='unknown', side_effect='unknown' WHERE status='pending'",
            [],
        )?;
        self.connection.execute(
            "UPDATE approvals SET state='invalidated' WHERE state='pending'",
            [],
        )?;
        Ok(changed)
    }

    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionSummary>, StoreError> {
        self.search_sessions("", limit, 0)
    }

    pub fn search_sessions(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SessionSummary>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT s.id,s.title,s.created_at,s.updated_at,
                    (SELECT r.state FROM runs r
                     WHERE r.session_id=s.id
                     ORDER BY r.started_at DESC LIMIT 1)
             FROM sessions s
             WHERE (?1 = '' OR lower(s.title) LIKE '%' || lower(?1) || '%')
             ORDER BY s.updated_at DESC LIMIT ?2 OFFSET ?3",
        )?;
        let rows = statement.query_map(
            params![query.trim(), limit.clamp(1, 500) as i64, offset as i64],
            |row| {
                Ok(SessionSummary {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    created_at: row.get(2)?,
                    updated_at: row.get(3)?,
                    latest_run_state: row.get(4)?,
                })
            },
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_effect(
        &self,
        idempotency_key: &str,
        run_id: &str,
        call_id: &str,
        tool: &str,
        status: &str,
        side_effect: &str,
        data: &serde_json::Value,
    ) -> Result<bool, StoreError> {
        let data = redact_sensitive(&serde_json::to_string(data).unwrap_or_else(|_| "null".into()));
        let inserted = self.connection.execute(
            "INSERT OR IGNORE INTO effects(idempotency_key,run_id,call_id,tool,status,side_effect,data,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            params![idempotency_key, run_id, call_id, tool, status, side_effect, data],
        )?;
        Ok(inserted == 1)
    }

    pub fn begin_effect(
        &self,
        idempotency_key: &str,
        run_id: &str,
        call_id: &str,
        tool: &str,
        data: &serde_json::Value,
    ) -> Result<bool, StoreError> {
        let data = redact_sensitive(&serde_json::to_string(data).unwrap_or_else(|_| "null".into()));
        let inserted = self.connection.execute(
            "INSERT OR IGNORE INTO effects(idempotency_key,run_id,call_id,tool,status,side_effect,data,created_at) VALUES (?1,?2,?3,?4,'pending','unknown',?5,strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            params![idempotency_key, run_id, call_id, tool, data],
        )?;
        Ok(inserted == 1)
    }

    pub fn complete_effect(
        &self,
        idempotency_key: &str,
        status: &str,
        side_effect: &str,
        data: &serde_json::Value,
    ) -> Result<bool, StoreError> {
        let data = redact_sensitive(&serde_json::to_string(data).unwrap_or_else(|_| "null".into()));
        let changed = self.connection.execute(
            "UPDATE effects SET status=?1, side_effect=?2, data=?3 WHERE idempotency_key=?4 AND status='pending'",
            params![status, side_effect, data, idempotency_key],
        )?;
        Ok(changed == 1)
    }

    pub fn set_preference(
        &self,
        key: &str,
        value: &str,
        schema_version: i64,
    ) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO preferences(key,value,schema_version) VALUES (?1,?2,?3) ON CONFLICT(key) DO UPDATE SET value=excluded.value, schema_version=excluded.schema_version",
            params![key, value, schema_version],
        )?;
        Ok(())
    }

    pub fn preference(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .connection
            .query_row(
                "SELECT value FROM preferences WHERE key=?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn record_approval(&self, record: &ApprovalRecord) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO approvals(id,run_id,call_id,args_hash,scope,expires_at,state) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![
                record.id,
                record.run_id,
                record.call_id,
                record.args_hash,
                record.scope,
                record.expires_at.to_string(),
                record.state,
            ],
        )?;
        Ok(())
    }

    pub fn set_approval_state(&self, approval_id: &str, state: &str) -> Result<(), StoreError> {
        self.connection.execute(
            "UPDATE approvals SET state=?1 WHERE id=?2",
            params![state, approval_id],
        )?;
        Ok(())
    }

    pub fn approvals_for_run(&self, run_id: &str) -> Result<Vec<ApprovalRecord>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT id,run_id,call_id,args_hash,scope,expires_at,state FROM approvals WHERE run_id=?1 ORDER BY expires_at ASC",
        )?;
        let rows = statement.query_map(params![run_id], |row| {
            let expires_at = row.get::<_, String>(5)?.parse::<u64>().unwrap_or_default();
            Ok(ApprovalRecord {
                id: row.get(0)?,
                run_id: row.get(1)?,
                call_id: row.get(2)?,
                args_hash: row.get(3)?,
                scope: row.get(4)?,
                expires_at,
                state: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn effect(&self, idempotency_key: &str) -> Result<Option<EffectRecord>, StoreError> {
        Ok(self
            .connection
            .query_row(
                "SELECT idempotency_key,run_id,call_id,tool,status,side_effect,data FROM effects WHERE idempotency_key=?1",
                params![idempotency_key],
                |row| {
                    Ok(EffectRecord {
                        idempotency_key: row.get(0)?,
                        run_id: row.get(1)?,
                        call_id: row.get(2)?,
                        tool: row.get(3)?,
                        status: row.get(4)?,
                        side_effect: row.get(5)?,
                        data: row.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn export_session(&self, session_id: &str) -> Result<String, StoreError> {
        if !self.session_exists(session_id)? {
            return Err(StoreError::SessionNotFound);
        }
        let messages = self.messages(session_id)?;
        let mut statement = self.connection.prepare(
            "SELECT id,session_id,state,model_ref FROM runs WHERE session_id=?1 ORDER BY started_at ASC",
        )?;
        let runs = statement
            .query_map(params![session_id], |row| {
                Ok(RunRecord {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    state: row.get(2)?,
                    model_ref: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        serde_json::to_string(&serde_json::json!({
            "version": 1,
            "session_id": session_id,
            "messages": messages,
            "runs": runs,
            "secrets": "excluded"
        }))
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)).into())
    }

    pub fn delete_session(&self, session_id: &str) -> Result<(), StoreError> {
        let active: i64 = self.connection.query_row(
            "SELECT COUNT(*) FROM runs WHERE session_id=?1 AND state NOT IN ('completed','failed','cancelled','interrupted')",
            params![session_id],
            |row| row.get(0),
        )?;
        if active > 0 {
            return Err(StoreError::ActiveRun);
        }
        let changed = self
            .connection
            .execute("DELETE FROM sessions WHERE id=?1", params![session_id])?;
        if changed == 0 {
            return Err(StoreError::SessionNotFound);
        }
        Ok(())
    }

    pub fn prune_sessions(&self, max_age: Duration) -> Result<usize, StoreError> {
        self.prune_sessions_except(max_age, None)
    }

    pub fn prune_sessions_except(
        &self,
        max_age: Duration,
        protected_session_id: Option<&str>,
    ) -> Result<usize, StoreError> {
        let cutoff = SystemTime::now()
            .checked_sub(max_age)
            .unwrap_or(UNIX_EPOCH)
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        // SQLite's ISO timestamps sort lexically, so the cutoff is converted to
        // a database timestamp instead of comparing locale-dependent strings.
        Ok(self.connection.execute(
            "DELETE FROM sessions WHERE CAST(strftime('%s',updated_at) AS INTEGER) < ?1
             AND (?2 IS NULL OR id != ?2)
             AND id NOT IN (
                 SELECT session_id FROM runs
                 WHERE state NOT IN ('completed','failed','cancelled','interrupted')
             )",
            params![cutoff, protected_session_id],
        )?)
    }

    pub fn messages(&self, session_id: &str) -> Result<Vec<MessageRecord>, StoreError> {
        let mut statement = self.connection.prepare("SELECT id,session_id,role,content,seq FROM messages WHERE session_id=?1 ORDER BY seq ASC")?;
        let rows = statement.query_map(params![session_id], |row| {
            Ok(MessageRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                seq: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn session_exists(&self, session_id: &str) -> Result<bool, StoreError> {
        Ok(self
            .connection
            .query_row(
                "SELECT 1 FROM sessions WHERE id=?1",
                params![session_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .is_some())
    }
}

fn redact_sensitive(value: &str) -> String {
    let mut redacted = value.to_owned();
    for marker in ["Bearer ", "sk-", "api_key=", "api-key=", "token="] {
        let mut cursor = 0;
        while let Some(relative) = redacted[cursor..].find(marker) {
            let start = cursor + relative;
            let value_start = start + marker.len();
            let end = redacted[value_start..]
                .find(|character: char| {
                    character.is_whitespace() || matches!(character, '"' | '\'' | ',' | '}')
                })
                .map(|offset| value_start + offset)
                .unwrap_or(redacted.len());
            redacted.replace_range(value_start..end, "[REDACTED]");
            cursor = value_start + "[REDACTED]".len();
        }
    }
    redacted
}

fn classify_sql_error(error: rusqlite::Error) -> StoreError {
    match error.sqlite_error_code() {
        Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase) => {
            StoreError::Corrupt
        }
        _ => StoreError::Sql(error),
    }
}

fn classify_store_error(error: StoreError) -> StoreError {
    match error {
        StoreError::Sql(error) => classify_sql_error(error),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_messages_and_marks_open_runs_interrupted() {
        let store = SessionStore::open_in_memory().unwrap();
        let session = store.create_session("test").unwrap();
        store.append_message(&session, "user", "hello", 1).unwrap();
        let run = store.start_run(&session, Some("fake-text")).unwrap();
        assert_eq!(store.recover_interrupted().unwrap(), 1);
        assert_eq!(store.messages(&session).unwrap()[0].content, "hello");
        store.finish_run(&run.id, "cancelled").unwrap();
    }

    #[test]
    fn a06_secrets_are_redacted_from_messages_effects_and_exports() {
        let store = SessionStore::open_in_memory().unwrap();
        let session = store.create_session("test").unwrap();
        store
            .append_message(&session, "user", "Bearer super-secret", 1)
            .unwrap();
        assert!(store
            .record_effect(
                "effect-1",
                "run-1",
                "call-1",
                "files.write",
                "success",
                "applied",
                &serde_json::json!({"token":"secret"}),
            )
            .unwrap());
        assert!(!store
            .record_effect(
                "effect-1",
                "run-1",
                "call-1",
                "files.write",
                "success",
                "applied",
                &serde_json::json!({}),
            )
            .unwrap());
        let export = store.export_session(&session).unwrap();
        assert!(!export.contains("super-secret"));
        assert!(!export.contains("\"token\":\"secret\""));
    }

    #[test]
    fn migration_is_reopenable_and_preferences_are_durable() {
        let path = std::env::temp_dir().join(format!(
            "vox-session-store-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let store = SessionStore::open(&path).unwrap();
            assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
            store.set_preference("theme", "dark", 1).unwrap();
            store
                .record_approval(&ApprovalRecord {
                    id: "approval-1".into(),
                    run_id: "run-1".into(),
                    call_id: "call-1".into(),
                    args_hash: "hash".into(),
                    scope: "run:run-1".into(),
                    expires_at: 123,
                    state: "pending".into(),
                })
                .unwrap();
        }
        {
            let store = SessionStore::open(&path).unwrap();
            assert_eq!(store.preference("theme").unwrap().as_deref(), Some("dark"));
            assert_eq!(store.schema_version().unwrap(), CURRENT_SCHEMA_VERSION);
            assert_eq!(
                store.approvals_for_run("run-1").unwrap()[0].state,
                "invalidated"
            );
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn active_session_cannot_be_deleted_until_run_is_terminal() {
        let store = SessionStore::open_in_memory().unwrap();
        let session = store.create_session("test").unwrap();
        let run = store.start_run(&session, None).unwrap();
        assert!(matches!(
            store.delete_session(&session),
            Err(StoreError::ActiveRun)
        ));
        store.finish_run(&run.id, "completed").unwrap();
        store.delete_session(&session).unwrap();
        assert!(matches!(store.messages(&session), Ok(messages) if messages.is_empty()));
    }

    #[test]
    fn a_session_cannot_start_two_competing_runs() {
        let store = SessionStore::open_in_memory().unwrap();
        let session = store.create_session("single run").unwrap();
        let first = store.start_run(&session, None).unwrap();
        assert!(matches!(
            store.start_run(&session, None),
            Err(StoreError::ActiveRun)
        ));
        store.finish_run(&first.id, "cancelled").unwrap();
        assert!(store.start_run(&session, None).is_ok());
    }

    #[test]
    fn session_search_paginates_and_reports_latest_run_state() {
        let store = SessionStore::open_in_memory().unwrap();
        let alpha = store.create_session("Alpha").unwrap();
        let run = store.start_run(&alpha, Some("fake-text")).unwrap();
        store.finish_run(&run.id, "completed").unwrap();
        let _beta = store.create_session("Beta").unwrap();

        let page = store.search_sessions("alp", 1, 0).unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].title, "Alpha");
        assert_eq!(page[0].latest_run_state.as_deref(), Some("completed"));
        assert!(store.search_sessions("alp", 1, 1).unwrap().is_empty());
    }

    #[test]
    fn corrupt_database_has_a_recoverable_diagnosis() {
        let path = std::env::temp_dir().join(format!(
            "vox-corrupt-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"not sqlite").unwrap();
        assert!(matches!(
            SessionStore::open(&path),
            Err(StoreError::Corrupt)
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a10_crash_after_an_effect_recovers_without_replaying_it() {
        let path = std::env::temp_dir().join(format!(
            "vox-a10-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let session;
        {
            let store = SessionStore::open(&path).unwrap();
            session = store.create_session("crash test").unwrap();
            let run = store.start_run(&session, None).unwrap();
            assert!(store
                .record_effect(
                    "idempotent-effect",
                    &run.id,
                    "call",
                    "files.write",
                    "success",
                    "applied",
                    &serde_json::json!({"path":"safe"}),
                )
                .unwrap());
        }
        {
            let store = SessionStore::open(&path).unwrap();
            let export = store.export_session(&session).unwrap();
            assert!(export.contains("interrupted"));
            assert!(!store
                .record_effect(
                    "idempotent-effect",
                    "same-run",
                    "same-call",
                    "files.write",
                    "success",
                    "applied",
                    &serde_json::json!({}),
                )
                .unwrap());
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn pending_effect_is_reconciled_as_unknown_and_cannot_be_completed_twice() {
        let path = std::env::temp_dir().join(format!(
            "vox-pending-effect-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let store = SessionStore::open(&path).unwrap();
            assert!(store
                .begin_effect(
                    "effect-pending",
                    "run-pending",
                    "call-pending",
                    "files.write",
                    &serde_json::json!({"path":"safe"}),
                )
                .unwrap());
            assert!(store
                .complete_effect(
                    "effect-pending",
                    "success",
                    "applied",
                    &serde_json::json!({"path":"safe"}),
                )
                .unwrap());
            assert!(!store
                .complete_effect(
                    "effect-pending",
                    "success",
                    "applied",
                    &serde_json::json!({"path":"replay"}),
                )
                .unwrap());
            assert_eq!(
                store.effect("effect-pending").unwrap().unwrap().status,
                "success"
            );
        }
        {
            let store = SessionStore::open(&path).unwrap();
            assert!(store
                .begin_effect(
                    "effect-after-crash",
                    "run-pending",
                    "call-pending",
                    "files.write",
                    &serde_json::json!({"path":"unsafe-to-repeat"}),
                )
                .unwrap());
        }
        {
            let store = SessionStore::open(&path).unwrap();
            let effect = store.effect("effect-after-crash").unwrap().unwrap();
            assert_eq!(effect.status, "unknown");
            assert_eq!(effect.side_effect, "unknown");
            assert!(!store
                .complete_effect(
                    "effect-after-crash",
                    "success",
                    "applied",
                    &serde_json::json!({"path":"replay"}),
                )
                .unwrap());
        }
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn retention_prunes_only_terminal_sessions() {
        let store = SessionStore::open_in_memory().unwrap();
        let old = store.create_session("old").unwrap();
        let old_run = store.start_run(&old, None).unwrap();
        store.finish_run(&old_run.id, "completed").unwrap();
        let active = store.create_session("active").unwrap();
        let _active_run = store.start_run(&active, None).unwrap();
        store
            .connection
            .execute(
                "UPDATE sessions SET updated_at='2000-01-01T00:00:00.000Z'",
                [],
            )
            .unwrap();
        let removed = store.prune_sessions(Duration::from_secs(86_400)).unwrap();
        assert!(removed >= 1);
        assert!(store.messages(&active).unwrap().is_empty());
        assert!(store.search_sessions("active", 10, 0).unwrap().len() == 1);
    }

    #[test]
    fn retention_can_protect_the_current_session() {
        let store = SessionStore::open_in_memory().unwrap();
        let current = store.create_session("current").unwrap();
        let run = store.start_run(&current, None).unwrap();
        store.finish_run(&run.id, "completed").unwrap();
        store
            .connection
            .execute(
                "UPDATE sessions SET updated_at='2000-01-01T00:00:00.000Z'",
                [],
            )
            .unwrap();
        assert_eq!(
            store
                .prune_sessions_except(Duration::from_secs(86_400), Some(&current))
                .unwrap(),
            0
        );
        assert_eq!(store.search_sessions("current", 10, 0).unwrap().len(), 1);
    }
}
