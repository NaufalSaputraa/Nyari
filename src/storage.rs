//! Dedicated SQLite Actor Thread owning `history.db` and `vault.db`.
//!
//! All `rusqlite::Connection` instances are owned exclusively by one OS thread to eliminate
//! `SQLITE_BUSY` and shutdown checkpoint deadlocks.
//!
//! **Privacy invariant:** Private Tabs (`is_private == true`) MUST NEVER be recorded to
//! `history.db` via `DbActor::record_visit` (nor persisted to `session.json`).

use crate::NyariError;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const COMMAND_QUEUE_CAPACITY: usize = 256;
const DEFAULT_DROP_CHECKPOINT_TIMEOUT: Duration = Duration::from_millis(150);

const HISTORY_SCHEMA_SQL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS visits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id TEXT NOT NULL DEFAULT 'ws-default',
    url TEXT NOT NULL,
    title TEXT NOT NULL,
    visit_time INTEGER NOT NULL,
    visit_count INTEGER NOT NULL DEFAULT 1,
    favicon_blob BLOB,
    UNIQUE(workspace_id, url)
);

CREATE INDEX IF NOT EXISTS idx_visits_ws_time ON visits(workspace_id, visit_time DESC);

CREATE VIRTUAL TABLE IF NOT EXISTS visits_fts USING fts5(
    title,
    url,
    content='visits',
    content_rowid='id'
);

CREATE TRIGGER IF NOT EXISTS visits_ai AFTER INSERT ON visits BEGIN
  INSERT INTO visits_fts(rowid, title, url) VALUES (new.id, new.title, new.url);
END;

CREATE TRIGGER IF NOT EXISTS visits_ad AFTER DELETE ON visits BEGIN
  INSERT INTO visits_fts(visits_fts, rowid, title, url) VALUES('delete', old.id, old.title, old.url);
END;

CREATE TRIGGER IF NOT EXISTS visits_au AFTER UPDATE OF title, url ON visits BEGIN
  INSERT INTO visits_fts(visits_fts, rowid, title, url) VALUES('delete', old.id, old.title, old.url);
  INSERT INTO visits_fts(rowid, title, url) VALUES (new.id, new.title, new.url);
END;

CREATE TABLE IF NOT EXISTS bookmarks (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    parent_folder TEXT DEFAULT 'root',
    created_at INTEGER NOT NULL
);
"#;

const VAULT_SCHEMA_SQL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS credentials (
    id TEXT PRIMARY KEY,
    origin_domain TEXT NOT NULL,
    username_hint TEXT NOT NULL,
    payload_cipher BLOB NOT NULL,
    salt BLOB NOT NULL,
    nonce BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(origin_domain, username_hint)
);

CREATE INDEX IF NOT EXISTS idx_cred_domain ON credentials(origin_domain);
"#;

const UPSERT_VISIT_SQL: &str = r#"
INSERT INTO visits (workspace_id, url, title, visit_time, visit_count, favicon_blob)
VALUES (?1, ?2, ?3, ?4, 1, ?5)
ON CONFLICT(workspace_id, url) DO UPDATE SET
    visit_count  = visits.visit_count + 1,
    visit_time   = excluded.visit_time,
    title        = CASE WHEN excluded.title != '' THEN excluded.title ELSE visits.title END,
    favicon_blob = COALESCE(excluded.favicon_blob, visits.favicon_blob);
"#;

/// Commands dispatched to the dedicated SQLite Actor Thread.
pub enum DbCommand {
    RecordVisit {
        workspace_id: String,
        url: String,
        title: String,
        favicon: Option<Vec<u8>>,
        reply: SyncSender<Result<(), NyariError>>,
    },
    VisitCountFor {
        workspace_id: String,
        url: String,
        reply: SyncSender<Result<Option<i64>, NyariError>>,
    },
    ClearHistory {
        workspace_id: Option<String>,
        reply: SyncSender<Result<(), NyariError>>,
    },
    ShutdownAndCheckpoint {
        reply: SyncSender<Result<(), NyariError>>,
    },
}

struct ShutdownOnLastDrop {
    tx: SyncSender<DbCommand>,
    join_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl Drop for ShutdownOnLastDrop {
    fn drop(&mut self) {
        let already_stopped = self
            .join_handle
            .lock()
            .map(|guard| guard.is_none())
            .unwrap_or(true);
        if !already_stopped {
            let _ = shutdown_actor_with_timeout(
                &self.tx,
                &self.join_handle,
                DEFAULT_DROP_CHECKPOINT_TIMEOUT,
            );
        }
    }
}

/// Cloneable handle for communicating with the dedicated SQLite Actor Thread.
#[derive(Clone)]
pub struct DbActor {
    tx: SyncSender<DbCommand>,
    join_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    _shutdown_guard: Arc<ShutdownOnLastDrop>,
}

impl DbActor {
    /// Spawns a single dedicated OS thread owning `history.db` and `vault.db` in WAL mode.
    pub fn spawn(data_dir: &Path) -> Result<DbActor, NyariError> {
        std::fs::create_dir_all(data_dir)?;

        let (tx, rx) = mpsc::sync_channel::<DbCommand>(COMMAND_QUEUE_CAPACITY);
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(), NyariError>>(1);
        let dir_owned = data_dir.to_path_buf();

        let handle = thread::Builder::new()
            .name("nyari-sqlite-actor".to_string())
            .spawn(move || match open_and_init_connections(&dir_owned) {
                Ok((history_conn, vault_conn)) => {
                    let _ = init_tx.send(Ok(()));
                    run_actor_loop(history_conn, vault_conn, rx);
                }
                Err(err) => {
                    let _ = init_tx.send(Err(err));
                }
            })?;

        match init_rx.recv() {
            Ok(Ok(())) => {
                let join_handle = Arc::new(Mutex::new(Some(handle)));
                let shutdown_guard = Arc::new(ShutdownOnLastDrop {
                    tx: tx.clone(),
                    join_handle: Arc::clone(&join_handle),
                });
                Ok(Self {
                    tx,
                    join_handle,
                    _shutdown_guard: shutdown_guard,
                })
            }
            Ok(Err(err)) => {
                let _ = handle.join();
                Err(err)
            }
            Err(_) => {
                let _ = handle.join();
                Err(NyariError::Other(
                    "SQLite actor thread exited unexpectedly during initialization".to_string(),
                ))
            }
        }
    }

    /// Upserts a visited URL scoped to `workspace_id`.
    ///
    /// Callers MUST ensure Private Tabs (`is_private == true`) are never recorded here.
    pub fn record_visit(
        &self,
        workspace_id: &str,
        url: &str,
        title: &str,
        favicon: Option<&[u8]>,
    ) -> Result<(), NyariError> {
        if workspace_id.is_empty() || url.is_empty() {
            return Err(NyariError::InvalidInput(
                "workspace_id and url must be non-empty when recording a visit".to_string(),
            ));
        }

        self.send_request(|reply| DbCommand::RecordVisit {
            workspace_id: workspace_id.to_string(),
            url: url.to_string(),
            title: title.to_string(),
            favicon: favicon.map(|bytes| bytes.to_vec()),
            reply,
        })
    }

    /// Returns the `visit_count` for `(workspace_id, url)` if present.
    pub fn visit_count_for(
        &self,
        workspace_id: &str,
        url: &str,
    ) -> Result<Option<i64>, NyariError> {
        self.send_request(|reply| DbCommand::VisitCountFor {
            workspace_id: workspace_id.to_string(),
            url: url.to_string(),
            reply,
        })
    }

    /// Deletes browsing history for a specific workspace, or all visits when `workspace_id` is `None`.
    pub fn clear_history(&self, workspace_id: Option<&str>) -> Result<(), NyariError> {
        self.send_request(|reply| DbCommand::ClearHistory {
            workspace_id: workspace_id.map(str::to_string),
            reply,
        })
    }

    /// Checkpoints both `history.db` and `vault.db` (`PRAGMA wal_checkpoint(TRUNCATE)`), closes
    /// connections, and joins the actor thread within `timeout`.
    pub fn shutdown_and_checkpoint(&self, timeout: Duration) -> Result<(), NyariError> {
        shutdown_actor_with_timeout(&self.tx, &self.join_handle, timeout)
    }

    fn send_request<T>(
        &self,
        build: impl FnOnce(SyncSender<Result<T, NyariError>>) -> DbCommand,
    ) -> Result<T, NyariError> {
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        self.tx
            .send(build(reply_tx))
            .map_err(|_| NyariError::Other("SQLite actor thread has shut down".to_string()))?;
        reply_rx.recv().map_err(|_| {
            NyariError::Other("SQLite actor thread disconnected before replying".to_string())
        })?
    }
}

fn open_and_init_connections(data_dir: &Path) -> Result<(Connection, Connection), NyariError> {
    let history_path: PathBuf = data_dir.join("history.db");
    let vault_path: PathBuf = data_dir.join("vault.db");

    let history_conn = Connection::open(history_path)?;
    history_conn.execute_batch(HISTORY_SCHEMA_SQL)?;

    let vault_conn = Connection::open(vault_path)?;
    vault_conn.execute_batch(VAULT_SCHEMA_SQL)?;

    Ok((history_conn, vault_conn))
}

fn run_actor_loop(history_conn: Connection, vault_conn: Connection, rx: Receiver<DbCommand>) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            DbCommand::RecordVisit {
                workspace_id,
                url,
                title,
                favicon,
                reply,
            } => {
                let res = handle_record_visit(
                    &history_conn,
                    &workspace_id,
                    &url,
                    &title,
                    favicon.as_deref(),
                );
                let _ = reply.send(res);
            }
            DbCommand::VisitCountFor {
                workspace_id,
                url,
                reply,
            } => {
                let res = handle_visit_count_for(&history_conn, &workspace_id, &url);
                let _ = reply.send(res);
            }
            DbCommand::ClearHistory {
                workspace_id,
                reply,
            } => {
                let res = handle_clear_history(&history_conn, workspace_id.as_deref());
                let _ = reply.send(res);
            }
            DbCommand::ShutdownAndCheckpoint { reply } => {
                let res = checkpoint_and_close_both(history_conn, vault_conn);
                let _ = reply.send(res);
                return;
            }
        }
    }

    // Fallback cleanup if all senders disconnect without ShutdownAndCheckpoint.
    let _ = checkpoint_and_close_both(history_conn, vault_conn);
}

fn handle_record_visit(
    history_conn: &Connection,
    workspace_id: &str,
    url: &str,
    title: &str,
    favicon: Option<&[u8]>,
) -> Result<(), NyariError> {
    let visit_time = current_unix_ms();
    history_conn.execute(
        UPSERT_VISIT_SQL,
        params![workspace_id, url, title, visit_time, favicon],
    )?;
    Ok(())
}

fn handle_visit_count_for(
    history_conn: &Connection,
    workspace_id: &str,
    url: &str,
) -> Result<Option<i64>, NyariError> {
    let count = history_conn
        .query_row(
            "SELECT visit_count FROM visits WHERE workspace_id = ?1 AND url = ?2;",
            params![workspace_id, url],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    Ok(count)
}

fn handle_clear_history(
    history_conn: &Connection,
    workspace_id: Option<&str>,
) -> Result<(), NyariError> {
    match workspace_id {
        Some(ws) => {
            history_conn.execute("DELETE FROM visits WHERE workspace_id = ?1;", params![ws])?;
        }
        None => {
            history_conn.execute("DELETE FROM visits;", [])?;
        }
    }
    Ok(())
}

fn checkpoint_and_close_both(
    history_conn: Connection,
    vault_conn: Connection,
) -> Result<(), NyariError> {
    let mut first_err: Option<NyariError> = None;

    if let Err(err) = checkpoint_wal(&history_conn, "history.db") {
        first_err = Some(err);
    }
    if let Err(err) = checkpoint_wal(&vault_conn, "vault.db") {
        if first_err.is_none() {
            first_err = Some(err);
        }
    }

    if let Err(err) = close_connection(history_conn) {
        if first_err.is_none() {
            first_err = Some(err);
        }
    }
    if let Err(err) = close_connection(vault_conn) {
        if first_err.is_none() {
            first_err = Some(err);
        }
    }

    match first_err {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn checkpoint_wal(conn: &Connection, db_label: &str) -> Result<(), NyariError> {
    let (busy, _log_frames, _checkpointed_frames): (i32, i32, i32) =
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE);", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
    if busy != 0 {
        return Err(NyariError::Other(format!(
            "{db_label} WAL checkpoint blocked (busy={busy})"
        )));
    }
    Ok(())
}

fn close_connection(conn: Connection) -> Result<(), NyariError> {
    conn.close().map_err(|(_conn, err)| NyariError::Sqlite(err))
}

fn shutdown_actor_with_timeout(
    tx: &SyncSender<DbCommand>,
    join_handle: &Mutex<Option<JoinHandle<()>>>,
    timeout: Duration,
) -> Result<(), NyariError> {
    let mut guard = join_handle
        .lock()
        .map_err(|_| NyariError::Other("SQLite actor join handle lock poisoned".to_string()))?;
    if guard.is_none() {
        return Ok(());
    }

    let deadline = Instant::now() + timeout;
    let (reply_tx, reply_rx) = mpsc::sync_channel(1);
    let mut cmd = Some(DbCommand::ShutdownAndCheckpoint { reply: reply_tx });

    loop {
        let taken = cmd.take().expect("shutdown command available");
        match tx.try_send(taken) {
            Ok(()) => break,
            Err(TrySendError::Full(returned)) => {
                cmd = Some(returned);
                if Instant::now() >= deadline {
                    return Err(NyariError::Other(format!(
                        "SQLite shutdown queue saturated; timed out after {} ms",
                        timeout.as_millis()
                    )));
                }
                thread::sleep(Duration::from_millis(1));
            }
            Err(TrySendError::Disconnected(_)) => {
                if let Some(handle) = guard.take() {
                    handle.join().map_err(|_| {
                        NyariError::Other("SQLite actor thread panicked".to_string())
                    })?;
                }
                return Ok(());
            }
        }
    }

    let remaining = deadline.saturating_duration_since(Instant::now());
    match reply_rx.recv_timeout(remaining) {
        Ok(res) => {
            if let Some(handle) = guard.take() {
                handle
                    .join()
                    .map_err(|_| NyariError::Other("SQLite actor thread panicked".to_string()))?;
            }
            res
        }
        Err(RecvTimeoutError::Timeout) => Err(NyariError::Other(format!(
            "SQLite shutdown and checkpoint timed out after {} ms",
            timeout.as_millis()
        ))),
        Err(RecvTimeoutError::Disconnected) => {
            if let Some(handle) = guard.take() {
                handle
                    .join()
                    .map_err(|_| NyariError::Other("SQLite actor thread panicked".to_string()))?;
            }
            Err(NyariError::Other(
                "SQLite actor disconnected before completing checkpoint".to_string(),
            ))
        }
    }
}

fn current_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::DbActor;
    use crate::NyariError;
    use rusqlite::Connection;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(prefix: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("nyari_storage_{prefix}_{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).expect("create temp test dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn assert_wal_files_truncated_or_removed(dir: &Path) {
        for wal_name in ["history.db-wal", "vault.db-wal"] {
            let wal_path = dir.join(wal_name);
            if wal_path.exists() {
                let len = std::fs::metadata(&wal_path).expect("stat WAL file").len();
                assert_eq!(len, 0, "{wal_name} should be truncated to 0 bytes");
            }
        }
    }

    #[test]
    fn schema_initializes_history_and_vault_with_wal_and_fts5() {
        let dir = TestDir::new("schema");
        let actor = DbActor::spawn(dir.path()).expect("spawn DbActor");
        actor
            .record_visit(
                "ws-default",
                "https://github.com/rust-lang",
                "Rust GitHub",
                None,
            )
            .expect("record initial visit");
        actor
            .shutdown_and_checkpoint(Duration::from_millis(150))
            .expect("shutdown_and_checkpoint");

        let history_path = dir.path().join("history.db");
        let vault_path = dir.path().join("vault.db");
        assert!(history_path.exists());
        assert!(vault_path.exists());

        let history_conn = Connection::open(&history_path).expect("open history.db after close");
        let history_mode: String = history_conn
            .query_row("PRAGMA journal_mode;", [], |r| r.get(0))
            .expect("journal_mode");
        assert_eq!(history_mode.to_lowercase(), "wal");

        let fts_hits: i64 = history_conn
            .query_row(
                "SELECT count(*) FROM visits_fts WHERE visits_fts MATCH '\"rust\"*';",
                [],
                |r| r.get(0),
            )
            .expect("query visits_fts");
        assert_eq!(fts_hits, 1);

        let bookmarks_exists: i64 = history_conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='bookmarks';",
                [],
                |r| r.get(0),
            )
            .expect("check bookmarks table");
        assert_eq!(bookmarks_exists, 1);

        let vault_conn = Connection::open(&vault_path).expect("open vault.db after close");
        let vault_mode: String = vault_conn
            .query_row("PRAGMA journal_mode;", [], |r| r.get(0))
            .expect("vault journal_mode");
        assert_eq!(vault_mode.to_lowercase(), "wal");

        let creds_exists: i64 = vault_conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='credentials';",
                [],
                |r| r.get(0),
            )
            .expect("check credentials table");
        assert_eq!(creds_exists, 1);

        let cred_idx_exists: i64 = vault_conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='idx_cred_domain';",
                [],
                |r| r.get(0),
            )
            .expect("check idx_cred_domain");
        assert_eq!(cred_idx_exists, 1);
    }

    #[test]
    fn record_visit_upserts_per_workspace_and_isolates_workspaces() {
        let dir = TestDir::new("upsert");
        let actor = DbActor::spawn(dir.path()).expect("spawn DbActor");

        let favicon = [0x89u8, 0x50, 0x4E, 0x47];
        actor
            .record_visit("ws-default", "https://github.com", "GitHub", Some(&favicon))
            .expect("visit 1");
        actor
            .record_visit("ws-default", "https://github.com", "", None)
            .expect("visit 2");
        actor
            .record_visit("ws-default", "https://github.com", "GitHub Home", None)
            .expect("visit 3");
        actor
            .record_visit("ws-default", "https://github.com", "GitHub", None)
            .expect("visit 4");
        actor
            .record_visit("ws-default", "https://github.com", "", None)
            .expect("visit 5");

        actor
            .record_visit("ws-work", "https://github.com", "GitHub Work", None)
            .expect("ws-work visit");

        assert_eq!(
            actor
                .visit_count_for("ws-default", "https://github.com")
                .expect("query ws-default"),
            Some(5)
        );
        assert_eq!(
            actor
                .visit_count_for("ws-work", "https://github.com")
                .expect("query ws-work"),
            Some(1)
        );
        assert_eq!(
            actor
                .visit_count_for("ws-default", "https://missing.example")
                .expect("query missing"),
            None
        );

        actor
            .shutdown_and_checkpoint(Duration::from_millis(150))
            .expect("shutdown");

        let history_conn =
            Connection::open(dir.path().join("history.db")).expect("open history.db");
        let row_count: i64 = history_conn
            .query_row(
                "SELECT count(*) FROM visits WHERE workspace_id = 'ws-default' AND url = 'https://github.com';",
                [],
                |r| r.get(0),
            )
            .expect("count rows");
        assert_eq!(row_count, 1);

        let (title, saved_favicon): (String, Option<Vec<u8>>) = history_conn
            .query_row(
                "SELECT title, favicon_blob FROM visits WHERE workspace_id = 'ws-default' AND url = 'https://github.com';",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("select title and favicon");
        assert_eq!(title, "GitHub");
        assert_eq!(saved_favicon.as_deref(), Some(&favicon[..]));
    }

    #[test]
    fn clear_history_scopes_by_workspace_or_clears_all() {
        let dir = TestDir::new("clear");
        let actor = DbActor::spawn(dir.path()).expect("spawn DbActor");

        actor
            .record_visit("ws-default", "https://github.com", "GitHub", None)
            .expect("ws-default visit");
        actor
            .record_visit("ws-work", "https://gitlab.com", "GitLab", None)
            .expect("ws-work visit");

        actor
            .clear_history(Some("ws-default"))
            .expect("clear ws-default history");

        assert_eq!(
            actor
                .visit_count_for("ws-default", "https://github.com")
                .expect("query ws-default after clear"),
            None
        );
        assert_eq!(
            actor
                .visit_count_for("ws-work", "https://gitlab.com")
                .expect("query ws-work after ws-default clear"),
            Some(1)
        );

        actor.clear_history(None).expect("clear all history");
        assert_eq!(
            actor
                .visit_count_for("ws-work", "https://gitlab.com")
                .expect("query ws-work after global clear"),
            None
        );

        actor
            .shutdown_and_checkpoint(Duration::from_millis(150))
            .expect("shutdown");

        let history_conn =
            Connection::open(dir.path().join("history.db")).expect("open history.db");
        let remaining_fts: i64 = history_conn
            .query_row("SELECT count(*) FROM visits_fts;", [], |r| r.get(0))
            .expect("count remaining visits_fts rows");
        assert_eq!(remaining_fts, 0);
    }

    #[test]
    fn shutdown_and_checkpoint_truncates_wals_within_150ms_without_busy() {
        let dir = TestDir::new("checkpoint");
        let actor = DbActor::spawn(dir.path()).expect("spawn DbActor");

        for i in 0..25 {
            actor
                .record_visit(
                    "ws-default",
                    &format!("https://example.com/page/{i}"),
                    &format!("Page {i}"),
                    None,
                )
                .expect("record visit");
        }

        let start = Instant::now();
        actor
            .shutdown_and_checkpoint(Duration::from_millis(150))
            .expect("shutdown_and_checkpoint within 150ms");
        let elapsed = start.elapsed();
        assert!(
            elapsed <= Duration::from_millis(150),
            "checkpoint took {elapsed:?}, exceeding 150ms budget"
        );

        assert_wal_files_truncated_or_removed(dir.path());

        // Idempotent second shutdown succeeds; subsequent operations fail cleanly.
        actor
            .shutdown_and_checkpoint(Duration::from_millis(150))
            .expect("second shutdown_and_checkpoint is idempotent");
        assert!(actor
            .record_visit("ws-default", "https://after.example", "After", None)
            .is_err());
    }

    #[test]
    fn cloned_actor_stays_alive_until_last_drop_and_drop_checkpoints_cleanly() {
        let dir = TestDir::new("clone_drop");
        let actor = DbActor::spawn(dir.path()).expect("spawn DbActor");
        let clone = actor.clone();

        clone
            .record_visit("ws-default", "https://clone.example", "Clone", None)
            .expect("record via clone");
        drop(clone);

        // Original handle must still function after clone drops.
        assert_eq!(
            actor
                .visit_count_for("ws-default", "https://clone.example")
                .expect("query via original"),
            Some(1)
        );
        actor
            .record_visit("ws-default", "https://clone.example", "Clone", None)
            .expect("record via original after clone drop");

        drop(actor);
        assert_wal_files_truncated_or_removed(dir.path());

        let conn = Connection::open(dir.path().join("history.db")).expect("open after drop");
        let count: i64 = conn
            .query_row(
                "SELECT visit_count FROM visits WHERE workspace_id = 'ws-default' AND url = 'https://clone.example';",
                [],
                |r| r.get(0),
            )
            .expect("visit persisted by Drop checkpoint");
        assert_eq!(count, 2);
    }

    #[test]
    fn record_visit_rejects_empty_workspace_or_url() {
        let dir = TestDir::new("invalid");
        let actor = DbActor::spawn(dir.path()).expect("spawn DbActor");

        let err1 = actor.record_visit("", "https://example.com", "Title", None);
        assert!(matches!(err1, Err(NyariError::InvalidInput(_))));

        let err2 = actor.record_visit("ws-default", "", "Title", None);
        assert!(matches!(err2, Err(NyariError::InvalidInput(_))));
    }
}
