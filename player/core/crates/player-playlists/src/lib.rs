//! Плейлисты: модель, порядок элементов, хранилище и форматы файлов.
//!
//! Устройство — в `docs/playlists.md`:
//! - элемент плейлиста хранит не локальный id трека, а снимок метаданных
//!   ([`TrackSnapshot`]: MBID, ISRC в `ext_ids`) и путь на этом устройстве.
//!   Поэтому плейлист переносится между устройствами и сервисами, а трек,
//!   которого нет в библиотеке, остаётся в плейлисте и находится сам, когда
//!   файл появится;
//! - порядок — дробные ключи ([`order`]): вставка и перестановка не трогают
//!   соседей, правки с разных устройств потом сливаются без перенумерации;
//! - удаление — надгробие: синхронизации нужно знать, что удалено;
//! - id (UUIDv7), время и ключи порядка задаёт вызывающий код (фасад), а
//!   хранилище только хранит. Так хранилища детерминированы и проверяются
//!   одними тестами (`player-testkit::playlists`).
//!
//! Крейт без ввода-вывода и собирается под wasm: форматы — чистые функции
//! «байты ↔ записи», хранилище — за трейтом [`PlaylistStore`] (SQLite —
//! `player-store-sqlite`, в памяти — [`MemoryPlaylists`]).

pub use player_domain::TrackSnapshot;
use serde::{Deserialize, Serialize};

pub mod formats;
mod memory;
pub mod order;
pub mod paths;

pub use memory::MemoryPlaylists;

/// Идентификатор плейлиста — UUIDv7: уникален без координации между устройствами.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlaylistId(pub String);

/// Идентификатор элемента плейлиста — UUIDv7. Один трек может стоять в
/// плейлисте дважды, поэтому элемент — не трек.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ItemId(pub String);

/// Плейлист без элементов.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    /// Идентификатор.
    pub id: PlaylistId,
    /// Название.
    pub title: String,
    /// Описание.
    pub description: Option<String>,
    /// Откуда взялся: `None` — создан здесь, `import:m3u` и т. п. — импорт.
    pub origin: Option<String>,
    /// Создан, миллисекунды Unix UTC.
    pub created_at_ms: i64,
    /// Последняя правка (название, описание, элементы).
    pub updated_at_ms: i64,
}

/// Элемент плейлиста.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistItem {
    /// Идентификатор.
    pub id: ItemId,
    /// Ключ порядка ([`order`]): элементы идут по возрастанию строки.
    pub position: String,
    /// Когда добавлен.
    pub added_at_ms: i64,
    /// Файл на этом устройстве (путь или URI), если известен.
    pub location: Option<String>,
    /// Что это за трек — независимо от файла.
    pub track: TrackSnapshot,
}

/// Плейлист с элементами по порядку.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistContents {
    /// Плейлист.
    pub playlist: Playlist,
    /// Элементы по возрастанию `position` (при равенстве — по id).
    pub items: Vec<PlaylistItem>,
}

/// Плейлист в списке: без элементов, с их числом и общей длительностью.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistSummary {
    /// Плейлист.
    pub playlist: Playlist,
    /// Сколько элементов.
    pub items: usize,
    /// Сумма известных длительностей.
    pub duration_ms: u64,
}

/// Ошибка хранилища или разбора файла.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistError(pub String);

impl std::fmt::Display for PlaylistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PlaylistError {}

/// Хранилище плейлистов. Удалённое не возвращается, но хранится надгробием.
/// Правка удалённого или несуществующего плейлиста — ошибка.
pub trait PlaylistStore: Send + Sync {
    /// Все плейлисты, недавно изменённые первыми.
    fn list(&self) -> Result<Vec<PlaylistSummary>, PlaylistError>;

    /// Плейлист с элементами; `None` — нет такого или удалён.
    fn get(&self, id: &PlaylistId) -> Result<Option<PlaylistContents>, PlaylistError>;

    /// Сохранить новый плейлист.
    fn create(&self, playlist: &Playlist) -> Result<(), PlaylistError>;

    /// Новое название и описание.
    fn update(
        &self,
        id: &PlaylistId,
        title: &str,
        description: Option<&str>,
        now_ms: i64,
    ) -> Result<(), PlaylistError>;

    /// Удалить плейлист (надгробие).
    fn delete(&self, id: &PlaylistId, now_ms: i64) -> Result<(), PlaylistError>;

    /// Добавить элементы; id и ключи порядка уже заданы.
    fn add_items(
        &self,
        id: &PlaylistId,
        items: &[PlaylistItem],
        now_ms: i64,
    ) -> Result<(), PlaylistError>;

    /// Убрать элементы (надгробия); неизвестные id пропускаются.
    fn remove_items(
        &self,
        id: &PlaylistId,
        items: &[ItemId],
        now_ms: i64,
    ) -> Result<(), PlaylistError>;

    /// Новый ключ порядка элемента — перестановка.
    fn set_position(
        &self,
        id: &PlaylistId,
        item: &ItemId,
        position: &str,
        now_ms: i64,
    ) -> Result<(), PlaylistError>;
}

/// «Артист - Название» для подписей и текстовых форматов.
pub fn display_name(track: &TrackSnapshot) -> String {
    if track.artists.is_empty() {
        track.title.clone()
    } else {
        format!("{} - {}", track.artists.join(", "), track.title)
    }
}
