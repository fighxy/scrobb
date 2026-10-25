//! Хранилище в памяти: для тестов и как образец поведения для SQLite.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use crate::{
    ItemId, Playlist, PlaylistContents, PlaylistError, PlaylistId, PlaylistItem, PlaylistStore,
    PlaylistSummary,
};

#[derive(Debug)]
struct Entry {
    playlist: Playlist,
    deleted: bool,
    items: Vec<(PlaylistItem, bool)>,
}

/// Плейлисты в памяти.
#[derive(Debug, Default)]
pub struct MemoryPlaylists {
    inner: Mutex<HashMap<PlaylistId, Entry>>,
}

impl MemoryPlaylists {
    /// Пустое хранилище.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<PlaylistId, Entry>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn live<'a>(
    map: &'a mut HashMap<PlaylistId, Entry>,
    id: &PlaylistId,
) -> Result<&'a mut Entry, PlaylistError> {
    map.get_mut(id)
        .filter(|e| !e.deleted)
        .ok_or_else(|| PlaylistError(format!("нет плейлиста {}", id.0)))
}

fn ordered(entry: &Entry) -> Vec<PlaylistItem> {
    let mut items: Vec<PlaylistItem> = entry
        .items
        .iter()
        .filter(|(_, deleted)| !deleted)
        .map(|(item, _)| item.clone())
        .collect();
    items.sort_by(|a, b| a.position.cmp(&b.position).then_with(|| a.id.cmp(&b.id)));
    items
}

impl PlaylistStore for MemoryPlaylists {
    fn list(&self) -> Result<Vec<PlaylistSummary>, PlaylistError> {
        let map = self.lock();
        let mut out: Vec<PlaylistSummary> = map
            .values()
            .filter(|e| !e.deleted)
            .map(|e| {
                let items = ordered(e);
                PlaylistSummary {
                    playlist: e.playlist.clone(),
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
        let map = self.lock();
        Ok(map
            .get(id)
            .filter(|e| !e.deleted)
            .map(|e| PlaylistContents {
                playlist: e.playlist.clone(),
                items: ordered(e),
            }))
    }

    fn create(&self, playlist: &Playlist) -> Result<(), PlaylistError> {
        let mut map = self.lock();
        if map.contains_key(&playlist.id) {
            return Err(PlaylistError(format!(
                "плейлист {} уже есть",
                playlist.id.0
            )));
        }
        map.insert(
            playlist.id.clone(),
            Entry {
                playlist: playlist.clone(),
                deleted: false,
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
        now_ms: i64,
    ) -> Result<(), PlaylistError> {
        let mut map = self.lock();
        let entry = live(&mut map, id)?;
        entry.playlist.title = title.to_owned();
        entry.playlist.description = description.map(str::to_owned);
        entry.playlist.updated_at_ms = now_ms;
        Ok(())
    }

    fn delete(&self, id: &PlaylistId, now_ms: i64) -> Result<(), PlaylistError> {
        let mut map = self.lock();
        let entry = live(&mut map, id)?;
        entry.deleted = true;
        entry.playlist.updated_at_ms = now_ms;
        Ok(())
    }

    fn add_items(
        &self,
        id: &PlaylistId,
        items: &[PlaylistItem],
        now_ms: i64,
    ) -> Result<(), PlaylistError> {
        let mut map = self.lock();
        let entry = live(&mut map, id)?;
        if let Some(dup) = items
            .iter()
            .find(|new| entry.items.iter().any(|(old, _)| old.id == new.id))
        {
            return Err(PlaylistError(format!("элемент {} уже есть", dup.id.0)));
        }
        entry.items.extend(items.iter().map(|i| (i.clone(), false)));
        entry.playlist.updated_at_ms = now_ms;
        Ok(())
    }

    fn remove_items(
        &self,
        id: &PlaylistId,
        items: &[ItemId],
        now_ms: i64,
    ) -> Result<(), PlaylistError> {
        let mut map = self.lock();
        let entry = live(&mut map, id)?;
        for (item, deleted) in &mut entry.items {
            if items.contains(&item.id) {
                *deleted = true;
            }
        }
        entry.playlist.updated_at_ms = now_ms;
        Ok(())
    }

    fn set_position(
        &self,
        id: &PlaylistId,
        item: &ItemId,
        position: &str,
        now_ms: i64,
    ) -> Result<(), PlaylistError> {
        let mut map = self.lock();
        let entry = live(&mut map, id)?;
        let (found, _) = entry
            .items
            .iter_mut()
            .find(|(i, deleted)| i.id == *item && !deleted)
            .ok_or_else(|| PlaylistError(format!("нет элемента {}", item.0)))?;
        found.position = position.to_owned();
        entry.playlist.updated_at_ms = now_ms;
        Ok(())
    }
}
