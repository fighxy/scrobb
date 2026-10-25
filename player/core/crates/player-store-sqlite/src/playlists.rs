//! Плейлисты в SQLite — файл `user.db`: правки пользователя. Сейчас там
//! плейлисты и состояние их синхронизации, позже — рейтинги и «любимые».
//! Библиотеку можно пересобрать сканированием, а эти данные — нет
//! (`docs/playlists.md`).
//!
//! У изменяемых полей — метки гибридных часов строкой (`*_hlc`): строки
//! сравниваются как метки. Неотправленное — `change_no > pushed_no`:
//! локальная правка берёт новый номер из `sync_state.change_seq`, пришедшее
//! с сервера остаётся с номером 0.

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use player_playlists::{
    Edit, Hlc, ItemId, ItemRecord, Lww, Merged, Playlist, PlaylistContents, PlaylistError,
    PlaylistId, PlaylistItem, PlaylistRecord, PlaylistStore, PlaylistSummary, SyncState,
    TrackSnapshot, Unsent,
};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::{bad_column, configure, migrate};

/// Метка из уже сохранённого времени (узел 0): для миграции v2.
macro_rules! legacy_hlc {
    ($ms:literal) => {
        concat!("printf('%013d-%010d-%08x', ", $ms, ", 0, 0)")
    };
}

/// Миграции `user.db`: элемент `i` переводит базу с версии `i` на `i + 1`.
const MIGRATIONS: &[&str] = &[
    // v1 — `docs/playlists.md` §3.
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
    // v2 — синхронизация, `docs/playlists.md` §7. Сохранённое раньше получает
    // метки из своего времени (узел 0) и становится неотправленным.
    concat!(
        "
    ALTER TABLE playlists ADD COLUMN title_hlc TEXT NOT NULL DEFAULT '';
    ALTER TABLE playlists ADD COLUMN description_hlc TEXT NOT NULL DEFAULT '';
    ALTER TABLE playlists ADD COLUMN deleted_hlc TEXT;
    ALTER TABLE playlists ADD COLUMN change_no INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE playlists ADD COLUMN pushed_no INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE playlist_items ADD COLUMN position_hlc TEXT NOT NULL DEFAULT '';
    ALTER TABLE playlist_items ADD COLUMN deleted_hlc TEXT;
    ALTER TABLE playlist_items ADD COLUMN change_no INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE playlist_items ADD COLUMN pushed_no INTEGER NOT NULL DEFAULT 0;

    UPDATE playlists SET
      title_hlc = ",
        legacy_hlc!("updated_at_ms"),
        ",
      description_hlc = ",
        legacy_hlc!("updated_at_ms"),
        ",
      deleted_hlc = CASE WHEN deleted_at_ms IS NULL THEN NULL ELSE ",
        legacy_hlc!("deleted_at_ms"),
        " END,
      change_no = 1;
    UPDATE playlist_items SET
      position_hlc = ",
        legacy_hlc!("added_at_ms"),
        ",
      deleted_hlc = CASE WHEN deleted_at_ms IS NULL THEN NULL ELSE ",
        legacy_hlc!("deleted_at_ms"),
        " END,
      change_no = 1;

    CREATE INDEX playlists_unsent ON playlists (change_no) WHERE change_no > pushed_no;
    CREATE INDEX playlist_items_unsent ON playlist_items (change_no) WHERE change_no > pushed_no;

    CREATE TABLE sync_state (
      id         INTEGER PRIMARY KEY CHECK (id = 1),
      node       INTEGER,
      cursor     TEXT,
      change_seq INTEGER NOT NULL
    ) STRICT;
    INSERT INTO sync_state (id, change_seq) VALUES (1, 1);
    "
    ),
];

const PLAYLIST_COLUMNS: &str = "id, title, description, origin, created_at_ms, updated_at_ms";
const ITEM_COLUMNS: &str = "id, position, added_at_ms, location, title, artists, album, \
     album_artist, duration_ms, recording_mbid, ext_ids";
/// Плейлист для синхронизации: [`read_record`].
const RECORD_COLUMNS: &str = "id, created_at_ms, origin, title, title_hlc, description, \
     description_hlc, deleted_hlc, change_no";
/// Элемент для синхронизации: [`read_item_record`].
const ITEM_RECORD_COLUMNS: &str = "id, playlist_id, added_at_ms, title, artists, album, \
     album_artist, duration_ms, recording_mbid, ext_ids, position, position_hlc, deleted_hlc, \
     change_no";

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

    /// Правка внутри транзакции: плейлист должен существовать. Если `f`
    /// что-то изменил (`true`), время изменения плейлиста сдвигается.
    fn edit(
        &self,
        id: &PlaylistId,
        edit: Edit,
        f: impl FnOnce(&Transaction<'_>) -> Result<bool, PlaylistError>,
    ) -> Result<(), PlaylistError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(err)?;
        let live: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM playlists WHERE id = ?1 AND deleted_at_ms IS NULL",
                [&id.0],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        if live.is_none() {
            return Err(missing(id));
        }
        if f(&tx)? {
            tx.execute(
                "UPDATE playlists SET updated_at_ms = ?2 WHERE id = ?1",
                params![id.0, edit.at_ms],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)
    }
}

fn err(e: rusqlite::Error) -> PlaylistError {
    PlaylistError(e.to_string())
}

fn missing(id: &PlaylistId) -> PlaylistError {
    PlaylistError(format!("нет плейлиста {}", id.0))
}

/// Номер новой правки: по нему отбирается неотправленное.
fn next_change(tx: &Transaction<'_>) -> Result<i64, PlaylistError> {
    tx.query_row(
        "UPDATE sync_state SET change_seq = change_seq + 1 WHERE id = 1 RETURNING change_seq",
        [],
        |r| r.get(0),
    )
    .map_err(err)
}

fn hlc(r: &Row<'_>, idx: usize) -> rusqlite::Result<Hlc> {
    let text: String = r.get(idx)?;
    text.parse().map_err(|e| bad_column(idx, e))
}

fn opt_hlc(r: &Row<'_>, idx: usize) -> rusqlite::Result<Option<Hlc>> {
    let text: Option<String> = r.get(idx)?;
    text.map(|t| t.parse().map_err(|e| bad_column(idx, e)))
        .transpose()
}

/// Время правки по метке: для сортировки «недавно изменённые».
fn wall(hlc: Hlc) -> i64 {
    i64::try_from(hlc.wall_ms).unwrap_or(i64::MAX)
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

/// Снимок трека из колонок, начиная с `at`: название, артисты, альбом,
/// артист альбома, длительность, MBID, `ext_ids`.
fn read_track(r: &Row<'_>, at: usize) -> rusqlite::Result<TrackSnapshot> {
    let artists: String = r.get(at + 1)?;
    let ext_ids: String = r.get(at + 6)?;
    Ok(TrackSnapshot {
        title: r.get(at)?,
        artists: serde_json::from_str(&artists).map_err(|e| bad_column(at + 1, e))?,
        album: r.get(at + 2)?,
        album_artist: r.get(at + 3)?,
        duration_ms: r.get(at + 4)?,
        recording_mbid: r.get(at + 5)?,
        ext_ids: serde_json::from_str(&ext_ids).map_err(|e| bad_column(at + 6, e))?,
    })
}

fn read_item(r: &Row<'_>) -> rusqlite::Result<PlaylistItem> {
    Ok(PlaylistItem {
        id: ItemId(r.get(0)?),
        position: r.get(1)?,
        added_at_ms: r.get(2)?,
        location: r.get(3)?,
        track: read_track(r, 4)?,
    })
}

/// Плейлист для синхронизации и его номер правки (колонки [`RECORD_COLUMNS`]).
fn read_record(r: &Row<'_>) -> rusqlite::Result<(PlaylistRecord, i64)> {
    Ok((
        PlaylistRecord {
            id: r.get(0)?,
            created_at_ms: r.get(1)?,
            origin: r.get(2)?,
            title: Lww {
                value: r.get(3)?,
                stamp: hlc(r, 4)?,
            },
            description: Lww {
                value: r.get(5)?,
                stamp: hlc(r, 6)?,
            },
            deleted: opt_hlc(r, 7)?,
        },
        r.get(8)?,
    ))
}

/// Элемент для синхронизации и его номер правки (колонки [`ITEM_RECORD_COLUMNS`]).
fn read_item_record(r: &Row<'_>) -> rusqlite::Result<(ItemRecord, i64)> {
    Ok((
        ItemRecord {
            id: r.get(0)?,
            playlist_id: r.get(1)?,
            added_at_ms: r.get(2)?,
            track: read_track(r, 3)?,
            position: Lww {
                value: r.get(10)?,
                stamp: hlc(r, 11)?,
            },
            deleted: opt_hlc(r, 12)?,
        },
        r.get(13)?,
    ))
}

fn json(track: &TrackSnapshot) -> (String, String) {
    (
        serde_json::to_string(&track.artists).expect("массив строк сериализуется"),
        serde_json::to_string(&track.ext_ids).expect("словарь строк сериализуется"),
    )
}

/// Сохранить новый элемент целиком.
fn insert_item(
    tx: &Transaction<'_>,
    item: &ItemRecord,
    location: Option<&str>,
    change_no: i64,
) -> Result<(), PlaylistError> {
    let t = &item.track;
    let (artists, ext_ids) = json(t);
    tx.execute(
        "INSERT INTO playlist_items (id, playlist_id, position, added_at_ms, location, title,
           artists, album, album_artist, duration_ms, recording_mbid, ext_ids, deleted_at_ms,
           position_hlc, deleted_hlc, change_no)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            item.id,
            item.playlist_id,
            item.position.value,
            item.added_at_ms,
            location,
            t.title,
            artists,
            t.album,
            t.album_artist,
            t.duration_ms,
            t.recording_mbid,
            ext_ids,
            item.deleted.map(wall),
            item.position.stamp.to_string(),
            item.deleted.map(|h| h.to_string()),
            change_no,
        ],
    )
    .map_err(|e| PlaylistError(format!("элемент {}: {e}", item.id)))?;
    Ok(())
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

    fn create(&self, p: &Playlist, edit: Edit) -> Result<(), PlaylistError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(err)?;
        let change_no = next_change(&tx)?;
        let stamp = edit.hlc.to_string();
        tx.execute(
            "INSERT INTO playlists (id, title, description, origin, created_at_ms, updated_at_ms,
               title_hlc, description_hlc, change_no)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8)",
            params![
                p.id.0,
                p.title,
                p.description,
                p.origin,
                p.created_at_ms,
                p.updated_at_ms,
                stamp,
                change_no
            ],
        )
        .map_err(err)?;
        tx.commit().map_err(err)
    }

    fn update(
        &self,
        id: &PlaylistId,
        title: &str,
        description: Option<&str>,
        edit: Edit,
    ) -> Result<(), PlaylistError> {
        self.edit(id, edit, |tx| {
            let stamp = edit.hlc.to_string();
            let renamed = tx
                .execute(
                    "UPDATE playlists SET title = ?2, title_hlc = ?3
                     WHERE id = ?1 AND title IS NOT ?2",
                    params![id.0, title, stamp],
                )
                .map_err(err)?;
            let described = tx
                .execute(
                    "UPDATE playlists SET description = ?2, description_hlc = ?3
                     WHERE id = ?1 AND description IS NOT ?2",
                    params![id.0, description, stamp],
                )
                .map_err(err)?;
            let changed = renamed + described > 0;
            if changed {
                let change_no = next_change(tx)?;
                tx.execute(
                    "UPDATE playlists SET change_no = ?2 WHERE id = ?1",
                    params![id.0, change_no],
                )
                .map_err(err)?;
            }
            Ok(changed)
        })
    }

    fn delete(&self, id: &PlaylistId, edit: Edit) -> Result<(), PlaylistError> {
        self.edit(id, edit, |tx| {
            let change_no = next_change(tx)?;
            tx.execute(
                "UPDATE playlists SET deleted_at_ms = ?2, deleted_hlc = ?3, change_no = ?4
                 WHERE id = ?1",
                params![id.0, edit.at_ms, edit.hlc.to_string(), change_no],
            )
            .map_err(err)?;
            Ok(true)
        })
    }

    fn add_items(
        &self,
        id: &PlaylistId,
        items: &[PlaylistItem],
        edit: Edit,
    ) -> Result<(), PlaylistError> {
        self.edit(id, edit, |tx| {
            for item in items {
                let record = ItemRecord {
                    id: item.id.0.clone(),
                    playlist_id: id.0.clone(),
                    added_at_ms: item.added_at_ms,
                    track: item.track.clone(),
                    position: Lww {
                        value: item.position.clone(),
                        stamp: edit.hlc,
                    },
                    deleted: None,
                };
                let change_no = next_change(tx)?;
                insert_item(tx, &record, item.location.as_deref(), change_no)?;
            }
            Ok(true)
        })
    }

    fn remove_items(
        &self,
        id: &PlaylistId,
        items: &[ItemId],
        edit: Edit,
    ) -> Result<(), PlaylistError> {
        self.edit(id, edit, |tx| {
            let mut removed = 0;
            for item in items {
                let change_no = next_change(tx)?;
                removed += tx
                    .execute(
                        "UPDATE playlist_items SET deleted_at_ms = ?3, deleted_hlc = ?4,
                           change_no = ?5
                         WHERE playlist_id = ?1 AND id = ?2 AND deleted_at_ms IS NULL",
                        params![id.0, item.0, edit.at_ms, edit.hlc.to_string(), change_no],
                    )
                    .map_err(err)?;
            }
            Ok(removed > 0)
        })
    }

    fn set_position(
        &self,
        id: &PlaylistId,
        item: &ItemId,
        position: &str,
        edit: Edit,
    ) -> Result<(), PlaylistError> {
        self.edit(id, edit, |tx| {
            let change_no = next_change(tx)?;
            let moved = tx
                .execute(
                    "UPDATE playlist_items SET position = ?3, position_hlc = ?4, change_no = ?5
                     WHERE playlist_id = ?1 AND id = ?2 AND deleted_at_ms IS NULL",
                    params![id.0, item.0, position, edit.hlc.to_string(), change_no],
                )
                .map_err(err)?;
            if moved == 0 {
                return Err(PlaylistError(format!("нет элемента {}", item.0)));
            }
            Ok(true)
        })
    }

    fn unsent(&self, limit: usize) -> Result<Unsent, PlaylistError> {
        let conn = self.conn();
        let playlists = conn
            .prepare(&format!(
                "SELECT {RECORD_COLUMNS} FROM playlists
                 WHERE change_no > pushed_no ORDER BY change_no, id"
            ))
            .map_err(err)?
            .query_map([], read_record)
            .map_err(err)?
            .map(|r| r.map(|(record, no)| (record, no as u64)))
            .collect::<Result<_, _>>()
            .map_err(err)?;
        let items = conn
            .prepare(&format!(
                "SELECT {ITEM_RECORD_COLUMNS} FROM playlist_items
                 WHERE change_no > pushed_no ORDER BY change_no, id LIMIT ?1"
            ))
            .map_err(err)?
            .query_map([limit as i64], read_item_record)
            .map_err(err)?
            .map(|r| r.map(|(record, no)| (record, no as u64)))
            .collect::<Result<_, _>>()
            .map_err(err)?;
        Ok(Unsent { playlists, items })
    }

    fn mark_sent(&self, sent: &Unsent) -> Result<(), PlaylistError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(err)?;
        for (record, no) in &sent.playlists {
            tx.execute(
                "UPDATE playlists SET pushed_no = max(pushed_no, ?2) WHERE id = ?1",
                params![record.id, *no as i64],
            )
            .map_err(err)?;
        }
        for (record, no) in &sent.items {
            tx.execute(
                "UPDATE playlist_items SET pushed_no = max(pushed_no, ?2) WHERE id = ?1",
                params![record.id, *no as i64],
            )
            .map_err(err)?;
        }
        tx.commit().map_err(err)
    }

    fn merge(
        &self,
        playlists: Vec<PlaylistRecord>,
        items: Vec<ItemRecord>,
    ) -> Result<Merged, PlaylistError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(err)?;
        let mut merged = Merged::default();
        for record in playlists {
            let latest = record.latest();
            merged.latest = merged.latest.max(Some(latest));
            let local = tx
                .query_row(
                    &format!("SELECT {RECORD_COLUMNS} FROM playlists WHERE id = ?1"),
                    [&record.id],
                    read_record,
                )
                .optional()
                .map_err(err)?;
            let r = match local {
                Some((mut local, _)) => {
                    if !local.merge(record) {
                        continue;
                    }
                    local
                }
                None => {
                    tx.execute(
                        "INSERT INTO playlists (id, title, description, origin, created_at_ms,
                           updated_at_ms, title_hlc, description_hlc)
                         VALUES (?1, '', NULL, NULL, ?2, ?2, '', '')",
                        params![record.id, record.created_at_ms],
                    )
                    .map_err(err)?;
                    record
                }
            };
            tx.execute(
                "UPDATE playlists SET created_at_ms = ?2, origin = ?3, title = ?4, title_hlc = ?5,
                   description = ?6, description_hlc = ?7, deleted_hlc = ?8, deleted_at_ms = ?9,
                   updated_at_ms = max(updated_at_ms, ?10)
                 WHERE id = ?1",
                params![
                    r.id,
                    r.created_at_ms,
                    r.origin,
                    r.title.value,
                    r.title.stamp.to_string(),
                    r.description.value,
                    r.description.stamp.to_string(),
                    r.deleted.map(|h| h.to_string()),
                    r.deleted.map(wall),
                    wall(latest),
                ],
            )
            .map_err(err)?;
            merged.changed += 1;
        }
        for record in items {
            let latest = record.latest();
            merged.latest = merged.latest.max(Some(latest));
            let local = tx
                .query_row(
                    &format!("SELECT {ITEM_RECORD_COLUMNS} FROM playlist_items WHERE id = ?1"),
                    [&record.id],
                    read_item_record,
                )
                .optional()
                .map_err(err)?;
            let playlist_id = match local {
                Some((mut local, _)) => {
                    if !local.merge(record) {
                        continue;
                    }
                    tx.execute(
                        "UPDATE playlist_items SET added_at_ms = ?2, position = ?3,
                           position_hlc = ?4, deleted_hlc = ?5, deleted_at_ms = ?6
                         WHERE id = ?1",
                        params![
                            local.id,
                            local.added_at_ms,
                            local.position.value,
                            local.position.stamp.to_string(),
                            local.deleted.map(|h| h.to_string()),
                            local.deleted.map(wall),
                        ],
                    )
                    .map_err(err)?;
                    local.playlist_id
                }
                None => {
                    let known: Option<i64> = tx
                        .query_row(
                            "SELECT 1 FROM playlists WHERE id = ?1",
                            [&record.playlist_id],
                            |r| r.get(0),
                        )
                        .optional()
                        .map_err(err)?;
                    if known.is_none() {
                        return Err(PlaylistError(format!(
                            "элемент {}: нет плейлиста {}",
                            record.id, record.playlist_id
                        )));
                    }
                    insert_item(&tx, &record, None, 0)?;
                    record.playlist_id
                }
            };
            tx.execute(
                "UPDATE playlists SET updated_at_ms = max(updated_at_ms, ?2) WHERE id = ?1",
                params![playlist_id, wall(latest)],
            )
            .map_err(err)?;
            merged.changed += 1;
        }
        tx.commit().map_err(err)?;
        Ok(merged)
    }

    fn sync_state(&self) -> Result<SyncState, PlaylistError> {
        let conn = self.conn();
        let (node, cursor): (Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT node, cursor FROM sync_state WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(err)?;
        // Строки меток сравниваются как метки: максимум считает SQLite.
        let latest: Option<String> = conn
            .query_row(
                "SELECT max(
                   coalesce((SELECT max(max(title_hlc, description_hlc, coalesce(deleted_hlc, '')))
                             FROM playlists), ''),
                   coalesce((SELECT max(max(position_hlc, coalesce(deleted_hlc, '')))
                             FROM playlist_items), ''))",
                [],
                |r| r.get(0),
            )
            .map_err(err)?;
        let latest = latest
            .filter(|s| !s.is_empty())
            .map(|s| s.parse::<Hlc>().map_err(|e| PlaylistError(e.to_string())))
            .transpose()?;
        Ok(SyncState {
            node: node.map(|n| n as u32),
            cursor,
            latest,
        })
    }

    fn set_node(&self, node: u32) -> Result<(), PlaylistError> {
        self.conn()
            .execute(
                "UPDATE sync_state SET node = ?1 WHERE id = 1",
                [i64::from(node)],
            )
            .map_err(err)?;
        Ok(())
    }

    fn set_cursor(&self, cursor: Option<&str>) -> Result<(), PlaylistError> {
        self.conn()
            .execute("UPDATE sync_state SET cursor = ?1 WHERE id = 1", [cursor])
            .map_err(err)?;
        Ok(())
    }

    fn reset_sync(&self) -> Result<(), PlaylistError> {
        self.conn()
            .execute_batch(
                "BEGIN;
                 UPDATE sync_state SET cursor = NULL WHERE id = 1;
                 UPDATE playlists SET pushed_no = -1;
                 UPDATE playlist_items SET pushed_no = -1;
                 COMMIT;",
            )
            .map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_testkit::disk::TempDir;
    use player_testkit::playlists::{conformance, edit, item, playlist};

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
            store.create(&p, edit(1_000)).unwrap();
            store
                .add_items(&p.id, &[item("a", "V", "Кукушка")], edit(2_000))
                .unwrap();
            store.set_node(42).unwrap();
        }
        let store = SqlitePlaylists::open(&path).unwrap();
        let got = store.get(&playlist(1, 0).id).unwrap().unwrap();
        assert_eq!(got.items.len(), 1);
        assert_eq!(got.items[0].track.title, "Кукушка");
        let state = store.sync_state().unwrap();
        assert_eq!(state.node, Some(42));
        assert_eq!(state.latest, Some(edit(2_000).hlc));
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
        store.create(&p, edit(1_000)).unwrap();
        store
            .add_items(&p.id, &[item("a", "V", "Кукушка")], edit(2_000))
            .unwrap();
        let err = store
            .add_items(
                &p.id,
                &[item("b", "e", "Новый"), item("a", "k", "Дубль")],
                edit(3_000),
            )
            .unwrap_err();
        assert!(err.0.contains("элемент a"), "{err}");
        let got = store.get(&p.id).unwrap().unwrap();
        assert_eq!(got.items.len(), 1, "пачка — одна транзакция");
        assert_eq!(got.playlist.updated_at_ms, 2_000);
    }

    #[test]
    fn version_one_database_becomes_unsent_with_legacy_stamps() {
        let dir = TempDir::new("user-db-v1");
        let path = dir.path().join("user.db");
        {
            let mut conn = Connection::open(&path).unwrap();
            migrate(&mut conn, &MIGRATIONS[..1]).unwrap();
            conn.execute_batch(
                "INSERT INTO playlists VALUES ('p1', 'Вечер', NULL, NULL, 1000, 5000, NULL);
                 INSERT INTO playlists VALUES ('p2', 'Старый', NULL, NULL, 1000, 6000, 6000);
                 INSERT INTO playlist_items (id, playlist_id, position, added_at_ms, location,
                   title, artists, ext_ids)
                 VALUES ('i1', 'p1', 'V', 2000, '/m/a.flac', 'Кукушка', '[\"Кино\"]', '{}');",
            )
            .unwrap();
        }
        let store = SqlitePlaylists::open(&path).unwrap();
        let unsent = store.unsent(10).unwrap();
        let legacy = |ms: u64| Hlc {
            wall_ms: ms,
            counter: 0,
            node: 0,
        };
        let titles: Vec<_> = unsent
            .playlists
            .iter()
            .map(|(r, _)| (r.id.as_str(), r.title.stamp, r.deleted))
            .collect();
        assert_eq!(
            titles,
            [
                ("p1", legacy(5_000), None),
                ("p2", legacy(6_000), Some(legacy(6_000)))
            ]
        );
        let (i1, _) = &unsent.items[0];
        assert_eq!(
            (i1.id.as_str(), i1.position.stamp, i1.track.title.as_str()),
            ("i1", legacy(2_000), "Кукушка")
        );
        assert_eq!(store.sync_state().unwrap().latest, Some(legacy(6_000)));
        // Новая правка после миграции — позже старых меток.
        let p1 = PlaylistId("p1".into());
        store
            .update(&p1, "Вечер у окна", None, edit(7_000))
            .unwrap();
        assert_eq!(
            store.get(&p1).unwrap().unwrap().playlist.title,
            "Вечер у окна"
        );
    }
}
