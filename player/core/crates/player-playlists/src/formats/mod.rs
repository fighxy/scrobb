//! Форматы файлов плейлистов: чистые функции «байты ↔ записи».
//!
//! | Формат | Для чего | Импорт | Экспорт |
//! |---|---|---|---|
//! | M3U / M3U8 | Любой плеер и медиасервер | ✓ | ✓ (M3U8) |
//! | PLS | Старые плееры, радио | ✓ | — |
//! | XSPF | VLC, Strawberry, Rhythmbox; открытый стандарт Xiph | ✓ | ✓ |
//! | JSPF | ListenBrainz и Scrobb: идентификаторы MusicBrainz, ISRC | ✓ | ✓ |
//! | CSV | Переносчики (TuneMyMusic, Soundiiz), Exportify, таблицы | ✓ | ✓ |
//! | TXT | «Артист - Название»: импорт Яндекс Музыки, переносчики | ✓ | ✓ |
//!
//! Почему так — `docs/playlists.md`.

use player_domain::TrackSnapshot;
use serde::{Deserialize, Serialize};

use crate::{Playlist, PlaylistError, PlaylistItem};

mod csv;
mod jspf;
mod m3u;
mod pls;
mod text;
mod txt;
mod xspf;

pub use text::{decode_text, iso_utc, split_artist_title};

/// Формат файла.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// M3U и M3U8.
    M3u,
    /// PLS.
    Pls,
    /// XSPF (XML).
    Xspf,
    /// JSPF (JSON-форма XSPF), как у ListenBrainz.
    Jspf,
    /// Таблица CSV.
    Csv,
    /// Текст «Артист - Название» построчно.
    Txt,
}

impl Format {
    /// Все форматы экспорта.
    pub const EXPORT: [Format; 5] = [
        Format::M3u,
        Format::Xspf,
        Format::Jspf,
        Format::Csv,
        Format::Txt,
    ];

    /// Расширение файла при экспорте.
    pub fn extension(self) -> &'static str {
        match self {
            Format::M3u => "m3u8",
            Format::Pls => "pls",
            Format::Xspf => "xspf",
            Format::Jspf => "jspf",
            Format::Csv => "csv",
            Format::Txt => "txt",
        }
    }

    /// MIME-тип.
    pub fn mime(self) -> &'static str {
        match self {
            Format::M3u => "audio/x-mpegurl",
            Format::Pls => "audio/x-scpls",
            Format::Xspf => "application/xspf+xml",
            Format::Jspf => "application/json",
            Format::Csv => "text/csv",
            Format::Txt => "text/plain",
        }
    }
}

/// Запись из файла: путь (если есть) и что это за трек.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Путь или URI.
    pub location: Option<String>,
    /// Метаданные и идентификаторы.
    pub track: TrackSnapshot,
}

/// Разобранный файл.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Imported {
    /// Название из файла (`#PLAYLIST`, `<title>`…), если было.
    pub title: Option<String>,
    /// Описание (`<annotation>`), если было.
    pub description: Option<String>,
    /// Записи по порядку.
    pub entries: Vec<Entry>,
}

/// Результат экспорта.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exported {
    /// Текст файла.
    pub text: String,
    /// Сколько элементов не попало: например, в M3U8 — без файла на этом устройстве.
    pub skipped: usize,
}

/// Формат по имени файла, а если расширение не говорит — по содержимому.
pub fn detect(file_name: &str, bytes: &[u8]) -> Format {
    let ext = file_name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    let head = decode_text(&bytes[..bytes.len().min(4096)]);
    let head = head.trim_start();
    match ext.as_str() {
        "m3u" | "m3u8" => Format::M3u,
        "pls" => Format::Pls,
        "xspf" => Format::Xspf,
        "jspf" => Format::Jspf,
        "csv" | "tsv" => Format::Csv,
        _ if head.starts_with("#EXTM3U") => Format::M3u,
        _ if head.to_ascii_lowercase().starts_with("[playlist]") => Format::Pls,
        _ if head.starts_with("<?xml") || head.starts_with("<playlist") => Format::Xspf,
        _ if head.starts_with('{') => Format::Jspf,
        _ => Format::Txt,
    }
}

/// Разобрать файл. `base_dir` — папка файла: от неё считаются относительные пути.
pub fn parse(
    format: Format,
    bytes: &[u8],
    base_dir: Option<&str>,
) -> Result<Imported, PlaylistError> {
    let text = decode_text(bytes);
    match format {
        Format::M3u => Ok(m3u::parse(&text, base_dir)),
        Format::Pls => Ok(pls::parse(&text, base_dir)),
        Format::Xspf => xspf::parse(&text, base_dir),
        Format::Jspf => jspf::parse(&text, base_dir),
        Format::Csv => Ok(csv::parse(&text, base_dir)),
        Format::Txt => Ok(txt::parse(&text)),
    }
}

/// Записать плейлист. `target_dir` — папка, куда ляжет файл: пути M3U8 будут
/// относительно неё, если файлы рядом.
pub fn write(
    format: Format,
    playlist: &Playlist,
    items: &[PlaylistItem],
    target_dir: Option<&str>,
) -> Result<Exported, PlaylistError> {
    Ok(match format {
        Format::M3u => m3u::write(playlist, items, target_dir),
        Format::Pls => return Err(PlaylistError("PLS — только для импорта".into())),
        Format::Xspf => xspf::write(playlist, items),
        Format::Jspf => jspf::write(playlist, items),
        Format::Csv => csv::write(items),
        Format::Txt => txt::write(items),
    })
}

/// Трек из «Артист - Название» и длительности — так пишут M3U и PLS.
fn track_from_label(
    label: &str,
    location: Option<&str>,
    duration_ms: Option<u64>,
) -> TrackSnapshot {
    let (artist, title) = split_artist_title(label);
    let title = if title.is_empty() {
        location
            .map(crate::paths::file_stem)
            .unwrap_or_default()
            .to_owned()
    } else {
        title.to_owned()
    };
    TrackSnapshot {
        title,
        artists: artist.map(|a| vec![a.to_owned()]).unwrap_or_default(),
        duration_ms,
        ..TrackSnapshot::default()
    }
}

/// MBID из идентификатора `https://musicbrainz.org/recording/<mbid>`.
fn mbid_from_identifier(id: &str) -> Option<String> {
    let rest = id.split_once("musicbrainz.org/recording/")?.1;
    let mbid: String = rest
        .chars()
        .take_while(|c| c.is_ascii_hexdigit() || *c == '-')
        .collect();
    (mbid.len() == 36).then(|| mbid.to_ascii_lowercase())
}

/// Идентификатор записи MusicBrainz как URI — так его ждут XSPF и ListenBrainz.
fn mbid_identifier(mbid: &str) -> String {
    format!("https://musicbrainz.org/recording/{mbid}")
}

/// Внешний идентификатор из URI: ISRC, Spotify. Ключи — как `ext_ids` сервера.
fn ext_id_from_identifier(id: &str) -> Option<(&'static str, String)> {
    let lower = id.to_ascii_lowercase();
    if let Some(isrc) = lower
        .strip_prefix("urn:isrc:")
        .or_else(|| lower.strip_prefix("isrc:"))
    {
        return Some(("isrc", isrc.replace('-', "").to_ascii_uppercase()));
    }
    if let Some(track) = id.strip_prefix("spotify:track:") {
        return Some(("spotify", track.to_owned()));
    }
    if let Some(rest) = id.split_once("open.spotify.com/track/") {
        let track: String = rest
            .1
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        return Some(("spotify", track));
    }
    None
}

#[cfg(test)]
mod tests;
