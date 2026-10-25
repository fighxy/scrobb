//! Доменные типы плеера, общие для всех модулей ядра и оболочек.
//!
//! Только данные, без ввода-вывода; собирается под wasm. Типы сериализуются
//! (`serde`): так их получают оболочки, которым ядро отдаёт JSON, — окно Tauri
//! и веб. Формат закреплён тестом `player-core/tests/json.rs`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Локальный идентификатор трека в базе этого устройства.
/// Между устройствами трек узнаётся по `scrobb_sync::TrackKey`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TrackId(pub u64);

/// Где лежит файл: путь, `content://` на Android, закладка с правами на Apple.
/// Ядро не разбирает адрес — его понимает только порт `FileAccess`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MediaUri(pub String);

/// Значения ReplayGain из тегов.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ReplayGain {
    /// Усиление трека, дБ.
    pub track_gain_db: Option<f32>,
    /// Пик трека.
    pub track_peak: Option<f32>,
    /// Усиление альбома, дБ.
    pub album_gain_db: Option<f32>,
    /// Пик альбома.
    pub album_peak: Option<f32>,
}

/// Трек библиотеки.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    /// Локальный идентификатор.
    pub id: TrackId,
    /// Файл.
    pub uri: MediaUri,
    /// Название.
    pub title: String,
    /// Артисты.
    pub artists: Vec<String>,
    /// Альбом.
    pub album: Option<String>,
    /// Артист альбома.
    pub album_artist: Option<String>,
    /// Длительность.
    pub duration_ms: Option<u64>,
    /// Номер трека.
    pub track_no: Option<u32>,
    /// Номер диска.
    pub disc_no: Option<u32>,
    /// MusicBrainz recording ID.
    pub recording_mbid: Option<String>,
    /// ISRC — код записи, по которому трек узнают стриминги.
    #[serde(default)]
    pub isrc: Option<String>,
    /// ReplayGain.
    pub replay_gain: ReplayGain,
}

/// Метаданные трека, не привязанные к файлу: снимок в истории прослушиваний
/// и в плейлисте. По ним трек узнаётся на другом устройстве и в другом сервисе.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackSnapshot {
    /// Название.
    pub title: String,
    /// Артисты.
    pub artists: Vec<String>,
    /// Альбом.
    pub album: Option<String>,
    /// Артист альбома.
    pub album_artist: Option<String>,
    /// Длительность.
    pub duration_ms: Option<u64>,
    /// MusicBrainz recording ID.
    pub recording_mbid: Option<String>,
    /// Внешние идентификаторы (`isrc`, `yandex`…) — с теми же ключами,
    /// что `ext_ids` на сервере (`docs/architecture.md` §5.1).
    pub ext_ids: BTreeMap<String, String>,
}

impl TrackSnapshot {
    /// ISRC из `ext_ids`.
    pub fn isrc(&self) -> Option<&str> {
        self.ext_ids.get(EXT_ISRC).map(String::as_str)
    }
}

/// Ключ ISRC в `ext_ids`.
pub const EXT_ISRC: &str = "isrc";

impl From<&Track> for TrackSnapshot {
    fn from(track: &Track) -> Self {
        let mut ext_ids = BTreeMap::new();
        if let Some(isrc) = &track.isrc {
            ext_ids.insert(EXT_ISRC.to_owned(), isrc.clone());
        }
        Self {
            title: track.title.clone(),
            artists: track.artists.clone(),
            album: track.album.clone(),
            album_artist: track.album_artist.clone(),
            duration_ms: track.duration_ms,
            recording_mbid: track.recording_mbid.clone(),
            ext_ids,
        }
    }
}

/// Состояние воспроизведения.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStatus {
    /// Ничего не играет.
    #[default]
    Stopped,
    /// Буферизация.
    Buffering,
    /// Играет.
    Playing,
    /// Пауза.
    Paused,
}

/// Режим повтора.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    /// Без повтора.
    #[default]
    Off,
    /// Повтор очереди.
    All,
    /// Повтор трека.
    One,
}

/// Снимок состояния плеера, который ядро отдаёт оболочкам.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerState {
    /// Состояние.
    pub status: PlaybackStatus,
    /// Текущий трек.
    pub current: Option<Track>,
    /// Позиция; между снимками оболочка интерполирует её сама.
    pub position_ms: u64,
    /// Позиция текущего трека в очереди и длина очереди.
    pub queue_position: Option<(usize, usize)>,
    /// Повтор.
    pub repeat: RepeatMode,
    /// Перемешивание.
    pub shuffle: bool,
    /// Громкость 0.0–1.0.
    pub volume: f32,
}

/// Окно большого списка: библиотека не передаётся через FFI целиком.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page<T> {
    /// Элементы окна.
    pub items: Vec<T>,
    /// Смещение окна.
    pub offset: usize,
    /// Всего элементов.
    pub total: usize,
}
