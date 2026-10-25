//! Библиотека: трек по идентификатору ([`TrackStore`]), сканер папок
//! ([`scan`]) и чтение тегов ([`tags`], lofty) — оба через порт `FileAccess`.
//! Позже — база SQLite с FTS5, умные плейлисты (свой язык запросов → SQL),
//! слежение за изменениями папок.
//!
//! Большие списки отдаются окнами (`player_domain::Page`), а не целиком —
//! `docs/player-architecture.md` §3.3.

use std::collections::HashMap;
use std::sync::RwLock;

use player_domain::{Track, TrackId};

pub mod scan;
pub mod tags;

/// Откуда фасад берёт трек по идентификатору: в очереди — идентификаторы,
/// движку нужен файл. Позже это база SQLite; пока — [`MemoryLibrary`].
pub trait TrackStore: Send + Sync {
    /// Трек или `None`, если его нет в библиотеке.
    fn track(&self, id: TrackId) -> Option<Track>;
}

/// Библиотека в памяти: для тестов и пока нет сканера с базой.
#[derive(Debug, Default)]
pub struct MemoryLibrary {
    tracks: RwLock<HashMap<TrackId, Track>>,
}

impl MemoryLibrary {
    /// Пустая библиотека.
    pub fn new() -> Self {
        Self::default()
    }

    /// Добавить или заменить трек.
    pub fn insert(&self, track: Track) {
        self.tracks
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(track.id, track);
    }

    /// Убрать трек, например когда файл удалили с диска.
    pub fn remove(&self, id: TrackId) {
        self.tracks
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    }
}

impl TrackStore for MemoryLibrary {
    fn track(&self, id: TrackId) -> Option<Track> {
        self.tracks
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
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
