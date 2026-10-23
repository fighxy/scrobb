//! Библиотека (заготовка): сканер через порт `FileAccess`, чтение тегов
//! (lofty), SQLite с FTS5, умные плейлисты (свой язык запросов → SQL),
//! слежение за изменениями папок.
//!
//! Большие списки отдаются окнами (`player_domain::Page`), а не целиком —
//! `docs/player-architecture.md` §3.3.

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
