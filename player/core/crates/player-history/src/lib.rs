//! История прослушиваний: каждое прослушивание хранится локально целиком
//! (основа статистики «плеера с памятью»), а засчитанные — уходят в Scrobb.
//!
//! Формат, база и синхронизация описаны в `docs/player-history.md`:
//! - прослушивание — неизменяемый факт с глобальным id ([`PlayId`], UUIDv7)
//!   и снимком метаданных ([`TrackSnapshot`]), поэтому история не зависит
//!   от файлов и библиотеки и сливается между устройствами без конфликтов;
//! - доставка в сервисы — отдельная очередь по паре «прослушивание × сервис»
//!   ([`DeliveryState`]), она же журнал отправленного.
//!
//! Крейт без I/O и собирается под wasm: хранилище — за трейтом [`HistoryStore`]
//! (SQLite — `player-store-sqlite`, в памяти — [`MemoryHistory`]).

use player_domain::TrackId;
pub use player_domain::TrackSnapshot;
use scrobb_api::{ClientKind, ListenSubmission, Mbids, NowPlaying, Source};
use serde::{Deserialize, Serialize};

mod memory;

pub use memory::MemoryHistory;

/// Коннектор, которым плеер помечает свои прослушивания.
pub const CONNECTOR: &str = "player";
/// Сервис Scrobb в очереди доставки.
pub const TARGET_SCROBB: &str = "scrobb";
/// Откуда запись: сыграна этим плеером.
pub const ORIGIN_PLAYER: &str = "player";
/// Больше стольких ожидающих отправки в один сервис не держим (§18.8).
pub const QUEUE_MAX_ITEMS: usize = 10_000;
/// Прослушивания старше этого не отправляем: скорее ошибка часов (§18.8).
pub const QUEUE_MAX_AGE_MS: i64 = 14 * 24 * 60 * 60 * 1000;

/// Глобальный идентификатор прослушивания: UUIDv7 в каноническом виде.
/// Он же `client_id` при отправке в любой сервис.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlayId(pub String);

/// Чем закончилось прослушивание.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Ещё играет (или приложение закрылось, а запуск после этого ещё не разобрал).
    Playing,
    /// Дослушан до конца.
    Finished,
    /// Пропущен: следующий, предыдущий, другая очередь, перемотка в начало.
    Skipped,
    /// Воспроизведение остановлено.
    Stopped,
    /// Ошибка чтения файла.
    Error,
    /// Приложение закрылось, не закрыв запись.
    Interrupted,
}

impl Outcome {
    /// Имя в базе и при экспорте.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Playing => "playing",
            Self::Finished => "finished",
            Self::Skipped => "skipped",
            Self::Stopped => "stopped",
            Self::Error => "error",
            Self::Interrupted => "interrupted",
        }
    }

    /// Обратно из имени.
    pub fn parse(s: &str) -> Option<Self> {
        [
            Self::Playing,
            Self::Finished,
            Self::Skipped,
            Self::Stopped,
            Self::Error,
            Self::Interrupted,
        ]
        .into_iter()
        .find(|o| o.as_str() == s)
    }
}

/// Начало прослушивания; остальное хранилище заполняет само.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPlay {
    /// Идентификатор.
    pub id: PlayId,
    /// Начало, миллисекунды Unix UTC.
    pub started_at_ms: i64,
    /// Трек.
    pub track: TrackSnapshot,
    /// Трек в библиотеке этого устройства.
    pub track_id: Option<TrackId>,
}

/// Запись истории.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayRecord {
    /// Идентификатор.
    pub id: PlayId,
    /// Начало, миллисекунды Unix UTC.
    pub started_at_ms: i64,
    /// Конец; `None` — ещё играет.
    pub ended_at_ms: Option<i64>,
    /// Сколько реально прозвучало: без пауз и перемоток вперёд.
    pub played_ms: u64,
    /// Чем закончилось.
    pub outcome: Outcome,
    /// Пройден ли порог скроббла (§18.7).
    pub scrobbled: bool,
    /// Трек.
    pub track: TrackSnapshot,
    /// Трек в библиотеке устройства, на котором его слушали.
    pub track_id: Option<TrackId>,
    /// Устройство, на котором слушали.
    pub device_id: String,
    /// Откуда запись: [`ORIGIN_PLAYER`], позже — импорт и синхронизация.
    pub origin: String,
}

/// Состояние доставки прослушивания в сервис.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    /// Ждёт отправки.
    Pending,
    /// Принято.
    Accepted,
    /// Сервис уже знал это прослушивание — тоже доставлено.
    Duplicate,
    /// Отклонено, повторять бессмысленно.
    Rejected,
    /// Пролежало дольше [`QUEUE_MAX_AGE_MS`] или вытеснено лимитом [`QUEUE_MAX_ITEMS`].
    Expired,
}

impl DeliveryState {
    /// Имя в базе.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Duplicate => "duplicate",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
        }
    }

    /// Обратно из имени.
    pub fn parse(s: &str) -> Option<Self> {
        [
            Self::Pending,
            Self::Accepted,
            Self::Duplicate,
            Self::Rejected,
            Self::Expired,
        ]
        .into_iter()
        .find(|d| d.as_str() == s)
    }
}

/// Итог доставки одного прослушивания.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    /// Прослушивание.
    pub id: PlayId,
    /// Новое состояние.
    pub state: DeliveryState,
    /// Причина отказа от сервиса.
    pub reason: Option<String>,
}

/// Ошибка хранилища.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryError(pub String);

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "история: {}", self.0)
    }
}

impl std::error::Error for HistoryError {}

/// Хранилище истории и очереди доставки.
///
/// Записи только добавляются: меняются лишь поля незакрытой записи и состояние
/// доставки. Вызывается из одного потока записи ядра и из потока интерфейса
/// (чтение ленты), поэтому методы берут `&self`.
pub trait HistoryStore: Send + Sync {
    /// Прослушивание началось: запись `playing` с нулевым временем звучания.
    fn begin(&self, play: &NewPlay) -> Result<(), HistoryError>;

    /// Контрольная точка: сколько уже прозвучало (чтобы падение теряло немного).
    fn checkpoint(&self, id: &PlayId, played_ms: u64) -> Result<(), HistoryError>;

    /// Порог пройден: отметить и поставить в очередь каждого из `targets` —
    /// одной транзакцией.
    fn mark_scrobbled(
        &self,
        id: &PlayId,
        played_ms: u64,
        targets: &[&str],
        now_ms: i64,
    ) -> Result<(), HistoryError>;

    /// Прослушивание закончилось.
    fn finish(
        &self,
        id: &PlayId,
        ended_at_ms: i64,
        played_ms: u64,
        outcome: Outcome,
    ) -> Result<(), HistoryError>;

    /// После падения: незакрытые записи → [`Outcome::Interrupted`], конец —
    /// начало плюс время звучания на последней контрольной точке.
    /// Возвращает, сколько записей закрыто.
    fn recover_interrupted(&self) -> Result<usize, HistoryError>;

    /// Ждущие отправки в `target`, самые старые первыми.
    fn pending(&self, target: &str, limit: usize) -> Result<Vec<PlayRecord>, HistoryError>;

    /// Сколько ждёт отправки в `target`.
    fn pending_count(&self, target: &str) -> Result<usize, HistoryError>;

    /// Записать ответ сервиса.
    fn settle(&self, target: &str, results: &[Settlement], now_ms: i64)
    -> Result<(), HistoryError>;

    /// Ждущие старше [`QUEUE_MAX_AGE_MS`] и самые старые сверх
    /// [`QUEUE_MAX_ITEMS`] → [`DeliveryState::Expired`]. Возвращает их число.
    fn expire(&self, target: &str, now_ms: i64) -> Result<usize, HistoryError>;

    /// Лента: новые первыми, начатые строго раньше `before_ms`.
    fn recent(&self, before_ms: Option<i64>, limit: usize)
    -> Result<Vec<PlayRecord>, HistoryError>;
}

/// Прослушивание для отправки в Scrobb: `client_id` — id записи, время
/// звучания — на момент отправки.
pub fn submission(play: &PlayRecord) -> ListenSubmission {
    let track = &play.track;
    ListenSubmission {
        client_id: play.id.0.clone(),
        listened_at: play.started_at_ms.div_euclid(1000),
        track: track.title.clone(),
        artist: track.artists.join(", "),
        album: track.album.clone(),
        album_artist: track.album_artist.clone(),
        duration_ms: track.duration_ms,
        played_ms: play.played_ms,
        mbids: track.recording_mbid.clone().map(|recording| Mbids {
            recording: Some(recording),
            ..Mbids::default()
        }),
        source: source(),
    }
}

/// «Сейчас играет» для Scrobb (§18.7).
pub fn now_playing(track: &TrackSnapshot, position_ms: Option<u64>) -> NowPlaying {
    NowPlaying {
        track: track.title.clone(),
        artist: track.artists.join(", "),
        album: track.album.clone(),
        duration_ms: track.duration_ms,
        position_ms,
        source: source(),
    }
}

fn source() -> Source {
    Source {
        kind: ClientKind::Player,
        connector: CONNECTOR.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_carries_id_seconds_and_played_time() {
        let play = PlayRecord {
            id: PlayId("0190a5b2-0000-7000-8000-000000000001".into()),
            started_at_ms: 1_760_000_000_999,
            ended_at_ms: None,
            played_ms: 200_000,
            outcome: Outcome::Playing,
            scrobbled: true,
            track: TrackSnapshot {
                title: "Кукушка".into(),
                artists: vec!["Кино".into(), "Виктор Цой".into()],
                album: Some("Чёрный альбом".into()),
                duration_ms: Some(400_000),
                ..TrackSnapshot::default()
            },
            track_id: Some(TrackId(1)),
            device_id: "d".into(),
            origin: ORIGIN_PLAYER.into(),
        };
        let s = submission(&play);
        assert_eq!(s.client_id, play.id.0);
        assert_eq!(s.listened_at, 1_760_000_000);
        assert_eq!(s.artist, "Кино, Виктор Цой");
        assert_eq!(s.played_ms, 200_000);
        assert_eq!(s.source.kind, ClientKind::Player);
        assert!(s.mbids.is_none());

        let np = now_playing(&play.track, Some(1_000));
        assert_eq!(
            (np.artist.as_str(), np.position_ms),
            ("Кино, Виктор Цой", Some(1_000))
        );
        assert_eq!(np.source, s.source);
    }

    #[test]
    fn names_round_trip() {
        for o in [
            Outcome::Playing,
            Outcome::Finished,
            Outcome::Skipped,
            Outcome::Stopped,
            Outcome::Error,
            Outcome::Interrupted,
        ] {
            assert_eq!(Outcome::parse(o.as_str()), Some(o));
        }
        for d in [
            DeliveryState::Pending,
            DeliveryState::Accepted,
            DeliveryState::Duplicate,
            DeliveryState::Rejected,
            DeliveryState::Expired,
        ] {
            assert_eq!(DeliveryState::parse(d.as_str()), Some(d));
        }
        assert_eq!(Outcome::parse("nope"), None);
    }
}
