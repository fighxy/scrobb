//! Плейлисты в SQLite — файл `user.db`: правки пользователя. Сейчас там
//! плейлисты, позже — рейтинги, «любимые» и журнал синхронизации. Библиотеку
//! можно пересобрать сканированием, а эти данные — нет (`docs/playlists.md`).

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use player_playlists::{
    ItemId, Playlist, PlaylistContents, PlaylistError, PlaylistId, PlaylistItem, PlaylistStore,
    PlaylistSummary, TrackSnapshot,
};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::{bad_column, configure, migrate};

/// Миграции `user.db`: элемент `i` переводит базу с версии `i` на `i + 1`.
const MIGRATIONS: &[&str] = &[
    // v1 — `docs/playlists.md`, «Хранение».
    "
    CREATE TABLE playlists (
      id            TEXT PRIMARY KEY,
      title         TEXT NOT NULL,
      description   TEXT,
      origin        TEXT,
      created_at_ms INTEGER NOT NULL,
      updated_at_ms INTEGER NOT NULL,
      deleted_at_ms INTEGER
    ) STRICT;

    CREATE TABLE playlist_items (
      id             TEXT PRIMARY KEY,
      playlist_id    TEXT NOT NULL REFERENCES playlists (id),
      position       TEXT NOT NULL,
      added_at_ms    INTEGER NOT NULL,
      location       TEXT,
      title          TEXT NOT NULL,
      artists        TEXT NOT NULL,
      album          TEXT,
      album_artist   TEXT,
      duration_ms    INTEGER,
      recording_mbid TEXT,
      ext_ids        TEXT NOT NULL DEFAULT '{}',
      deleted_at_ms  INTEGER
    ) STRICT;
    CREATE INDEX playlist_items_order ON playlist_items (playlist_id, position, id)
      WHERE deleted_at_ms IS NULL;
    ",
];

const PLAYLIST_COLUMNS: &str = "id, title, description, origin, created_at_ms, updated_at_ms";
const ITEM_COLUMNS: &str = "id, position, added_at_ms, location, title, artists, album, \
     album_artist, duration_ms, recording_mbid, ext_ids";

/// Плейлисты в SQLite.
#[derive(Debug)]
pub struct SqlitePlaylists {
    conn: Mutex<Connection>,
}

impl SqlitePlaylists {
    /// Открыть или создать `user.db`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, PlaylistError> {
        Self::init(Connection::open(path).map_err(err)?)
    }

    /// В памяти: для тестов.
    pub fn open_in_memory() -> Result<Self, PlaylistError> {
        Self::init(Connection::open_in_memory().map_err(err)?)
    }

    fn init(mut conn: Connection) -> Result<Self, PlaylistError> {
        configure(&conn).map_err(err)?;
        migrate(&mut conn, MIGRATIONS).map_err(PlaylistError)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Правка внутри транзакции: плейлист должен существовать, после правки
    /// его время изменения сдвигается.
    fn edit<T>(
        &self,
        id: &PlaylistId,
        now_ms: i64,
        f: impl FnOnce(&Transaction<'_>) -> Result<T, PlaylistError>,
    ) -> Result<T, PlaylistError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(err)?;
        let touched = tx
            .execute(
                "UPDATE playlists SET updated_at_ms = ?2 WHERE id = ?1 AND deleted_at_ms IS NULL",
                params![id.0, now_ms],
            )
            .map_err(err)?;
        if touched == 0 {
            return Err(missing(id));
        }
        let out = f(&tx)?;
        tx.commit().map_err(err)?;
        Ok(out)
    }
}

fn err(e: rusqlite::Error) -> PlaylistError {
    PlaylistError(e.to_string())
}

fn missing(id: &PlaylistId) -> PlaylistError {
    PlaylistError(format!("нет плейлиста {}", id.0))
}

fn read_playlist(r: &Row<'_>) -> rusqlite::Result<Playlist> {
    Ok(Playlist {
        id: PlaylistId(r.get(0)?),
        title: r.get(1)?,
        description: r.get(2)?,
        origin: r.get(3)?,
        created_at_ms: r.get(4)?,
        updated_at_ms: r.get(5)?,
    })
}

fn read_item(r: &Row<'_>) -> rusqlite::Result<PlaylistItem> {
    let artists: String = r.get(5)?;
    let ext_ids: String = r.get(10)?;
    Ok(PlaylistItem {
        id: ItemId(r.get(0)?),
        position: r.get(1)?,
        added_at_ms: r.get(2)?,
        location: r.get(3)?,
        track: TrackSnapshot {
            title: r.get(4)?,
            artists: serde_json::from_str(&artists).map_err(|e| bad_column(5, e))?,
            album: r.get(6)?,
            album_artist: r.get(7)?,
            duration_ms: r.get(8)?,
            recording_mbid: r.get(9)?,
            ext_ids: serde_json::from_str(&ext_ids).map_err(|e| bad_column(10, e))?,
        },
    })
}

impl PlaylistStore for SqlitePlaylists {
    fn list(&self) -> Result<Vec<PlaylistSummary>, PlaylistError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT p.id, p.title, p.description, p.origin, p.created_at_ms, p.updated_at_ms,
                        COUNT(i.id), COALESCE(SUM(i.duration_ms), 0)
                 FROM playlists p
                 LEFT JOIN playlist_items i ON i.playlist_id = p.id AND i.deleted_at_ms IS NULL
                 WHERE p.deleted_at_ms IS NULL
                 GROUP BY p.id
                 ORDER BY p.updated_at_ms DESC, p.id DESC",
            )
            .map_err(err)?;
        stmt.query_map([], |r| {
            Ok(PlaylistSummary {
                playlist: read_playlist(r)?,
                items: r.get::<_, i64>(6)? as usize,
                duration_ms: r.get::<_, i64>(7)? as u64,
            })
        })
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
    }

    fn get(&self, id: &PlaylistId) -> Result<Option<PlaylistContents>, PlaylistError> {
        let conn = self.conn();
        let playlist = conn
            .query_row(
                &format!("SELECT {PLAYLIST_COLUMNS} FROM playlists WHERE id = ?1 AND deleted_at_ms IS NULL"),
                [&id.0],
                read_playlist,
            )
            .optional()
            .map_err(err)?;
        let Some(playlist) = playlist else {
            return Ok(None);
        };
        let mut stmt = conn
            .prepare(&format!(
                "SELECT {ITEM_COLUMNS} FROM playlist_items
                 WHERE playlist_id = ?1 AND deleted_at_ms IS NULL
                 ORDER BY position, id"
            ))
            .map_err(err)?;
        let items = stmt
            .query_map([&id.0], read_item)
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        Ok(Some(PlaylistContents { playlist, items }))
    }

    fn create(&self, p: &Playlist) -> Result<(), PlaylistError> {
        self.conn()
            .execute(
                &format!(
                    "INSERT INTO playlists ({PLAYLIST_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
                ),
                params![
                    p.id.0,
                    p.title,
                    p.description,
                    p.origin,
                    p.created_at_ms,
                    p.updated_at_ms
                ],
            )
            .map_err(err)?;
        Ok(())
    }

    fn update(
        &self,
        id: &PlaylistId,
        title: &str,
        description: Option<&str>,
        now_ms: i64,
    ) -> Result<(), PlaylistError> {
        self.edit(id, now_ms, |tx| {
            tx.execute(
                "UPDATE playlists SET title = ?2, description = ?3 WHERE id = ?1",
                params![id.0, title, description],
            )
            .map_err(err)?;
            Ok(())
        })
    }

    fn delete(&self, id: &PlaylistId, now_ms: i64) -> Result<(), PlaylistError> {
        self.edit(id, now_ms, |tx| {
            tx.execute(
                "UPDATE playlists SET deleted_at_ms = ?2 WHERE id = ?1",
                params![id.0, now_ms],
            )
            .map_err(err)?;
            Ok(())
        })
    }

    fn add_items(
        &self,
        id: &PlaylistId,
        items: &[PlaylistItem],
        now_ms: i64,
    ) -> Result<(), PlaylistError> {
        self.edit(id, now_ms, |tx| {
            let mut stmt = tx
                .prepare(&format!(
                    "INSERT INTO playlist_items (playlist_id, {ITEM_COLUMNS})
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"
                ))
                .map_err(err)?;
            for item in items {
                let t = &item.track;
                let artists =
                    serde_json::to_string(&t.artists).expect("массив строк сериализуется");
                let ext_ids =
                    serde_json::to_string(&t.ext_ids).expect("словарь строк сериализуется");
                stmt.execute(params![
                    id.0,
                    item.id.0,
                    item.position,
                    item.added_at_ms,
                    item.location,
                    t.title,
                    artists,
                    t.album,
                    t.album_artist,
                    t.duration_ms,
                    t.recording_mbid,
                    ext_ids,
                ])
                .map_err(|e| PlaylistError(format!("элемент {}: {e}", item.id.0)))?;
            }
            Ok(())
        })
    }

    fn remove_items(
        &self,
        id: &PlaylistId,
        items: &[ItemId],
        now_ms: i64,
    ) -> Result<(), PlaylistError> {
        self.edit(id, now_ms, |tx| {
            let mut stmt = tx
                .prepare(
                    "UPDATE playlist_items SET deleted_at_ms = ?3
                     WHERE playlist_id = ?1 AND id = ?2 AND deleted_at_ms IS NULL",
                )
                .map_err(err)?;
            for item in items {
                stmt.execute(params![id.0, item.0, now_ms]).map_err(err)?;
            }
            Ok(())
        })
    }

    fn set_position(
        &self,
        id: &PlaylistId,
        item: &ItemId,
        position: &str,
        now_ms: i64,
    ) -> Result<(), PlaylistError> {
        self.edit(id, now_ms, |tx| {
            let moved = tx
                .execute(
                    "UPDATE playlist_items SET position = ?3
                     WHERE playlist_id = ?1 AND id = ?2 AND deleted_at_ms IS NULL",
                    params![id.0, item.0, position],
                )
                .map_err(err)?;
            if moved == 0 {
                return Err(PlaylistError(format!("нет элемента {}", item.0)));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_testkit::disk::TempDir;
    use player_testkit::playlists::{conformance, item, playlist};

    #[test]
    fn passes_conformance() {
        conformance(|| SqlitePlaylists::open_in_memory().unwrap());
    }

    #[test]
    fn survives_reopening_and_refuses_newer_schema() {
        let dir = TempDir::new("user-db");
        let path = dir.path().join("user.db");
        {
            let store = SqlitePlaylists::open(&path).unwrap();
            let p = playlist(1, 1_000);
            store.create(&p).unwrap();
            store
                .add_items(&p.id, &[item("a", "V", "Кукушка")], 2_000)
                .unwrap();
        }
        let store = SqlitePlaylists::open(&path).unwrap();
        let got = store.get(&playlist(1, 0).id).unwrap().unwrap();
        assert_eq!(got.items.len(), 1);
        assert_eq!(got.items[0].track.title, "Кукушка");
        drop(store);

        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        drop(conn);
        let refused = SqlitePlaylists::open(&path).unwrap_err();
        assert!(refused.0.contains("новее"), "{refused}");
    }

    #[test]
    fn failed_batch_adds_nothing() {
        let store = SqlitePlaylists::open_in_memory().unwrap();
        let p = playlist(1, 1_000);
        store.create(&p).unwrap();
        store
            .add_items(&p.id, &[item("a", "V", "Кукушка")], 2_000)
            .unwrap();
        let err = store
            .add_items(
                &p.id,
                &[item("b", "e", "Новый"), item("a", "k", "Дубль")],
                3_000,
            )
            .unwrap_err();
        assert!(err.0.contains("элемент a"), "{err}");
        let got = store.get(&p.id).unwrap().unwrap();
        assert_eq!(got.items.len(), 1, "пачка — одна транзакция");
        assert_eq!(got.playlist.updated_at_ms, 2_000);
    }
}
