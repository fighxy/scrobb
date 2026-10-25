//! CSV — таблица для переносчиков (TuneMyMusic, Soundiiz), Exportify и
//! электронных таблиц. Названия колонок экспорта — как у Exportify
//! (exportify.net): их узнают большинство импортёров. При импорте колонки
//! ищутся по названиям, в том числе русским и старым экспортифаевским
//! («Track Duration (ms)»), разделитель — запятая, точка с запятой (Excel) или таб.

use super::{Entry, Exported, Imported};
use crate::paths::{self, from_file_uri, resolve};
use crate::{PlaylistItem, TrackSnapshot};

const HEADER: [&str; 8] = [
    "Track Name",
    "Artist Name(s)",
    "Album Name",
    "Album Artist Name(s)",
    "Duration (ms)",
    "ISRC",
    "MusicBrainz ID",
    "Location",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Column {
    Title,
    Artist,
    Album,
    AlbumArtist,
    Duration { ms: bool },
    Isrc,
    Mbid,
    Location,
    SpotifyUri,
}

fn column(name: &str) -> Option<Column> {
    let n = name.trim().trim_matches('"').trim().to_lowercase();
    Some(match n.as_str() {
        "track name" | "title" | "track" | "name" | "song" | "song name" | "track title"
        | "название" | "трек" | "песня" => Column::Title,
        "artist name(s)"
        | "artist name"
        | "artist"
        | "artists"
        | "исполнитель"
        | "артист"
        | "артисты" => Column::Artist,
        "album name" | "album" | "release" | "альбом" => Column::Album,
        "album artist name(s)" | "album artist" | "артист альбома" => {
            Column::AlbumArtist
        }
        "track duration (ms)" | "duration (ms)" | "duration_ms" | "length (ms)" => {
            Column::Duration { ms: true }
        }
        "duration" | "length" | "time" | "длительность" => {
            Column::Duration { ms: false }
        }
        "isrc" => Column::Isrc,
        "musicbrainz id" | "recording mbid" | "mbid" | "musicbrainz recording id" => Column::Mbid,
        "location" | "path" | "file" | "filename" | "путь" | "файл" => Column::Location,
        "track uri" | "spotify uri" | "uri" => Column::SpotifyUri,
        _ => return None,
    })
}

pub fn parse(text: &str, base_dir: Option<&str>) -> Imported {
    let delimiter = detect_delimiter(text.lines().next().unwrap_or(""));
    let mut rows = records(text, delimiter).into_iter();
    let Some(first) = rows.next() else {
        return Imported::default();
    };
    let header: Vec<Option<Column>> = first.iter().map(|h| column(h)).collect();
    let has_header = header.contains(&Some(Column::Title));
    // Без заголовка — «артист, название, альбом», как в самых простых списках.
    let columns = if has_header {
        header
    } else {
        vec![
            Some(Column::Artist),
            Some(Column::Title),
            Some(Column::Album),
        ]
    };
    let data: Vec<Vec<String>> = if has_header {
        rows.collect()
    } else {
        std::iter::once(first).chain(rows).collect()
    };
    let entries = data
        .into_iter()
        .filter(|row| row.iter().any(|c| !c.trim().is_empty()))
        .filter_map(|row| entry(&columns, &row, base_dir))
        .collect();
    Imported {
        entries,
        ..Imported::default()
    }
}

fn entry(columns: &[Option<Column>], row: &[String], base_dir: Option<&str>) -> Option<Entry> {
    let mut track = TrackSnapshot::default();
    let mut location = None;
    for (col, value) in columns.iter().zip(row) {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match col {
            Some(Column::Title) => track.title = value.to_owned(),
            Some(Column::Artist) => track.artists = vec![value.to_owned()],
            Some(Column::Album) => track.album = Some(value.to_owned()),
            Some(Column::AlbumArtist) => track.album_artist = Some(value.to_owned()),
            Some(Column::Duration { ms }) => track.duration_ms = duration(value, *ms),
            Some(Column::Isrc) => {
                track
                    .ext_ids
                    .insert("isrc".into(), value.replace('-', "").to_uppercase());
            }
            Some(Column::Mbid) => track.recording_mbid = Some(value.to_lowercase()),
            Some(Column::Location) => {
                location = Some(match paths::scheme(value) {
                    Some(s) if s.eq_ignore_ascii_case("file") => {
                        from_file_uri(value).unwrap_or_else(|| value.to_owned())
                    }
                    Some(_) => value.to_owned(),
                    None => resolve(base_dir, value),
                })
            }
            Some(Column::SpotifyUri) => {
                if let Some(id) = value.strip_prefix("spotify:track:") {
                    track.ext_ids.insert("spotify".into(), id.to_owned());
                }
            }
            None => {}
        }
    }
    if track.title.is_empty() {
        track.title = location.as_deref().map(paths::file_stem)?.to_owned();
    }
    Some(Entry { location, track })
}

/// «3:45», «1:02:03», «225000» (мс), «225» (с, если колонка не в мс).
fn duration(value: &str, ms_column: bool) -> Option<u64> {
    if value.contains(':') {
        let secs = value.split(':').try_fold(0u64, |acc, part| {
            part.trim().parse::<u64>().ok().map(|p| acc * 60 + p)
        })?;
        return Some(secs * 1000).filter(|ms| *ms > 0);
    }
    let n: f64 = value.parse().ok()?;
    let ms = if ms_column || n >= 36_000.0 {
        n
    } else {
        n * 1000.0
    };
    Some(ms.round() as u64).filter(|ms| *ms > 0)
}

fn detect_delimiter(header: &str) -> char {
    [',', ';', '\t']
        .into_iter()
        .max_by_key(|d| header.matches(*d).count())
        .filter(|d| header.contains(*d))
        .unwrap_or(',')
}

/// Записи по RFC 4180: кавычки, `""` внутри, переводы строк в кавычках.
fn records(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            c if c == delimiter => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

pub fn write(items: &[PlaylistItem]) -> Exported {
    let mut text = row(HEADER.iter().map(|h| h.to_string()));
    for item in items {
        let t = &item.track;
        text.push_str(&row([
            t.title.clone(),
            t.artists.join(", "),
            t.album.clone().unwrap_or_default(),
            t.album_artist.clone().unwrap_or_default(),
            t.duration_ms.map(|ms| ms.to_string()).unwrap_or_default(),
            t.isrc().unwrap_or_default().to_owned(),
            t.recording_mbid.clone().unwrap_or_default(),
            item.location.clone().unwrap_or_default(),
        ]));
    }
    Exported { text, skipped: 0 }
}

fn row(fields: impl IntoIterator<Item = String>) -> String {
    let mut line = fields
        .into_iter()
        .map(|f| {
            if f.contains([',', '"', '\n', '\r']) {
                format!("\"{}\"", f.replace('"', "\"\""))
            } else {
                f
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    line.push_str("\r\n");
    line
}
