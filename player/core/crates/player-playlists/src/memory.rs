//! Хранилище в памяти: для тестов и как образец поведения для SQLite.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use crate::{
    Edit, Hlc, ItemId, ItemRecord, Lww, Merged, Playlist, PlaylistContents, PlaylistError,
    PlaylistId, PlaylistItem, PlaylistRecord, PlaylistStore, PlaylistSummary, SyncState, Unsent,
};

/// Отметки неотправленного: запись изменена здесь, если `change_no > pushed_no`.
/// Пришедшее с сервера получает `change_no = 0`, после [`PlaylistStore::reset_sync`]
/// `pushed_no = -1` — тогда неотправленным становится всё.
#[derive(Debug, Clone, Copy, Default)]
struct Marks {
    change_no: i64,
    pushed_no: i64,
}

impl Marks {
    fn unsent(self) -> bool {
        self.change_no > self.pushed_no
    }
}

#[derive(Debug)]
struct Entry {
    record: PlaylistRecord,
    updated_at_ms: i64,
    marks: Marks,
    items: Vec<ItemEntry>,
}

#[derive(Debug)]
struct ItemEntry {
    record: ItemRecord,
    location: Option<String>,
    marks: Marks,
}

#[derive(Debug, Default)]
struct State {
    playlists: HashMap<PlaylistId, Entry>,
    node: Option<u32>,
    cursor: Option<String>,
    /// Последний выданный номер правки.
    change_seq: i64,
}

impl State {
    /// Номер новой правки.
    fn changed(&mut self) -> Marks {
        self.change_seq += 1;
        Marks {
            change_no: self.change_seq,
            pushed_no: 0,
        }
    }
}

/// Плейлисты в памяти.
#[derive(Debug, Default)]
pub struct MemoryPlaylists {
    inner: Mutex<State>,
}

impl MemoryPlaylists {
    /// Пустое хранилище.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn missing(id: &PlaylistId) -> PlaylistError {
    PlaylistError(format!("нет плейлиста {}", id.0))
}

/// Живой плейлист для правки и номер правки для него.
fn live<'a>(
    state: &'a mut State,
    id: &PlaylistId,
) -> Result<(&'a mut Entry, Marks), PlaylistError> {
    if !state
        .playlists
        .get(id)
        .is_some_and(|e| e.record.deleted.is_none())
    {
        return Err(missing(id));
    }
    let marks = state.changed();
    Ok((state.playlists.get_mut(id).expect("проверено выше"), marks))
}

fn playlist(e: &Entry) -> Playlist {
    Playlist {
        id: PlaylistId(e.record.id.clone()),
        title: e.record.title.value.clone(),
        description: e.record.description.value.clone(),
        origin: e.record.origin.clone(),
        created_at_ms: e.record.created_at_ms,
        updated_at_ms: e.updated_at_ms,
    }
}

fn ordered(entry: &Entry) -> Vec<PlaylistItem> {
    let mut items: Vec<PlaylistItem> = entry
        .items
        .iter()
        .filter(|i| i.record.deleted.is_none())
        .map(|i| PlaylistItem {
            id: ItemId(i.record.id.clone()),
            position: i.record.position.value.clone(),
            added_at_ms: i.record.added_at_ms,
            location: i.location.clone(),
            track: i.record.track.clone(),
        })
        .collect();
    items.sort_by(|a, b| a.position.cmp(&b.position).then_with(|| a.id.cmp(&b.id)));
    items
}

/// Время правки по метке: для сортировки «недавно изменённые».
fn wall(hlc: Hlc) -> i64 {
    i64::try_from(hlc.wall_ms).unwrap_or(i64::MAX)
}

impl PlaylistStore for MemoryPlaylists {
    fn list(&self) -> Result<Vec<PlaylistSummary>, PlaylistError> {
        let state = self.lock();
        let mut out: Vec<PlaylistSummary> = state
            .playlists
            .values()
            .filter(|e| e.record.deleted.is_none())
            .map(|e| {
                let items = ordered(e);
                PlaylistSummary {
                    playlist: playlist(e),
                    items: items.len(),
                    duration_ms: items.iter().filter_map(|i| i.track.duration_ms).sum(),
                }
            })
            .collect();
        out.sort_by(|a, b| {
            b.playlist
                .updated_at_ms
                .cmp(&a.playlist.updated_at_ms)
                .then_with(|| b.playlist.id.cmp(&a.playlist.id))
        });
        Ok(out)
    }

    fn get(&self, id: &PlaylistId) -> Result<Option<PlaylistContents>, PlaylistError> {
        let state = self.lock();
        Ok(state
            .playlists
            .get(id)
            .filter(|e| e.record.deleted.is_none())
            .map(|e| PlaylistContents {
                playlist: playlist(e),
                items: ordered(e),
            }))
    }

    fn create(&self, p: &Playlist, edit: Edit) -> Result<(), PlaylistError> {
        let mut state = self.lock();
        if state.playlists.contains_key(&p.id) {
            return Err(PlaylistError(format!("плейлист {} уже есть", p.id.0)));
        }
        let marks = state.changed();
        let record = PlaylistRecord {
            id: p.id.0.clone(),
            created_at_ms: p.created_at_ms,
            origin: p.origin.clone(),
            title: Lww {
                value: p.title.clone(),
                stamp: edit.hlc,
            },
            description: Lww {
                value: p.description.clone(),
                stamp: edit.hlc,
            },
            deleted: None,
        };
        state.playlists.insert(
            p.id.clone(),
            Entry {
                record,
                updated_at_ms: p.updated_at_ms,
                marks,
                items: Vec::new(),
            },
        );
        Ok(())
    }

    fn update(
        &self,
        id: &PlaylistId,
        title: &str,
        description: Option<&str>,
        edit: Edit,
    ) -> Result<(), PlaylistError> {
        let mut state = self.lock();
        let (entry, marks) = live(&mut state, id)?;
        let r = &mut entry.record;
        let mut changed = false;
        if r.title.value != title {
            r.title = Lww {
                value: title.to_owned(),
                stamp: edit.hlc,
            };
            changed = true;
        }
        if r.description.value.as_deref() != description {
            r.description = Lww {
                value: description.map(str::to_owned),
                stamp: edit.hlc,
            };
            changed = true;
        }
        if changed {
            entry.marks = marks;
            entry.updated_at_ms = edit.at_ms;
        }
        Ok(())
    }

    fn delete(&self, id: &PlaylistId, edit: Edit) -> Result<(), PlaylistError> {
        let mut state = self.lock();
        let (entry, marks) = live(&mut state, id)?;
        entry.record.deleted = Some(edit.hlc);
        entry.marks = marks;
        entry.updated_at_ms = edit.at_ms;
        Ok(())
    }

    fn add_items(
        &self,
        id: &PlaylistId,
        items: &[PlaylistItem],
        edit: Edit,
    ) -> Result<(), PlaylistError> {
        let mut state = self.lock();
        let (entry, _) = live(&mut state, id)?;
        if let Some(dup) = items
            .iter()
            .find(|new| entry.items.iter().any(|old| old.record.id == new.id.0))
        {
            return Err(PlaylistError(format!("элемент {} уже есть", dup.id.0)));
        }
        let mut added = Vec::with_capacity(items.len());
        for item in items {
            added.push(ItemEntry {
                record: ItemRecord {
                    id: item.id.0.clone(),
                    playlist_id: id.0.clone(),
                    added_at_ms: item.added_at_ms,
                    track: item.track.clone(),
                    position: Lww {
                        value: item.position.clone(),
                        stamp: edit.hlc,
                    },
                    deleted: None,
                },
                location: item.location.clone(),
                marks: Marks::default(),
            });
        }
        for item in &mut added {
            item.marks = state.changed();
        }
        let entry = state.playlists.get_mut(id).expect("проверено выше");
        entry.items.extend(added);
        entry.updated_at_ms = edit.at_ms;
        Ok(())
    }

    fn remove_items(
        &self,
        id: &PlaylistId,
        items: &[ItemId],
        edit: Edit,
    ) -> Result<(), PlaylistError> {
        let mut state = self.lock();
        live(&mut state, id)?;
        let mut removed = 0;
        let entry = state.playlists.get_mut(id).expect("проверено выше");
        let targets: Vec<usize> = entry
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.record.deleted.is_none() && items.iter().any(|t| t.0 == i.record.id))
            .map(|(n, _)| n)
            .collect();
        for n in targets {
            let marks = state.changed();
            let entry = state.playlists.get_mut(id).expect("проверено выше");
            entry.items[n].record.deleted = Some(edit.hlc);
            entry.items[n].marks = marks;
            removed += 1;
        }
        if removed > 0 {
            state
                .playlists
                .get_mut(id)
                .expect("проверено выше")
                .updated_at_ms = edit.at_ms;
        }
        Ok(())
    }

    fn set_position(
        &self,
        id: &PlaylistId,
        item: &ItemId,
        position: &str,
        edit: Edit,
    ) -> Result<(), PlaylistError> {
        let mut state = self.lock();
        let (entry, marks) = live(&mut state, id)?;
        let found = entry
            .items
            .iter_mut()
            .find(|i| i.record.id == item.0 && i.record.deleted.is_none())
            .ok_or_else(|| PlaylistError(format!("нет элемента {}", item.0)))?;
        found.record.position = Lww {
            value: position.to_owned(),
            stamp: edit.hlc,
        };
        found.marks = marks;
        entry.updated_at_ms = edit.at_ms;
        Ok(())
    }

    fn unsent(&self, limit: usize) -> Result<Unsent, PlaylistError> {
        let state = self.lock();
        let mut playlists: Vec<(PlaylistRecord, u64)> = state
            .playlists
            .values()
            .filter(|e| e.marks.unsent())
            .map(|e| (e.record.clone(), e.marks.change_no as u64))
            .collect();
        playlists.sort_by_key(|(r, n)| (*n, r.id.clone()));
        let mut items: Vec<(ItemRecord, u64)> = state
            .playlists
            .values()
            .flat_map(|e| &e.items)
            .filter(|i| i.marks.unsent())
            .map(|i| (i.record.clone(), i.marks.change_no as u64))
            .collect();
        items.sort_by_key(|(r, n)| (*n, r.id.clone()));
        items.truncate(limit);
        Ok(Unsent { playlists, items })
    }

    fn mark_sent(&self, sent: &Unsent) -> Result<(), PlaylistError> {
        let mut state = self.lock();
        for (record, no) in &sent.playlists {
            if let Some(e) = state.playlists.get_mut(&PlaylistId(record.id.clone())) {
                e.marks.pushed_no = e.marks.pushed_no.max(*no as i64);
            }
        }
        for (record, no) in &sent.items {
            let item = state
                .playlists
                .get_mut(&PlaylistId(record.playlist_id.clone()))
                .and_then(|e| e.items.iter_mut().find(|i| i.record.id == record.id));
            if let Some(i) = item {
                i.marks.pushed_no = i.marks.pushed_no.max(*no as i64);
            }
        }
        Ok(())
    }

    fn merge(
        &self,
        playlists: Vec<PlaylistRecord>,
        items: Vec<ItemRecord>,
    ) -> Result<Merged, PlaylistError> {
        let mut state = self.lock();
        if let Some(orphan) = items.iter().find(|i| {
            !state
                .playlists
                .contains_key(&PlaylistId(i.playlist_id.clone()))
                && !playlists.iter().any(|p| p.id == i.playlist_id)
        }) {
            return Err(PlaylistError(format!(
                "элемент {}: нет плейлиста {}",
                orphan.id, orphan.playlist_id
            )));
        }
        let mut merged = Merged::default();
        for record in playlists {
            let latest = record.latest();
            merged.latest = merged.latest.max(Some(latest));
            match state.playlists.get_mut(&PlaylistId(record.id.clone())) {
                Some(e) => {
                    if e.record.merge(record) {
                        e.updated_at_ms = e.updated_at_ms.max(wall(latest));
                        merged.changed += 1;
                    }
                }
                None => {
                    let entry = Entry {
                        updated_at_ms: record.created_at_ms.max(wall(latest)),
                        record,
                        marks: Marks::default(),
                        items: Vec::new(),
                    };
                    state
                        .playlists
                        .insert(PlaylistId(entry.record.id.clone()), entry);
                    merged.changed += 1;
                }
            }
        }
        for record in items {
            let latest = record.latest();
            merged.latest = merged.latest.max(Some(latest));
            let entry = state
                .playlists
                .get_mut(&PlaylistId(record.playlist_id.clone()))
                .expect("проверено выше");
            let changed = match entry.items.iter_mut().find(|i| i.record.id == record.id) {
                Some(i) => i.record.merge(record),
                None => {
                    entry.items.push(ItemEntry {
                        record,
                        location: None,
                        marks: Marks::default(),
                    });
                    true
                }
            };
            if changed {
                entry.updated_at_ms = entry.updated_at_ms.max(wall(latest));
                merged.changed += 1;
            }
        }
        Ok(merged)
    }

    fn sync_state(&self) -> Result<SyncState, PlaylistError> {
        let state = self.lock();
        let latest = state
            .playlists
            .values()
            .flat_map(|e| {
                std::iter::once(e.record.latest()).chain(e.items.iter().map(|i| i.record.latest()))
            })
            .max();
        Ok(SyncState {
            node: state.node,
            cursor: state.cursor.clone(),
            latest,
        })
    }

    fn set_node(&self, node: u32) -> Result<(), PlaylistError> {
        self.lock().node = Some(node);
        Ok(())
    }

    fn set_cursor(&self, cursor: Option<&str>) -> Result<(), PlaylistError> {
        self.lock().cursor = cursor.map(str::to_owned);
        Ok(())
    }

    fn reset_sync(&self) -> Result<(), PlaylistError> {
        let mut state = self.lock();
        state.cursor = None;
        for e in state.playlists.values_mut() {
            e.marks.pushed_no = -1;
            for i in &mut e.items {
                i.marks.pushed_no = -1;
            }
        }
        Ok(())
    }
}
