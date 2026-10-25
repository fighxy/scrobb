//! Синхронизация правок пользователя через аккаунт Scrobb.
//!
//! Синхронизируются плейлисты, позже — рейтинги, «любимые» и позиции, но не
//! файлы: на каждом устройстве свои файлы, поэтому трек узнаётся по
//! [`TrackKey`], MBID и ISRC, а не по пути. Те же записи будет показывать и
//! править веб-приложение Scrobb, поэтому крейт относится к ярусу Scrobb и
//! собирается под wasm.
//!
//! - Время правок — гибридные логические часы [`Hlc`], поля — [`Lww`].
//! - Плейлисты — [`playlists`]: записи, слияние и формат обмена с сервером
//!   (`docs/playlists.md` §7), векторы `contracts/playlist-sync/v1.json`.
//! - Ключ трека общий с бэкендом: [`norm`] и [`TrackKey::new`] сверяются с
//!   векторами `contracts/match-key/v1.json`.

use serde::{Deserialize, Serialize};

mod hlc;
mod key;
pub mod playlists;

pub use hlc::{BadHlc, Hlc, Lww};
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

/// Синхронизируемая правка рейтинга или «любимого» (заготовка: в плеере их ещё нет).
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
