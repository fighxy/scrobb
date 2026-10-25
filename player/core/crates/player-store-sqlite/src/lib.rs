//! Хранилища ядра на SQLite (`rusqlite` со встроенной SQLite: одна версия
//! на всех платформах, на Android — без системной).
//!
//! История — отдельный файл `history.db`, не в базе библиотеки: библиотеку
//! можно пересканировать, историю — нет, поэтому её бэкапят, экспортируют
//! и мигрируют отдельно (`docs/player-history.md`). Плейлисты — в `user.db`
//! ([`SqlitePlaylists`], `docs/playlists.md`) по той же причине.
//!
//! Режим WAL, один писатель и параллельные читатели; версия схемы —
//! `PRAGMA user_version`, миграции — по порядку, каждая в своей транзакции.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use player_domain::TrackId;
use player_history::{
    DeliveryState, HistoryError, HistoryStore, NewPlay, ORIGIN_PLAYER, Outcome, PlayId, PlayRecord,
    QUEUE_MAX_AGE_MS, QUEUE_MAX_ITEMS, Settlement, TrackSnapshot,
};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Row, params};

mod playlists;

pub use playlists::SqlitePlaylists;

/// Миграции схемы истории: элемент `i` переводит базу с версии `i` на `i + 1`.
const MIGRATIONS: &[&str] = &[
    // v1 — `docs/player-history.md`, «Схема».
    "
    CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;

    CREATE TABLE plays (
      id             TEXT PRIMARY KEY,
      started_at_ms  INTEGER NOT NULL,
      ended_at_ms    INTEGER,
      played_ms      INTEGER NOT NULL,
      outcome        TEXT NOT NULL CHECK (outcome IN
                       ('playing','finished','skipped','stopped','error','interrupted')),
      scrobbled      INTEGER NOT NULL DEFAULT 0,
      title          TEXT NOT NULL,
      artists        TEXT NOT NULL,
      album          TEXT,
      album_artist   TEXT,
      duration_ms    INTEGER,
      recording_mbid TEXT,
      ext_ids        TEXT,
      track_id       INTEGER,
      device_id      TEXT NOT NULL,
      origin         TEXT NOT NULL DEFAULT 'player',
      deleted_at_ms  INTEGER
    ) STRICT;
    CREATE INDEX plays_by_time  ON plays (started_at_ms);
    CREATE INDEX plays_by_track ON plays (track_id, started_at_ms);

    CREATE TABLE deliveries (
      play_id       TEXT NOT NULL REFERENCES plays (id) ON DELETE CASCADE,
      target        TEXT NOT NULL,
      state         TEXT NOT NULL CHECK (state IN
                      ('pending','accepted','duplicate','rejected','expired')),
      reason        TEXT,
      updated_at_ms INTEGER NOT NULL,
      PRIMARY KEY (play_id, target)
    ) STRICT, WITHOUT ROWID;
    CREATE INDEX deliveries_pending ON deliveries (target) WHERE state = 'pending';
    ",
];

/// Колонки записи истории для [`read_play`]; таблица `plays` — под псевдонимом `p`.
const PLAY_COLUMNS: &str = "p.id, p.started_at_ms, p.ended_at_ms, p.played_ms, p.outcome, \
     p.scrobbled, p.title, p.artists, p.album, p.album_artist, p.duration_ms, \
     p.recording_mbid, p.ext_ids, p.track_id, p.device_id, p.origin";

/// Ждущие отправки в `?1`: не удалённые и в состоянии `pending`.
const PENDING: &str = "deliveries d JOIN plays p ON p.id = d.play_id \
     WHERE d.target = ?1 AND d.state = 'pending' AND p.deleted_at_ms IS NULL";

/// История прослушиваний в SQLite.
#[derive(Debug)]
pub struct SqliteHistory {
    conn: Mutex<Connection>,
    device_id: String,
}

impl SqliteHistory {
    /// Открыть или создать файл истории.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, HistoryError> {
        Self::init(Connection::open(path).map_err(err)?)
    }

    /// История в памяти: для тестов.
    pub fn open_in_memory() -> Result<Self, HistoryError> {
        Self::init(Connection::open_in_memory().map_err(err)?)
    }

    /// Устройство, на котором создана база; пишется в каждую запись.
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    fn init(mut conn: Connection) -> Result<Self, HistoryError> {
        configure(&conn).map_err(err)?;
        migrate(&mut conn, MIGRATIONS).map_err(HistoryError)?;
        let device_id = device_id(&conn).map_err(err)?;
        Ok(Self {
            conn: Mutex::new(conn),
            device_id,
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// WAL, `synchronous=NORMAL`, внешние ключи, ожидание блокировки.
fn configure(conn: &Connection) -> rusqlite::Result<()> {
    // В памяти WAL не бывает: SQLite отвечает `memory`, это не ошибка.
    conn.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get::<_, String>(0))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(Duration::from_secs(5))
}

/// Миграции по `PRAGMA user_version`; базу новее приложения не трогаем.
fn migrate(conn: &mut Connection, migrations: &[&str]) -> Result<(), String> {
    let s = |e: rusqlite::Error| e.to_string();
    let version: usize = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(s)?;
    if version > migrations.len() {
        return Err(format!(
            "база версии {version} новее приложения (знает до {})",
            migrations.len()
        ));
    }
    for (i, sql) in migrations.iter().enumerate().skip(version) {
        let tx = conn.transaction().map_err(s)?;
        tx.execute_batch(sql).map_err(s)?;
        tx.pragma_update(None, "user_version", i + 1).map_err(s)?;
        tx.commit().map_err(s)?;
    }
    Ok(())
}

/// Идентификатор устройства: создаётся при первом открытии и дальше не меняется.
fn device_id(conn: &Connection) -> rusqlite::Result<String> {
    let stored = conn
        .query_row("SELECT value FROM meta WHERE key = 'device_id'", [], |r| {
            r.get(0)
        })
        .optional()?;
    if let Some(id) = stored {
        return Ok(id);
    }
    let mut b: Vec<u8> = conn.query_row("SELECT randomblob(16)", [], |r| r.get(0))?;
    b[6] = (b[6] & 0x0f) | 0x40; // UUID версии 4
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    let id = format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    );
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('device_id', ?1)",
        [&id],
    )?;
    Ok(id)
}

fn err(e: rusqlite::Error) -> HistoryError {
    HistoryError(e.to_string())
}

fn not_found(id: &PlayId) -> HistoryError {
    HistoryError(format!("нет прослушивания {}", id.0))
}

/// `LIMIT` для SQLite: `-1` — без ограничения.
fn limit(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(-1)
}

fn bad_column(idx: usize, e: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(e))
}

fn read_play(r: &Row<'_>) -> rusqlite::Result<PlayRecord> {
    let outcome: String = r.get(4)?;
    let outcome = Outcome::parse(&outcome)
        .ok_or_else(|| bad_column(4, HistoryError(format!("неизвестный исход {outcome}"))))?;
    let artists: String = r.get(7)?;
    let ext_ids: Option<String> = r.get(12)?;
    Ok(PlayRecord {
        id: PlayId(r.get(0)?),
        started_at_ms: r.get(1)?,
        ended_at_ms: r.get(2)?,
        played_ms: r.get(3)?,
        outcome,
        scrobbled: r.get(5)?,
        track: TrackSnapshot {
            title: r.get(6)?,
            artists: serde_json::from_str(&artists).map_err(|e| bad_column(7, e))?,
            album: r.get(8)?,
            album_artist: r.get(9)?,
            duration_ms: r.get(10)?,
            recording_mbid: r.get(11)?,
            ext_ids: match ext_ids {
                Some(json) => serde_json::from_str(&json).map_err(|e| bad_column(12, e))?,
                None => Default::default(),
            },
        },
        track_id: r.get::<_, Option<u64>>(13)?.map(TrackId),
        device_id: r.get(14)?,
        origin: r.get(15)?,
    })
}

impl HistoryStore for SqliteHistory {
    fn begin(&self, play: &NewPlay) -> Result<(), HistoryError> {
        let t = &play.track;
        let artists = serde_json::to_string(&t.artists).map_err(|e| HistoryError(e.to_string()))?;
        let ext_ids = if t.ext_ids.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&t.ext_ids).map_err(|e| HistoryError(e.to_string()))?)
        };
        self.conn()
            .execute(
                "INSERT INTO plays (id, started_at_ms, played_ms, outcome, title, artists, album, \
                 album_artist, duration_ms, recording_mbid, ext_ids, track_id, device_id, origin) \
                 VALUES (?1, ?2, 0, 'playing', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    play.id.0,
                    play.started_at_ms,
                    t.title,
                    artists,
                    t.album,
                    t.album_artist,
                    t.duration_ms,
                    t.recording_mbid,
                    ext_ids,
                    play.track_id.map(|id| id.0),
                    self.device_id,
                    ORIGIN_PLAYER,
                ],
            )
            .map_err(err)?;
        Ok(())
    }

    fn checkpoint(&self, id: &PlayId, played_ms: u64) -> Result<(), HistoryError> {
        let changed = self
            .conn()
            .execute(
                "UPDATE plays SET played_ms = ?2 WHERE id = ?1",
                params![id.0, played_ms],
            )
            .map_err(err)?;
        if changed == 0 {
            return Err(not_found(id));
        }
        Ok(())
    }

    fn mark_scrobbled(
        &self,
        id: &PlayId,
        played_ms: u64,
        targets: &[&str],
        now_ms: i64,
    ) -> Result<(), HistoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(err)?;
        let changed = tx
            .execute(
                "UPDATE plays SET scrobbled = 1, played_ms = ?2 WHERE id = ?1",
                params![id.0, played_ms],
            )
            .map_err(err)?;
        if changed == 0 {
            return Err(not_found(id));
        }
        for target in targets {
            // Уже доставленное или отклонённое в очередь не возвращается.
            tx.execute(
                "INSERT OR IGNORE INTO deliveries (play_id, target, state, updated_at_ms) \
                 VALUES (?1, ?2, 'pending', ?3)",
                params![id.0, target, now_ms],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    fn finish(
        &self,
        id: &PlayId,
        ended_at_ms: i64,
        played_ms: u64,
        outcome: Outcome,
    ) -> Result<(), HistoryError> {
        let changed = self
            .conn()
            .execute(
                "UPDATE plays SET ended_at_ms = ?2, played_ms = ?3, outcome = ?4 WHERE id = ?1",
                params![id.0, ended_at_ms, played_ms, outcome.as_str()],
            )
            .map_err(err)?;
        if changed == 0 {
            return Err(not_found(id));
        }
        Ok(())
    }

    fn recover_interrupted(&self) -> Result<usize, HistoryError> {
        self.conn()
            .execute(
                "UPDATE plays SET outcome = 'interrupted', ended_at_ms = started_at_ms + played_ms \
                 WHERE outcome = 'playing'",
                [],
            )
            .map_err(err)
    }

    fn pending(&self, target: &str, n: usize) -> Result<Vec<PlayRecord>, HistoryError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare_cached(&format!(
                "SELECT {PLAY_COLUMNS} FROM {PENDING} ORDER BY p.started_at_ms, p.id LIMIT ?2"
            ))
            .map_err(err)?;
        let rows = stmt
            .query_map(params![target, limit(n)], read_play)
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }

    fn pending_count(&self, target: &str) -> Result<usize, HistoryError> {
        self.conn()
            .query_row(&format!("SELECT count(*) FROM {PENDING}"), [target], |r| {
                r.get(0)
            })
            .map_err(err)
    }

    fn settle(
        &self,
        target: &str,
        results: &[Settlement],
        now_ms: i64,
    ) -> Result<(), HistoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(err)?;
        for r in results {
            tx.execute(
                "UPDATE deliveries SET state = ?3, reason = ?4, updated_at_ms = ?5 \
                 WHERE play_id = ?1 AND target = ?2",
                params![r.id.0, target, r.state.as_str(), r.reason, now_ms],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    fn expire(&self, target: &str, now_ms: i64) -> Result<usize, HistoryError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(err)?;
        let expire_where = |condition: &str, args: &[&dyn rusqlite::ToSql]| {
            tx.execute(
                &format!(
                    "UPDATE deliveries SET state = '{}', updated_at_ms = ?2 \
                     WHERE target = ?1 AND play_id IN (SELECT p.id FROM {PENDING} {condition})",
                    DeliveryState::Expired.as_str()
                ),
                args,
            )
        };
        let old = expire_where(
            "AND p.started_at_ms < ?3",
            params![target, now_ms, now_ms - QUEUE_MAX_AGE_MS],
        )
        .map_err(err)?;
        let pending: usize = tx
            .query_row(&format!("SELECT count(*) FROM {PENDING}"), [target], |r| {
                r.get(0)
            })
            .map_err(err)?;
        let excess = pending.saturating_sub(QUEUE_MAX_ITEMS);
        let crowded = if excess > 0 {
            expire_where(
                "ORDER BY p.started_at_ms, p.id LIMIT ?3",
                params![target, now_ms, limit(excess)],
            )
            .map_err(err)?
        } else {
            0
        };
        tx.commit().map_err(err)?;
        Ok(old + crowded)
    }

    fn recent(&self, before_ms: Option<i64>, n: usize) -> Result<Vec<PlayRecord>, HistoryError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare_cached(&format!(
                "SELECT {PLAY_COLUMNS} FROM plays p \
                 WHERE p.deleted_at_ms IS NULL AND (?1 IS NULL OR p.started_at_ms < ?1) \
                 ORDER BY p.started_at_ms DESC, p.id DESC LIMIT ?2"
            ))
            .map_err(err)?;
        let rows = stmt
            .query_map(params![before_ms, limit(n)], read_play)
            .map_err(err)?;
        rows.collect::<Result<_, _>>().map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use player_history::TARGET_SCROBB;
    use player_testkit::history::{conformance, new_play, play_id};

    /// Временный каталог, удаляется в конце теста.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("player-store-sqlite-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn passes_conformance() {
        conformance(|| SqliteHistory::open_in_memory().unwrap());
    }

    #[test]
    fn reopened_file_keeps_plays_queue_and_device() {
        let dir = TempDir::new("reopen");
        let path = dir.0.join("history.db");
        let device = {
            let store = SqliteHistory::open(&path).unwrap();
            store.begin(&new_play(1, 1_000)).unwrap();
            store
                .mark_scrobbled(&play_id(1), 40_000, &[TARGET_SCROBB], 2_000)
                .unwrap();
            store
                .finish(&play_id(1), 2_500, 41_000, Outcome::Finished)
                .unwrap();
            store.begin(&new_play(2, 3_000)).unwrap();
            store.device_id().to_owned()
        };
        // Как после падения: вторая запись не закрыта.
        let store = SqliteHistory::open(&path).unwrap();
        assert_eq!(store.device_id(), device);
        assert_eq!(device.len(), 36);
        assert_eq!(store.recover_interrupted().unwrap(), 1);
        assert_eq!(store.pending_count(TARGET_SCROBB).unwrap(), 1);
        let plays = store.recent(None, 10).unwrap();
        assert_eq!(plays.len(), 2);
        assert!(plays.iter().all(|p| p.device_id == device));
        let version: usize = store
            .conn()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len());
        let mode: String = store
            .conn()
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn refuses_database_from_newer_app() {
        let dir = TempDir::new("newer");
        let path = dir.0.join("history.db");
        drop(SqliteHistory::open(&path).unwrap());
        Connection::open(&path)
            .unwrap()
            .pragma_update(None, "user_version", MIGRATIONS.len() + 1)
            .unwrap();
        let e = SqliteHistory::open(&path).unwrap_err();
        assert!(e.0.contains("новее"), "{e}");
    }

    #[test]
    fn pending_query_uses_partial_index() {
        let store = SqliteHistory::open_in_memory().unwrap();
        let conn = store.conn();
        let plan: Vec<String> = conn
            .prepare(&format!(
                "EXPLAIN QUERY PLAN SELECT {PLAY_COLUMNS} FROM {PENDING} \
                 ORDER BY p.started_at_ms, p.id LIMIT ?2"
            ))
            .unwrap()
            .query_map(params![TARGET_SCROBB, 50], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert!(
            plan.iter().any(|step| step.contains("deliveries_pending")),
            "{plan:?}"
        );
    }
}
