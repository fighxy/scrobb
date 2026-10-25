//! Синхронизация плейлистов: записи, правила слияния и формат обмена с
//! сервером Scrobb (`POST /api/v1/sync/playlists`, `docs/playlists.md` §7).
//!
//! Состояние — множество записей с метками [`Hlc`]. Слияние двух версий одной
//! записи коммутативно, ассоциативно и идемпотентно: устройства и сервер
//! приходят к одному состоянию при любом порядке и повторах доставки.
//! Правила сверяются с векторами `contracts/playlist-sync/v1.json` — их же
//! проходит бэкенд.
//!
//! - Неизменное с создания: id, время создания, происхождение, трек элемента,
//!   плейлист элемента.
//! - «Последняя запись побеждает» по каждому полю отдельно: название,
//!   описание, место элемента. Переименование на одном устройстве и смена
//!   описания на другом сохраняются обе.
//! - Удаление окончательно: надгробие ([`Hlc`] удаления) не снимается
//!   правками с других устройств, сделанными без знания о нём.
//! - Путь к файлу не синхронизируется: он свой на каждом устройстве, трек
//!   там ищется по MBID, ISRC и ключу «артист + название».

use serde::{Deserialize, Serialize};

use crate::{Hlc, Lww};

/// Сколько записей отправлять и просить за один запрос.
pub const MAX_RECORDS: usize = 500;

/// Плейлист без элементов.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistRecord {
    /// UUIDv7, заданный устройством, где плейлист создан.
    pub id: String,
    /// Когда создан, миллисекунды Unix UTC.
    pub created_at_ms: i64,
    /// Откуда взялся: `import:m3u` и т. п.; `None` — создан в плеере.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Название.
    pub title: Lww<String>,
    /// Описание.
    pub description: Lww<Option<String>>,
    /// Надгробие: когда удалён. Удаление окончательно.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted: Option<Hlc>,
}

/// Элемент плейлиста. `T` — снимок трека: у плеера `TrackSnapshot`
/// (`player-domain`), у сервера — его JSON как есть.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemRecord<T> {
    /// UUIDv7 элемента.
    pub id: String,
    /// Плейлист.
    pub playlist_id: String,
    /// Когда добавлен, миллисекунды Unix UTC.
    pub added_at_ms: i64,
    /// Что за трек: название, артисты, альбом, длительность, MBID, `ext_ids`.
    pub track: T,
    /// Ключ порядка (`player-playlists::order`): элементы идут по возрастанию,
    /// при равенстве — по id.
    pub position: Lww<String>,
    /// Надгробие: когда убран. Убранный не возвращается.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted: Option<Hlc>,
}

impl PlaylistRecord {
    /// Слить с версией той же записи с другого устройства; `true` — что-то
    /// изменилось.
    pub fn merge(&mut self, other: PlaylistRecord) -> bool {
        let before = self.clone();
        self.created_at_ms = self.created_at_ms.min(other.created_at_ms);
        self.origin = self.origin.take().max(other.origin);
        self.title.merge(other.title);
        self.description.merge(other.description);
        self.deleted = self.deleted.max(other.deleted);
        *self != before
    }

    /// Самая поздняя метка записи: время последней правки.
    pub fn latest(&self) -> Hlc {
        self.title
            .stamp
            .max(self.description.stamp)
            .max(self.deleted.unwrap_or(self.title.stamp))
    }
}

impl<T> ItemRecord<T> {
    /// Слить с версией той же записи с другого устройства; `true` — что-то
    /// изменилось. Трек и плейлист элемента не меняются после создания.
    pub fn merge(&mut self, other: ItemRecord<T>) -> bool {
        let before = (self.added_at_ms, self.position.clone(), self.deleted);
        self.added_at_ms = self.added_at_ms.min(other.added_at_ms);
        self.position.merge(other.position);
        self.deleted = self.deleted.max(other.deleted);
        (self.added_at_ms, self.position.clone(), self.deleted) != before
    }

    /// Самая поздняя метка записи: время последней правки.
    pub fn latest(&self) -> Hlc {
        self.position
            .stamp
            .max(self.deleted.unwrap_or(self.position.stamp))
    }
}

/// Тело `POST /api/v1/sync/playlists`: изменённое на устройстве и откуда
/// продолжать получать изменённое на сервере.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncRequest<T> {
    /// Курсор из прошлого ответа; `None` — получить всё с начала.
    pub cursor: Option<String>,
    /// Сколько записей устройство готово принять (не больше [`MAX_RECORDS`]).
    pub limit: u32,
    /// Изменённые здесь плейлисты — раньше элементов: сервер сливает их первыми.
    #[serde(default)]
    pub playlists: Vec<PlaylistRecord>,
    /// Изменённые здесь элементы.
    #[serde(default = "Vec::new")]
    pub items: Vec<ItemRecord<T>>,
}

/// Ответ `POST /api/v1/sync/playlists`: записи, изменённые на сервере после
/// курсора запроса (уже после слияния с присланным), в их окончательном виде.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncResponse<T> {
    /// Курсор для следующего запроса.
    pub cursor: String,
    /// Есть ещё изменённое — повторить запрос с новым курсором.
    pub more: bool,
    /// Плейлисты: изменённые и те, чьи элементы есть в ответе.
    #[serde(default)]
    pub playlists: Vec<PlaylistRecord>,
    /// Элементы.
    #[serde(default = "Vec::new")]
    pub items: Vec<ItemRecord<T>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(wall_ms: u64, node: u32) -> Hlc {
        Hlc {
            wall_ms,
            counter: 0,
            node,
        }
    }

    fn playlist(title: &str, stamp: Hlc) -> PlaylistRecord {
        PlaylistRecord {
            id: "p".into(),
            created_at_ms: 1_000,
            origin: None,
            title: Lww {
                value: title.into(),
                stamp,
            },
            description: Lww {
                value: None,
                stamp: at(1_000, 1),
            },
            deleted: None,
        }
    }

    #[test]
    fn fields_merge_independently_and_deletion_sticks() {
        let base = playlist("Вечер", at(1_000, 1));
        let mut renamed = base.clone();
        renamed.title = Lww {
            value: "Вечер у окна".into(),
            stamp: at(2_000, 1),
        };
        let mut described = base.clone();
        described.description = Lww {
            value: Some("тихое".into()),
            stamp: at(1_500, 2),
        };
        let mut a = renamed.clone();
        assert!(a.merge(described.clone()));
        let mut b = described;
        assert!(b.merge(renamed));
        assert_eq!(a, b, "порядок слияния не важен");
        assert_eq!(a.title.value, "Вечер у окна");
        assert_eq!(a.description.value.as_deref(), Some("тихое"));
        assert!(!a.clone().merge(b.clone()), "повтор ничего не меняет");

        let mut deleted = base.clone();
        deleted.deleted = Some(at(1_200, 2));
        let mut later_edit = a.clone();
        assert!(later_edit.merge(deleted));
        assert_eq!(later_edit.deleted, Some(at(1_200, 2)));
        assert_eq!(later_edit.latest(), at(2_000, 1));
    }

    #[test]
    fn item_track_never_changes_and_removal_beats_move() {
        let item = |position: &str, stamp: Hlc, deleted: Option<Hlc>| ItemRecord {
            id: "i".into(),
            playlist_id: "p".into(),
            added_at_ms: 1_000,
            track: "Кукушка",
            position: Lww {
                value: position.into(),
                stamp,
            },
            deleted,
        };
        let mut moved = item("k", at(3_000, 1), None);
        let removed = item("V", at(1_000, 1), Some(at(2_000, 2)));
        assert!(moved.merge(removed.clone()));
        assert_eq!(
            (moved.position.value.as_str(), moved.deleted),
            ("k", Some(at(2_000, 2)))
        );
        let mut other = ItemRecord {
            track: "другое",
            ..removed
        };
        other.merge(item("k", at(3_000, 1), None));
        assert_eq!(other.track, "другое", "трек элемента не меняется");
    }

    #[test]
    fn wire_format_is_stable() {
        let request = SyncRequest {
            cursor: None,
            limit: 500,
            playlists: vec![playlist("Вечер", at(1_761_384_000_000, 0x1a2b_3c4d))],
            items: Vec::<ItemRecord<String>>::new(),
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(
            json["playlists"][0]["title"],
            serde_json::json!({ "value": "Вечер", "stamp": "1761384000000-0000000000-1a2b3c4d" })
        );
        assert!(json["playlists"][0].get("deleted").is_none());
        assert!(json["playlists"][0].get("origin").is_none());
        let back: SyncRequest<String> = serde_json::from_value(json).unwrap();
        assert_eq!(back, request);
    }
}
