//! Синхронизация состояния библиотеки через аккаунт Scrobb (заготовка).
//!
//! Синхронизируются рейтинги, «любимые», плейлисты и позиции — но не файлы:
//! на каждом устройстве свои файлы, поэтому трек узнаётся по [`TrackKey`],
//! а не по пути. Те же записи будет показывать и править веб-приложение
//! Scrobb, поэтому крейт относится к ярусу Scrobb и собирается под wasm.
//!
//! Сервер для этого ещё не спроектирован: нужны эндпоинты синхронизации.
//! Нормализация ключа уже общая с бэкендом: [`norm`] и [`TrackKey::new`]
//! сверяются с векторами `contracts/match-key/v1.json`.

use serde::{Deserialize, Serialize};

mod key;

pub use key::{norm, primary_artist, strip_feat};

/// Ключ трека, общий для всех устройств и сервера.
///
/// Нормализация (регистр, «ё», `feat.`, транслитерация) должна совпадать
/// с серверным `match_key` (`docs/architecture.md` §5.3).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TrackKey {
    /// Нормализованный артист.
    pub artist: String,
    /// Нормализованное название.
    pub title: String,
    /// MusicBrainz recording ID — если есть, он важнее текстового ключа.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording_mbid: Option<String>,
}

impl TrackKey {
    /// Ключ из метаданных: главный артист и название без «feat. …»,
    /// нормализованные как на сервере (`docs/architecture.md` §5.3).
    pub fn new(artist: &str, title: &str, recording_mbid: Option<&str>) -> Self {
        Self {
            artist: norm(primary_artist(artist)),
            title: norm(&strip_feat(title)),
            recording_mbid: recording_mbid.map(str::to_owned),
        }
    }
}

/// Гибридные логические часы: упорядочивают правки с разных устройств
/// даже при расхождении системных часов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Hlc {
    /// Физическое время, миллисекунды Unix.
    pub wall_ms: u64,
    /// Счётчик для событий в одну миллисекунду.
    pub counter: u32,
    /// Идентификатор устройства — разрешает ничьи.
    pub node: u32,
}

impl Hlc {
    /// Следующая метка для локальной правки.
    pub fn tick(self, now_ms: u64) -> Self {
        if now_ms > self.wall_ms {
            Self {
                wall_ms: now_ms,
                counter: 0,
                node: self.node,
            }
        } else {
            Self {
                counter: self.counter + 1,
                ..self
            }
        }
    }
}

/// Регистр «последняя запись побеждает».
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lww<T> {
    /// Значение.
    pub value: T,
    /// Когда и где его записали.
    pub stamp: Hlc,
}

impl<T> Lww<T> {
    /// Слияние с правкой с другого устройства или сервера.
    pub fn merge(&mut self, other: Lww<T>) {
        if other.stamp > self.stamp {
            *self = other;
        }
    }
}

/// Синхронизируемая правка.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SyncOp {
    /// Рейтинг 0–10; `None` — снят.
    SetRating {
        /// Трек.
        track: TrackKey,
        /// Значение.
        rating: Lww<Option<u8>>,
    },
    /// «Любимый» трек.
    SetLoved {
        /// Трек.
        track: TrackKey,
        /// Значение.
        loved: Lww<bool>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn later_stamp_wins_regardless_of_arrival_order() {
        let a = Hlc {
            wall_ms: 100,
            counter: 0,
            node: 1,
        };
        let b = a.tick(100);
        let mut reg = Lww { value: 5, stamp: b };
        reg.merge(Lww { value: 9, stamp: a });
        assert_eq!(reg.value, 5);
    }
}
