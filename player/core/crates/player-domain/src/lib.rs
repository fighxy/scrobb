//! Доменные типы плеера, общие для всех модулей ядра и оболочек.
//!
//! Только данные, без ввода-вывода; собирается под wasm.

/// Локальный идентификатор трека в базе этого устройства.
/// Между устройствами трек узнаётся по `scrobb_sync::TrackKey`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TrackId(pub u64);

/// Где лежит файл: путь, `content://` на Android, закладка с правами на Apple.
/// Ядро не разбирает адрес — его понимает только порт `FileAccess`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MediaUri(pub String);

/// Значения ReplayGain из тегов.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
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
    /// ReplayGain.
    pub replay_gain: ReplayGain,
}

/// Состояние воспроизведения.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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
#[derive(Debug, Clone, Default, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
pub struct Page<T> {
    /// Элементы окна.
    pub items: Vec<T>,
    /// Смещение окна.
    pub offset: usize,
    /// Всего элементов.
    pub total: usize,
}
