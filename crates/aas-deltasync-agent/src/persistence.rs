//! `SQLite` persistence layer.

use rusqlite::{Connection, OptionalExtension, Result as SqliteResult};
use std::path::Path;

/// `SQLite`-backed persistence store.
pub struct SqliteStore {
    conn: Connection,
}

#[allow(dead_code)]
impl SqliteStore {
    /// Open or create a `SQLite` database.
    ///
    /// # Errors
    ///
    /// Returns error if database cannot be opened or initialized.
    pub fn open(path: &Path) -> SqliteResult<Self> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// Create an in-memory database (for testing).
    ///
    /// # Errors
    ///
    /// Returns error if database cannot be created.
    pub fn in_memory() -> SqliteResult<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// Initialize database schema.
    fn init_schema(&self) -> SqliteResult<()> {
        self.conn.execute_batch(
            r"
            -- State snapshots for each document
            CREATE TABLE IF NOT EXISTS doc_snapshots (
                doc_id TEXT PRIMARY KEY,
                snapshot_bytes BLOB NOT NULL,
                snapshot_clock BLOB NOT NULL,
                created_at INTEGER NOT NULL
            );

            -- Delta log
            CREATE TABLE IF NOT EXISTS delta_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                doc_id TEXT NOT NULL,
                delta_id BLOB NOT NULL,
                delta_bytes BLOB NOT NULL,
                actor_id TEXT NOT NULL,
                hlc_ts INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                UNIQUE(doc_id, delta_id)
            );

            CREATE INDEX IF NOT EXISTS idx_delta_log_doc_id ON delta_log(doc_id);
            CREATE INDEX IF NOT EXISTS idx_delta_log_hlc ON delta_log(hlc_ts);

            -- Peer progress tracking
            CREATE TABLE IF NOT EXISTS peer_progress (
                peer_id TEXT NOT NULL,
                doc_id TEXT NOT NULL,
                last_ack_delta_id BLOB,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (peer_id, doc_id)
            );
            ",
        )?;

        Ok(())
    }

    /// Save a delta to the log.
    ///
    /// # Errors
    ///
    /// Returns error if insert fails.
    pub fn save_delta(
        &self,
        doc_id: &str,
        delta_id: &[u8],
        delta_bytes: &[u8],
        actor_id: &str,
        hlc_ts: u64,
    ) -> SqliteResult<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let now_i64 = to_i64(now)?;
        let hlc_ts_i64 = to_i64(hlc_ts)?;

        self.conn.execute(
            r"
            INSERT OR REPLACE INTO delta_log (doc_id, delta_id, delta_bytes, actor_id, hlc_ts, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ",
            (doc_id, delta_id, delta_bytes, actor_id, hlc_ts_i64, now_i64),
        )?;

        Ok(())
    }

    /// Get deltas for a document after a given timestamp.
    ///
    /// # Errors
    ///
    /// Returns error if query fails.
    pub fn get_deltas_after(&self, doc_id: &str, after_ts: u64) -> SqliteResult<Vec<Vec<u8>>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT delta_bytes FROM delta_log
            WHERE doc_id = ?1 AND hlc_ts > ?2
            ORDER BY hlc_ts ASC
            ",
        )?;

        let deltas = stmt
            .query_map((doc_id, to_i64(after_ts)?), |row| row.get(0))?
            .collect::<SqliteResult<Vec<Vec<u8>>>>()?;

        Ok(deltas)
    }

    /// Save a document snapshot.
    ///
    /// # Errors
    ///
    /// Returns error if insert fails.
    pub fn save_snapshot(
        &self,
        doc_id: &str,
        snapshot_bytes: &[u8],
        clock_bytes: &[u8],
    ) -> SqliteResult<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let now_i64 = to_i64(now)?;

        self.conn.execute(
            r"
            INSERT OR REPLACE INTO doc_snapshots (doc_id, snapshot_bytes, snapshot_clock, created_at)
            VALUES (?1, ?2, ?3, ?4)
            ",
            (doc_id, snapshot_bytes, clock_bytes, now_i64),
        )?;

        Ok(())
    }

    /// Get the latest snapshot for a document.
    ///
    /// # Errors
    ///
    /// Returns error if query fails.
    pub fn get_snapshot(&self, doc_id: &str) -> SqliteResult<Option<(Vec<u8>, Vec<u8>)>> {
        let mut stmt = self.conn.prepare(
            r"
            SELECT snapshot_bytes, snapshot_clock FROM doc_snapshots
            WHERE doc_id = ?1
            ",
        )?;

        let result = stmt
            .query_row([doc_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;

        Ok(result)
    }

    /// Delete deltas before a given timestamp (compaction).
    ///
    /// # Errors
    ///
    /// Returns error if delete fails.
    pub fn compact_deltas_before(&self, doc_id: &str, before_ts: u64) -> SqliteResult<usize> {
        let deleted = self.conn.execute(
            r"
            DELETE FROM delta_log
            WHERE doc_id = ?1 AND hlc_ts < ?2
            ",
            (doc_id, to_i64(before_ts)?),
        )?;

        Ok(deleted)
    }

    /// Update peer progress.
    ///
    /// # Errors
    ///
    /// Returns error if update fails.
    #[allow(dead_code)]
    pub fn update_peer_progress(
        &self,
        peer_id: &str,
        doc_id: &str,
        last_delta_id: &[u8],
    ) -> SqliteResult<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let now_i64 = to_i64(now)?;

        self.conn.execute(
            r"
            INSERT OR REPLACE INTO peer_progress (peer_id, doc_id, last_ack_delta_id, updated_at)
            VALUES (?1, ?2, ?3, ?4)
            ",
            (peer_id, doc_id, last_delta_id, now_i64),
        )?;

        Ok(())
    }
}

fn to_i64(value: u64) -> SqliteResult<i64> {
    i64::try_from(value).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_store_basic_operations() {
        let store = SqliteStore::in_memory().unwrap();

        // Save a delta
        store
            .save_delta("doc1", b"delta1", b"payload1", "actor1", 1000)
            .unwrap();
        store
            .save_delta("doc1", b"delta2", b"payload2", "actor1", 2000)
            .unwrap();

        // Get deltas after timestamp
        let deltas = store.get_deltas_after("doc1", 1000).unwrap();
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0], b"payload2");

        // Save and retrieve snapshot
        store.save_snapshot("doc1", b"snapshot", b"clock").unwrap();
        let (snap, clock) = store.get_snapshot("doc1").unwrap().unwrap();
        assert_eq!(snap, b"snapshot");
        assert_eq!(clock, b"clock");

        // Compact
        let deleted = store.compact_deltas_before("doc1", 1500).unwrap();
        assert_eq!(deleted, 1);
    }
}
