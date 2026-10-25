//! Библиотека: трек по идентификатору ([`TrackStore`]), сканер папок
//! ([`scan`]), чтение тегов ([`tags`], lofty) и обложек ([`covers`]) — всё
//! через порт `FileAccess`; поиск трека по пути и метаданным для плейлистов
//! ([`index`]).
//! Позже — база SQLite с FTS5, умные плейлисты (свой язык запросов → SQL),
//! слежение за изменениями папок.
//!
//! Большие списки отдаются окнами (`player_domain::Page`), а не целиком —
//! `docs/player-architecture.md` §3.3.

use std::collections::HashMap;
use std::sync::RwLock;

use player_domain::{Track, TrackId};

pub mod covers;
pub mod index;
pub mod scan;
pub mod tags;

pub use index::{LibraryIndex, TrackQuery};

/// Откуда фасад берёт трек по идентификатору: в очереди — идентификаторы,
/// движку нужен файл. Позже это база SQLite; пока — [`MemoryLibrary`].
pub trait TrackStore: Send + Sync {
    /// Трек или `None`, если его нет в библиотеке.
    fn track(&self, id: TrackId) -> Option<Track>;

    /// Найти трек по пути и метаданным — для плейлистов ([`index`]).
    /// По умолчанию библиотека искать не умеет.
    fn resolve(&self, query: &TrackQuery<'_>) -> Option<TrackId> {
        let _ = query;
        None
    }
}

/// Библиотека в памяти: для тестов и пока нет сканера с базой.
#[derive(Debug, Default)]
pub struct MemoryLibrary {
    inner: RwLock<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    tracks: HashMap<TrackId, Track>,
    index: LibraryIndex,
}

impl MemoryLibrary {
    /// Пустая библиотека.
    pub fn new() -> Self {
        Self::default()
    }

    /// Добавить или заменить трек.
    pub fn insert(&self, track: Track) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        let Inner { tracks, index } = &mut *inner;
        if let Some(old) = tracks.get(&track.id) {
            index.remove(old);
        }
        index.insert(&track);
        tracks.insert(track.id, track);
    }

    /// Убрать трек, например когда файл удалили с диска.
    pub fn remove(&self, id: TrackId) {
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        if let Some(old) = inner.tracks.remove(&id) {
            inner.index.remove(&old);
        }
    }
}

impl TrackStore for MemoryLibrary {
    fn track(&self, id: TrackId) -> Option<Track> {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .tracks
            .get(&id)
            .cloned()
    }

    fn resolve(&self, query: &TrackQuery<'_>) -> Option<TrackId> {
        self.inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .index
            .resolve(query)
    }
}

/// Поле сортировки.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortBy {
    /// Артист, альбом, номер трека.
    #[default]
    Artist,
    /// Название.
    Title,
    /// Дата добавления.
    Added,
    /// Сколько раз слушали.
    PlayCount,
}

/// Запрос к библиотеке за одним окном списка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryQuery {
    /// Полнотекстовый поиск.
    pub text: Option<String>,
    /// Сортировка.
    pub sort: SortBy,
    /// Смещение окна.
    pub offset: usize,
    /// Размер окна.
    pub limit: usize,
}

impl Default for LibraryQuery {
    fn default() -> Self {
        Self {
            text: None,
            sort: SortBy::default(),
            offset: 0,
            limit: 200,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_domain::{MediaUri, ReplayGain, TrackSnapshot};

    fn track(uri: &str, title: &str) -> Track {
        Track {
            id: TrackId(1),
            uri: MediaUri(uri.into()),
            title: title.into(),
            artists: vec!["Кино".into()],
            album: None,
            album_artist: None,
            duration_ms: None,
            track_no: None,
            disc_no: None,
            recording_mbid: None,
            isrc: None,
            replay_gain: ReplayGain::default(),
        }
    }

    #[test]
    fn index_follows_replacement_and_removal() {
        let library = MemoryLibrary::new();
        library.insert(track("/m/old.flac", "Старое"));
        // Тот же id с новым файлом и названием: старое больше не находится.
        library.insert(track("/m/new.flac", "Новое"));
        let find = |location: Option<&str>, title: &str| {
            let snap = TrackSnapshot {
                title: title.into(),
                artists: vec!["Кино".into()],
                ..TrackSnapshot::default()
            };
            library.resolve(&TrackQuery {
                location,
                track: &snap,
            })
        };
        assert_eq!(find(Some("/m/old.flac"), "x"), None);
        assert_eq!(find(None, "Старое"), None);
        assert_eq!(find(None, "Новое"), Some(TrackId(1)));
        library.remove(TrackId(1));
        assert_eq!(find(Some("/m/new.flac"), "Новое"), None);
        assert!(library.track(TrackId(1)).is_none());
    }
}
