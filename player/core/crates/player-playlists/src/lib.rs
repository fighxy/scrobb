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
//! - каждая правка несёт метку [`Edit`]: время и гибридные часы [`Hlc`].
//!   Метки хранятся по полям, а неотправленные правки отмечены номером —
//!   так хранилище отдаёт изменённое серверу ([`PlaylistStore::unsent`]) и
//!   сливает пришедшее ([`PlaylistStore::merge`]) по правилам
//!   `scrobb_sync::playlists` (`docs/playlists.md` §7);
//! - id (UUIDv7), время, метки и ключи порядка задаёт вызывающий код
//!   (фасад), а хранилище только хранит. Так хранилища детерминированы и
//!   проверяются одними тестами (`player-testkit::playlists`).
//!
//! Крейт без ввода-вывода и собирается под wasm: форматы — чистые функции
//! «байты ↔ записи», хранилище — за трейтом [`PlaylistStore`] (SQLite —
//! `player-store-sqlite`, в памяти — [`MemoryPlaylists`]).

pub use player_domain::TrackSnapshot;
pub use scrobb_sync::playlists::PlaylistRecord;
pub use scrobb_sync::{Hlc, Lww};
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

/// Элемент в синхронизации: трек — снимок, пути нет (он свой на каждом устройстве).
pub type ItemRecord = scrobb_sync::playlists::ItemRecord<TrackSnapshot>;

/// Метка правки на этом устройстве: время для людей (сортировка, «изменён»)
/// и гибридные часы для слияния с другими устройствами.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edit {
    /// Миллисекунды Unix UTC.
    pub at_ms: i64,
    /// Метка гибридных часов этого устройства.
    pub hlc: Hlc,
}

/// Изменённое здесь и ещё не отправленное. У каждой записи — номер правки:
/// [`PlaylistStore::mark_sent`] снимет отметку, только если запись с тех
/// пор не менялась.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Unsent {
    /// Плейлисты — все неотправленные: сервер должен узнать их раньше элементов.
    pub playlists: Vec<(PlaylistRecord, u64)>,
    /// Элементы, не больше запрошенного.
    pub items: Vec<(ItemRecord, u64)>,
}

impl Unsent {
    /// Отправлять нечего.
    pub fn is_empty(&self) -> bool {
        self.playlists.is_empty() && self.items.is_empty()
    }
}

/// Итог слияния записей с сервера.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Merged {
    /// Сколько записей изменилось или появилось.
    pub changed: usize,
    /// Самая поздняя метка среди пришедших: часы устройства её догоняют.
    pub latest: Option<Hlc>,
}

/// Состояние синхронизации устройства.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncState {
    /// Узел гибридных часов этого устройства; `None` — ещё не назначен.
    pub node: Option<u32>,
    /// Курсор сервера: откуда продолжать получать изменённое.
    pub cursor: Option<String>,
    /// Самая поздняя метка в хранилище: часы не должны уйти назад после перезапуска.
    pub latest: Option<Hlc>,
}

/// Хранилище плейлистов. Удалённое не возвращается, но хранится надгробием.
/// Правка удалённого или несуществующего плейлиста — ошибка.
pub trait PlaylistStore: Send + Sync {
    /// Все плейлисты, недавно изменённые первыми.
    fn list(&self) -> Result<Vec<PlaylistSummary>, PlaylistError>;

    /// Плейлист с элементами; `None` — нет такого или удалён.
    fn get(&self, id: &PlaylistId) -> Result<Option<PlaylistContents>, PlaylistError>;

    /// Сохранить новый плейлист; время создания и изменения — из `playlist`.
    fn create(&self, playlist: &Playlist, edit: Edit) -> Result<(), PlaylistError>;

    /// Новое название и описание. Метку получает только то, что изменилось:
    /// иначе сохранение без правок затёрло бы правку с другого устройства.
    fn update(
        &self,
        id: &PlaylistId,
        title: &str,
        description: Option<&str>,
        edit: Edit,
    ) -> Result<(), PlaylistError>;

    /// Удалить плейлист (надгробие).
    fn delete(&self, id: &PlaylistId, edit: Edit) -> Result<(), PlaylistError>;

    /// Добавить элементы; id и ключи порядка уже заданы.
    fn add_items(
        &self,
        id: &PlaylistId,
        items: &[PlaylistItem],
        edit: Edit,
    ) -> Result<(), PlaylistError>;

    /// Убрать элементы (надгробия); неизвестные и уже убранные id пропускаются.
    fn remove_items(
        &self,
        id: &PlaylistId,
        items: &[ItemId],
        edit: Edit,
    ) -> Result<(), PlaylistError>;

    /// Новый ключ порядка элемента — перестановка.
    fn set_position(
        &self,
        id: &PlaylistId,
        item: &ItemId,
        position: &str,
        edit: Edit,
    ) -> Result<(), PlaylistError>;

    // --- Синхронизация (`docs/playlists.md` §7) ---

    /// Изменённое здесь и не отправленное, в порядке правок: все плейлисты
    /// (и удалённые) и не больше `limit` элементов.
    fn unsent(&self, limit: usize) -> Result<Unsent, PlaylistError>;

    /// Сервер принял `sent`: снять отметку с записей, не менявшихся с тех пор.
    fn mark_sent(&self, sent: &Unsent) -> Result<(), PlaylistError>;

    /// Слить записи с сервера с местными — одной транзакцией, плейлисты
    /// раньше элементов. Элемент плейлиста, которого нет ни здесь, ни в
    /// пачке, — ошибка, и не применяется ничего. Пришедшее не становится
    /// неотправленным; у новых элементов нет пути.
    fn merge(
        &self,
        playlists: Vec<PlaylistRecord>,
        items: Vec<ItemRecord>,
    ) -> Result<Merged, PlaylistError>;

    /// Узел часов, курсор и самая поздняя метка.
    fn sync_state(&self) -> Result<SyncState, PlaylistError>;

    /// Запомнить узел часов этого устройства (один раз, при первом запуске).
    fn set_node(&self, node: u32) -> Result<(), PlaylistError>;

    /// Запомнить курсор сервера.
    fn set_cursor(&self, cursor: Option<&str>) -> Result<(), PlaylistError>;

    /// Начать синхронизацию с нуля (привязан другой аккаунт): курсор
    /// забыть, всё местное — снова неотправленное.
    fn reset_sync(&self) -> Result<(), PlaylistError>;
}

/// «Артист - Название» для подписей и текстовых форматов.
pub fn display_name(track: &TrackSnapshot) -> String {
    if track.artists.is_empty() {
        track.title.clone()
    } else {
        format!("{} - {}", track.artists.join(", "), track.title)
    }
}
