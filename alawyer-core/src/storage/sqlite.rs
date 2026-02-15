use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use uuid::Uuid;

use crate::error::{CoreError, CoreResult};

#[derive(Debug, Clone, uniffi::Record)]
pub struct Session {
    pub id: String,
    pub title: Option<String>,
    pub scenario: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub status: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub content: String,
    pub phase: Option<String>,
    pub tool_calls: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct LogEntry {
    pub id: i64,
    pub level: String,
    pub message: String,
    pub session_id: Option<String>,
    pub created_at: i64,
}

pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

impl SqliteStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> CoreResult<Self> {
        let conn = Connection::open(path).map_err(|e| CoreError::Storage(e.to_string()))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| CoreError::Storage(e.to_string()))?;
        migrate(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn create_session(&self, scenario: &str, title: Option<&str>) -> CoreResult<Session> {
        let now = Utc::now().timestamp();
        let session = Session {
            id: Uuid::new_v4().to_string(),
            title: title.map(ToOwned::to_owned),
            scenario: scenario.to_owned(),
            created_at: now,
            updated_at: now,
            status: "active".to_owned(),
        };

        let conn = self
            .conn
            .lock()
            .map_err(|_| CoreError::Storage("storage lock poisoned".to_owned()))?;
        conn.execute(
            "INSERT INTO sessions (id, title, scenario, created_at, updated_at, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session.id,
                session.title,
                session.scenario,
                session.created_at,
                session.updated_at,
                session.status
            ],
        )
        .map_err(|e| CoreError::Storage(e.to_string()))?;

        Ok(session)
    }

    pub fn list_sessions(&self) -> CoreResult<Vec<Session>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CoreError::Storage("storage lock poisoned".to_owned()))?;

        let mut stmt = conn
            .prepare(
                "SELECT id, title, scenario, created_at, updated_at, status
                 FROM sessions ORDER BY updated_at DESC",
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        let sessions = stmt
            .query_map([], |row| {
                Ok(Session {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    scenario: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    status: row.get(5)?,
                })
            })
            .map_err(|e| CoreError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        Ok(sessions)
    }

    pub fn get_session(&self, session_id: &str) -> CoreResult<Option<Session>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CoreError::Storage("storage lock poisoned".to_owned()))?;

        conn.query_row(
            "SELECT id, title, scenario, created_at, updated_at, status
             FROM sessions WHERE id = ?1",
            params![session_id],
            |row| {
                Ok(Session {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    scenario: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    status: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(|e| CoreError::Storage(e.to_string()))
    }

    pub fn update_session_title(&self, session_id: &str, title: &str) -> CoreResult<()> {
        let now = Utc::now().timestamp();
        let conn = self
            .conn
            .lock()
            .map_err(|_| CoreError::Storage("storage lock poisoned".to_owned()))?;

        let updated = conn
            .execute(
                "UPDATE sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
                params![title, now, session_id],
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        if updated == 0 {
            return Err(CoreError::NotFound(format!("session {session_id}")));
        }
        Ok(())
    }

    pub fn delete_session(&self, session_id: &str) -> CoreResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CoreError::Storage("storage lock poisoned".to_owned()))?;

        let deleted = conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![session_id])
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        if deleted == 0 {
            return Err(CoreError::NotFound(format!("session {session_id}")));
        }
        Ok(())
    }

    pub fn create_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        phase: Option<&str>,
        tool_calls: Option<&Value>,
    ) -> CoreResult<Message> {
        let now = Utc::now().timestamp();
        let message = Message {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.to_owned(),
            role: role.to_owned(),
            content: content.to_owned(),
            phase: phase.map(ToOwned::to_owned),
            tool_calls: tool_calls.map(|value| value.to_string()),
            created_at: now,
        };

        let conn = self
            .conn
            .lock()
            .map_err(|_| CoreError::Storage("storage lock poisoned".to_owned()))?;

        // Check session exists within the same lock scope to avoid double-lock
        let session_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
                params![session_id],
                |row| row.get(0),
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        if !session_exists {
            return Err(CoreError::NotFound(format!("session {session_id}")));
        }

        conn.execute(
            "INSERT INTO messages (id, session_id, role, content, phase, tool_calls, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                message.id,
                message.session_id,
                message.role,
                message.content,
                message.phase,
                message.tool_calls,
                message.created_at,
            ],
        )
        .map_err(|e| CoreError::Storage(e.to_string()))?;

        conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![now, session_id],
        )
        .map_err(|e| CoreError::Storage(e.to_string()))?;

        Ok(message)
    }

    pub fn get_messages(&self, session_id: &str) -> CoreResult<Vec<Message>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CoreError::Storage("storage lock poisoned".to_owned()))?;

        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, role, content, phase, tool_calls, created_at
                 FROM messages WHERE session_id = ?1 ORDER BY created_at ASC",
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        let messages = stmt
            .query_map(params![session_id], |row| {
                Ok(Message {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    phase: row.get(4)?,
                    tool_calls: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })
            .map_err(|e| CoreError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        Ok(messages)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> CoreResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CoreError::Storage("storage lock poisoned".to_owned()))?;

        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> CoreResult<Option<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CoreError::Storage("storage lock poisoned".to_owned()))?;

        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| CoreError::Storage(e.to_string()))
    }

    pub fn set_tool_permission(&self, tool_name: &str, permission: &str) -> CoreResult<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CoreError::Storage("storage lock poisoned".to_owned()))?;

        conn.execute(
            "INSERT INTO tool_permissions (tool_name, permission) VALUES (?1, ?2)
             ON CONFLICT(tool_name) DO UPDATE SET permission = excluded.permission",
            params![tool_name, permission],
        )
        .map_err(|e| CoreError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn get_tool_permission(&self, tool_name: &str) -> CoreResult<String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CoreError::Storage("storage lock poisoned".to_owned()))?;

        let permission = conn
            .query_row(
                "SELECT permission FROM tool_permissions WHERE tool_name = ?1",
                params![tool_name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| CoreError::Storage(e.to_string()))?
            .unwrap_or_else(|| default_permission_for_tool(tool_name).to_owned());

        Ok(permission)
    }

    pub fn append_log(
        &self,
        level: &str,
        message: &str,
        session_id: Option<&str>,
    ) -> CoreResult<i64> {
        let now = Utc::now().timestamp();
        let conn = self
            .conn
            .lock()
            .map_err(|_| CoreError::Storage("storage lock poisoned".to_owned()))?;

        conn.execute(
            "INSERT INTO logs (level, message, session_id, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![level, message, session_id, now],
        )
        .map_err(|e| CoreError::Storage(e.to_string()))?;

        Ok(conn.last_insert_rowid())
    }

    pub fn list_logs(&self, limit: u32) -> CoreResult<Vec<LogEntry>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| CoreError::Storage("storage lock poisoned".to_owned()))?;

        let mut stmt = conn
            .prepare(
                "SELECT id, level, message, session_id, created_at
                 FROM logs ORDER BY id DESC LIMIT ?1",
            )
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        let logs = stmt
            .query_map(params![limit], |row| {
                Ok(LogEntry {
                    id: row.get(0)?,
                    level: row.get(1)?,
                    message: row.get(2)?,
                    session_id: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|e| CoreError::Storage(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Storage(e.to_string()))?;

        Ok(logs)
    }
}

fn migrate(conn: &Connection) -> CoreResult<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            title TEXT,
            scenario TEXT NOT NULL DEFAULT 'labor',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'active'
        );

        CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            phase TEXT,
            tool_calls TEXT,
            created_at INTEGER NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tool_permissions (
            tool_name TEXT PRIMARY KEY,
            permission TEXT NOT NULL DEFAULT 'ask'
        );

        CREATE TABLE IF NOT EXISTS logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            level TEXT NOT NULL,
            message TEXT NOT NULL,
            session_id TEXT,
            created_at INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);
        CREATE INDEX IF NOT EXISTS idx_logs_created ON logs(created_at);
        "#,
    )
    .map_err(|e| CoreError::Storage(e.to_string()))?;

    Ok(())
}

fn default_permission_for_tool(tool_name: &str) -> &'static str {
    match tool_name {
        "cite" | "summarize_facts" | "check_safety" | "suggest_escalation" => "allow",
        _ => "ask",
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::SqliteStorage;

    fn make_storage() -> (TempDir, SqliteStorage) {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("core.db");
        let storage = SqliteStorage::new(db_path).expect("storage");
        (temp_dir, storage)
    }

    #[test]
    fn session_crud_works() {
        let (_temp_dir, storage) = make_storage();

        let created = storage
            .create_session("labor", Some("工资拖欠"))
            .expect("create session");
        let listed = storage.list_sessions().expect("list sessions");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, created.id);

        storage
            .update_session_title(&created.id, "新标题")
            .expect("update title");
        let updated = storage
            .get_session(&created.id)
            .expect("get")
            .expect("session exists");
        assert_eq!(updated.title.as_deref(), Some("新标题"));

        storage.delete_session(&created.id).expect("delete session");
        let empty = storage.list_sessions().expect("list sessions");
        assert!(empty.is_empty());
    }

    #[test]
    fn message_crud_works() {
        let (_temp_dir, storage) = make_storage();
        let session = storage
            .create_session("labor", Some("测试"))
            .expect("create session");

        storage
            .create_message(&session.id, "user", "hello", Some("plan"), None)
            .expect("create message");

        let messages = storage.get_messages(&session.id).expect("list messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "hello");
        assert_eq!(messages[0].phase.as_deref(), Some("plan"));
    }

    #[test]
    fn settings_kv_works() {
        let (_temp_dir, storage) = make_storage();
        storage
            .set_setting("kb_version", "2026.02")
            .expect("set setting");
        let value = storage.get_setting("kb_version").expect("get setting");
        assert_eq!(value.as_deref(), Some("2026.02"));

        let missing = storage.get_setting("missing").expect("missing key");
        assert!(missing.is_none());
    }

    #[test]
    fn tool_permission_default_is_ask() {
        let (_temp_dir, storage) = make_storage();
        let permission = storage
            .get_tool_permission("kb_search")
            .expect("get default permission");
        assert_eq!(permission, "ask");

        let allow_by_default = storage
            .get_tool_permission("check_safety")
            .expect("get allow default permission");
        assert_eq!(allow_by_default, "allow");

        storage
            .set_tool_permission("kb_search", "allow")
            .expect("set permission");
        let updated = storage
            .get_tool_permission("kb_search")
            .expect("get updated permission");
        assert_eq!(updated, "allow");
    }

    #[test]
    fn cascade_delete_messages() {
        let (_temp_dir, storage) = make_storage();
        let session = storage
            .create_session("labor", Some("删除测试"))
            .expect("create session");
        storage
            .create_message(&session.id, "user", "test", None, None)
            .expect("create message");

        storage.delete_session(&session.id).expect("delete session");
        let messages = storage.get_messages(&session.id).expect("list messages");
        assert!(messages.is_empty());
    }
}
