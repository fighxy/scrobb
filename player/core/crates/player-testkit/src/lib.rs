//! Общая обвязка тестов ядра: движка и фасада.
//!
//! - [`ManualOutput`] — вывод звука без звуковой карты: тест сам забирает
//!   сэмплы, как это делал бы колбэк устройства;
//! - [`Fixtures`] — файлы из `fixtures/` (генерирует `fixtures/generate.sh`
//!   через ffmpeg) за портом `FileAccess`;
//! - [`read_wav`] — эталон, прочитанный без Symphonia, чтобы не зависеть
//!   от проверяемого декодера;
//! - [`ManualClock`] — часы, которые двигает тест;
//! - [`history`] — общие проверки любого `HistoryStore`.

use std::io::Cursor;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use player_domain::MediaUri;
use player_ports::{
    AudioOutput, Clock, FileAccess, FileEntry, MediaStream, PcmFormat, PcmSource, PortError,
};

pub mod history;

/// Папка с тестовыми файлами.
pub const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures");
/// Кадров в `full.wav`: 0,6 с при 44,1 кГц.
pub const FULL_FRAMES: usize = 26_460;

/// Файлы из [`FIXTURES`]; адрес трека — имя файла (`a.flac`).
#[derive(Debug, Default)]
pub struct Fixtures;

impl Fixtures {
    /// Порт `FileAccess` над тестовыми файлами.
    pub fn shared() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl FileAccess for Fixtures {
    fn list(&self, _: &MediaUri) -> Result<Vec<FileEntry>, PortError> {
        Err(PortError::Unsupported)
    }

    fn open(&self, uri: &MediaUri) -> Result<Box<dyn MediaStream>, PortError> {
        let bytes = std::fs::read(format!("{FIXTURES}/{}", uri.0))
            .map_err(|_| PortError::Other(format!("нет файла {}", uri.0)))?;
        Ok(Box::new(Cursor::new(bytes)))
    }
}

/// Вывод, из которого тест сам тянет сэмплы.
#[derive(Clone, Default)]
pub struct ManualOutput {
    source: Arc<Mutex<Option<Box<dyn PcmSource>>>>,
    /// Форматы, в которых вывод открывали, по порядку.
    pub formats: Arc<Mutex<Vec<PcmFormat>>>,
    /// Устройство умеет только эту частоту (`None` — любую).
    pub only_rate: Option<u32>,
}

impl AudioOutput for ManualOutput {
    fn sample_rate_for(&mut self, wanted: PcmFormat) -> u32 {
        self.only_rate.unwrap_or(wanted.sample_rate)
    }

    fn open(&mut self, format: PcmFormat, source: Box<dyn PcmSource>) -> Result<(), PortError> {
        lock(&self.formats).push(format);
        *lock(&self.source) = Some(source);
        Ok(())
    }

    fn start(&mut self) -> Result<(), PortError> {
        Ok(())
    }

    fn stop(&mut self) {}
}

impl ManualOutput {
    /// Устройство, которое умеет только 48 кГц.
    pub fn only_48k() -> Self {
        Self {
            only_rate: Some(48_000),
            ..Self::default()
        }
    }

    /// Один «колбэк устройства» на `samples` сэмплов: вернуть то, что пришло из потока.
    pub fn pull(&self, samples: usize) -> Vec<f32> {
        let mut out = vec![0.0; samples];
        let taken = match lock(&self.source).as_mut() {
            Some(source) => source.fill(&mut out),
            None => 0,
        };
        out.truncate(taken);
        out
    }

    /// Тянуть, пока не наберётся `samples` сэмплов (данные приходят из рабочего потока).
    pub fn collect(&self, samples: usize) -> Vec<f32> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut got = Vec::with_capacity(samples);
        while got.len() < samples {
            assert!(
                Instant::now() < deadline,
                "собрано {} из {samples}",
                got.len()
            );
            let chunk = self.pull((samples - got.len()).min(1024));
            if chunk.is_empty() {
                std::thread::sleep(Duration::from_millis(1));
            }
            got.extend(chunk);
        }
        got
    }
}

/// 16-битный WAV из [`FIXTURES`] как сэмплы `f32`.
pub fn read_wav(name: &str) -> Vec<f32> {
    let wav = std::fs::read(format!("{FIXTURES}/{name}")).expect(name);
    let data = wav
        .windows(4)
        .position(|w| w == b"data")
        .expect("блок data")
        + 8;
    wav[data..]
        .chunks_exact(2)
        .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0)
        .collect()
}

/// Часы, которые двигает тест: монотонное и Unix-время идут вместе.
#[derive(Debug, Default)]
pub struct ManualClock {
    monotonic_ms: AtomicU64,
    unix_ms: AtomicI64,
}

impl ManualClock {
    /// Часы, показывающие `unix_ms`.
    pub fn new(unix_ms: i64) -> Arc<Self> {
        Arc::new(Self {
            monotonic_ms: AtomicU64::new(0),
            unix_ms: AtomicI64::new(unix_ms),
        })
    }

    /// Сдвинуть время вперёд.
    pub fn advance(&self, ms: u64) {
        self.monotonic_ms.fetch_add(ms, Ordering::SeqCst);
        self.unix_ms.fetch_add(ms as i64, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn monotonic_ms(&self) -> u64 {
        self.monotonic_ms.load(Ordering::SeqCst)
    }

    fn unix_ms(&self) -> i64 {
        self.unix_ms.load(Ordering::SeqCst)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
