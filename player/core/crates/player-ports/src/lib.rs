//! Порты ядра: всё, что зависит от ОС, ядро получает через эти интерфейсы.
//!
//! Адаптеры пишутся на Rust (cpal, windows-rs, zbus, keyring) или нативно
//! (Kotlin, Swift) через callback-интерфейсы привязок — см.
//! `docs/player-architecture.md` §4. Ядро не знает, какой адаптер под ним.

use player_domain::{MediaUri, PlaybackStatus, Track};
pub use scrobb_client::{HttpRequest, HttpResponse};

/// Ошибка адаптера.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortError {
    /// Возможность недоступна на этой платформе или устройстве.
    Unsupported,
    /// Нет разрешения (доступ к файлам, уведомления).
    PermissionDenied,
    /// Прочая ошибка с описанием от адаптера.
    Other(String),
}

/// Часы. Монотонное время — для правила скроббла, время Unix — для `listened_at`.
pub trait Clock: Send + Sync {
    /// Миллисекунды монотонных часов.
    fn monotonic_ms(&self) -> u64;
    /// Миллисекунды Unix UTC.
    fn unix_ms(&self) -> i64;
}

/// Формат PCM на выходе движка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PcmFormat {
    /// Частота дискретизации.
    pub sample_rate: u32,
    /// Число каналов.
    pub channels: u16,
}

/// Источник PCM для аудиопотока.
pub trait PcmSource: Send {
    /// Заполнить весь `out` чередующимися сэмплами; где данных нет — тишиной.
    /// Возвращает, сколько сэмплов в начале `out` взято из потока (остальное — тишина).
    /// Вызывается из аудиопотока: без блокировок, аллокаций и ввода-вывода.
    fn fill(&mut self, out: &mut [f32]) -> usize;
}

/// Вывод звука: AAudio, CoreAudio, WASAPI, ALSA/PipeWire.
pub trait AudioOutput: Send {
    /// Открыть устройство; адаптер сам тянет сэмплы из `source`.
    /// Повторный вызов после [`stop`](Self::stop) переоткрывает вывод в новом формате
    /// (например, у следующего трека другая частота).
    fn open(&mut self, format: PcmFormat, source: Box<dyn PcmSource>) -> Result<(), PortError>;
    /// Запустить поток.
    fn start(&mut self) -> Result<(), PortError>;
    /// Остановить поток.
    fn stop(&mut self);
    /// Какую частоту устройство даст, если попросить `wanted`: саму `wanted`, если
    /// поддерживает, иначе свою. Остальное движок ресемплирует. По умолчанию — любую.
    fn sample_rate_for(&mut self, wanted: PcmFormat) -> u32 {
        wanted.sample_rate
    }
    /// Поддерживает ли устройство вывод без изменений (bit-perfect).
    fn bit_perfect(&self) -> bool {
        false
    }
}

/// Системная медиасессия: экран блокировки, уведомление, медиаклавиши
/// (Media3, `MPNowPlayingInfoCenter`, SMTC, MPRIS). Команды от ОС оболочка
/// передаёт в ядро обычными командами фасада.
pub trait MediaSession: Send + Sync {
    /// Опубликовать текущий трек и состояние.
    fn publish(&self, track: &Track, status: PlaybackStatus, position_ms: u64);
    /// Убрать сессию.
    fn clear(&self);
}

/// Запись в папке.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Адрес.
    pub uri: MediaUri,
    /// Папка ли это.
    pub is_dir: bool,
    /// Размер в байтах.
    pub size: u64,
    /// Время изменения, миллисекунды Unix.
    pub modified_ms: i64,
}

/// Поток байтов файла.
pub trait MediaStream: std::io::Read + std::io::Seek + Send + Sync {}
impl<T: std::io::Read + std::io::Seek + Send + Sync> MediaStream for T {}

/// Доступ к файлам: ФС, MediaStore и SAF, песочница и закладки на Apple, порталы Flatpak.
pub trait FileAccess: Send + Sync {
    /// Содержимое папки.
    fn list(&self, dir: &MediaUri) -> Result<Vec<FileEntry>, PortError>;
    /// Открыть файл на чтение.
    fn open(&self, uri: &MediaUri) -> Result<Box<dyn MediaStream>, PortError>;
}

/// Хранилище секретов ОС: Keystore, Keychain, Credential Manager, Secret Service.
/// Здесь живёт токен устройства Scrobb (`docs/architecture.md` §18.4).
pub trait SecureStore: Send + Sync {
    /// Прочитать.
    fn get(&self, key: &str) -> Result<Option<String>, PortError>;
    /// Записать.
    fn set(&self, key: &str, value: &str) -> Result<(), PortError>;
    /// Удалить.
    fn delete(&self, key: &str) -> Result<(), PortError>;
}

/// Сеть: выполняет запросы, которые строит `scrobb-client`.
/// Вызывается из сетевого потока ядра, поэтому может блокировать.
pub trait HttpTransport: Send + Sync {
    /// Выполнить запрос.
    fn send(&self, req: &HttpRequest) -> Result<HttpResponse, PortError>;
}

/// Запасной платформенный декодер: MediaCodec, AudioToolbox, Media Foundation —
/// для кодеков, которых нет в Rust-декодере (`docs/player-architecture.md` §3.2).
pub trait PlatformDecoder: Send + Sync {
    /// Может ли декодировать файл с таким расширением или MIME-типом.
    fn supports(&self, hint: &str) -> bool;
}
