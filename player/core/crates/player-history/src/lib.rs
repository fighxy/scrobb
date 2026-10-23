//! История прослушиваний: каждое прослушивание хранится локально целиком
//! (основа статистики «плеера с памятью»), а засчитанные — уходят в Scrobb.
//!
//! Порог засчитывания — общий для всех клиентов `scrobb-rules`. Хранилище
//! истории и локальная статистика — следующие шаги.

use player_domain::Track;
use scrobb_api::{ClientKind, ListenSubmission, Mbids, Source};
use scrobb_rules::Scrobble;

/// Коннектор, которым плеер помечает свои прослушивания.
pub const CONNECTOR: &str = "player";

/// Как закончилось прослушивание.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Дослушан до конца.
    Finished,
    /// Пропущен.
    Skipped,
    /// Воспроизведение остановлено.
    Stopped,
}

/// Запись истории: хранится локально, даже если порог не достигнут.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    /// Трек.
    pub track: Track,
    /// Начало, секунды Unix UTC.
    pub listened_at: i64,
    /// Сколько прозвучало.
    pub played_ms: u64,
    /// Как закончилось.
    pub outcome: Outcome,
    /// Засчитано ли по правилу скроббла.
    pub scrobbled: bool,
}

/// Прослушивание для отправки в Scrobb. `client_id` — UUID от вызывающего кода.
pub fn to_submission(track: &Track, scrobble: Scrobble, client_id: String) -> ListenSubmission {
    ListenSubmission {
        client_id,
        listened_at: scrobble.listened_at,
        track: track.title.clone(),
        artist: track.artists.join(", "),
        album: track.album.clone(),
        album_artist: track.album_artist.clone(),
        duration_ms: track.duration_ms,
        played_ms: scrobble.played_ms,
        mbids: track.recording_mbid.clone().map(|recording| Mbids {
            recording: Some(recording),
            ..Mbids::default()
        }),
        source: Source {
            kind: ClientKind::Player,
            connector: CONNECTOR.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_domain::{MediaUri, ReplayGain, TrackId};

    #[test]
    fn submission_carries_played_time_and_source() {
        let track = Track {
            id: TrackId(1),
            uri: MediaUri("file:///music/kino.flac".into()),
            title: "Кукушка".into(),
            artists: vec!["Кино".into()],
            album: Some("Чёрный альбом".into()),
            album_artist: None,
            duration_ms: Some(400_000),
            track_no: Some(4),
            disc_no: None,
            recording_mbid: None,
            replay_gain: ReplayGain::default(),
        };
        let s = to_submission(
            &track,
            Scrobble {
                listened_at: 1_760_000_000,
                played_ms: 200_000,
            },
            "id".into(),
        );
        assert_eq!(s.played_ms, 200_000);
        assert_eq!(s.source.kind, ClientKind::Player);
        assert!(s.mbids.is_none());
    }
}
