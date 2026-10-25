//! JSPF — JSON-форма XSPF (<https://xspf.org/jspf>). Его использует
//! ListenBrainz для импорта, экспорта и API плейлистов, с расширениями
//! MusicBrainz (<https://musicbrainz.org/doc/jspf>). Это канонический формат
//! Scrobb: то, что нужно для переноса между сервисами и чего нет в XSPF,
//! лежит в своём расширении `https://scrobb.ru/jspf#track`:
//! все артисты списком, артист альбома и `ext_ids` (ISRC, id сервисов).

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::{
    Entry, Exported, Imported, ext_id_from_identifier, iso_utc, mbid_from_identifier,
    mbid_identifier,
};
use crate::paths::{self, decode, from_file_uri, resolve, to_file_uri};
use crate::{Playlist, PlaylistError, PlaylistItem, TrackSnapshot};

/// Расширения MusicBrainz.
pub const MB_PLAYLIST: &str = "https://musicbrainz.org/doc/jspf#playlist";
/// Расширение трека MusicBrainz.
pub const MB_TRACK: &str = "https://musicbrainz.org/doc/jspf#track";
/// Расширение плейлиста Scrobb.
pub const SCROBB_PLAYLIST: &str = "https://scrobb.ru/jspf#playlist";
/// Расширение трека Scrobb.
pub const SCROBB_TRACK: &str = "https://scrobb.ru/jspf#track";

pub fn parse(text: &str, base_dir: Option<&str>) -> Result<Imported, PlaylistError> {
    let root: Value =
        serde_json::from_str(text).map_err(|e| PlaylistError(format!("JSPF: {e}")))?;
    let p = root.get("playlist").unwrap_or(&root);
    if !p.is_object() {
        return Err(PlaylistError("JSPF: нет объекта playlist".into()));
    }
    let entries = p
        .get("track")
        .and_then(Value::as_array)
        .map(|tracks| tracks.iter().map(|t| entry(t, base_dir)).collect())
        .unwrap_or_default();
    Ok(Imported {
        title: string(p, "title"),
        description: string(p, "annotation"),
        entries,
    })
}

fn entry(t: &Value, base_dir: Option<&str>) -> Entry {
    let location = strings(t.get("location"))
        .iter()
        .find_map(|uri| match paths::scheme(uri) {
            Some(s) if s.eq_ignore_ascii_case("file") => from_file_uri(uri),
            Some(_) => Some(uri.clone()),
            None => Some(resolve(base_dir, &decode(uri))),
        });
    let duration_ms = t.get("duration").and_then(|d| {
        d.as_u64()
            .or_else(|| d.as_f64().map(|f| f.max(0.0) as u64))
            .or_else(|| d.as_str().and_then(|s| s.trim().parse().ok()))
    });
    let mut track = TrackSnapshot {
        title: string(t, "title")
            .or_else(|| location.as_deref().map(|l| paths::file_stem(l).to_owned()))
            .unwrap_or_default(),
        artists: string(t, "creator").into_iter().collect(),
        album: string(t, "album"),
        duration_ms: duration_ms.filter(|ms| *ms > 0),
        ..TrackSnapshot::default()
    };
    for id in strings(t.get("identifier")) {
        if let Some(mbid) = mbid_from_identifier(&id) {
            track.recording_mbid.get_or_insert(mbid);
        } else if let Some((key, value)) = ext_id_from_identifier(&id) {
            track.ext_ids.entry(key.to_owned()).or_insert(value);
        }
    }
    if let Some(ext) = t.get("extension").and_then(|e| e.get(SCROBB_TRACK)) {
        let artists: Vec<String> = strings(ext.get("artists"));
        if !artists.is_empty() {
            track.artists = artists;
        }
        track.album_artist = string(ext, "album_artist");
        if let Some(ids) = ext.get("ext_ids").and_then(Value::as_object) {
            for (key, value) in ids {
                if let Some(value) = value.as_str() {
                    track.ext_ids.insert(key.clone(), value.to_owned());
                }
            }
        }
    }
    Entry { location, track }
}

fn string(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Строка или массив строк — JSPF допускает оба вида.
fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

pub fn write(playlist: &Playlist, items: &[PlaylistItem]) -> Exported {
    let tracks: Vec<Value> = items.iter().map(track).collect();
    let mut p = Map::new();
    p.insert("title".into(), json!(playlist.title));
    if let Some(description) = &playlist.description {
        p.insert("annotation".into(), json!(description));
    }
    p.insert("date".into(), json!(iso_utc(playlist.created_at_ms)));
    p.insert(
        "extension".into(),
        json!({
            MB_PLAYLIST: { "public": false, "last_modified_at": iso_utc(playlist.updated_at_ms) },
            SCROBB_PLAYLIST: { "id": playlist.id.0 },
        }),
    );
    p.insert("track".into(), Value::Array(tracks));
    let text = serde_json::to_string_pretty(&json!({ "playlist": p })).expect("JSON собирается");
    Exported {
        text: text + "\n",
        skipped: 0,
    }
}

fn track(item: &PlaylistItem) -> Value {
    let t = &item.track;
    let mut out = Map::new();
    out.insert("title".into(), json!(t.title));
    if !t.artists.is_empty() {
        out.insert("creator".into(), json!(t.artists.join(", ")));
    }
    if let Some(album) = &t.album {
        out.insert("album".into(), json!(album));
    }
    if let Some(ms) = t.duration_ms {
        out.insert("duration".into(), json!(ms));
    }
    if let Some(mbid) = &t.recording_mbid {
        out.insert("identifier".into(), json!([mbid_identifier(mbid)]));
    }
    if let Some(location) = &item.location {
        out.insert("location".into(), json!([to_file_uri(location)]));
    }
    let mut scrobb = Map::new();
    scrobb.insert("artists".into(), json!(t.artists));
    if let Some(album_artist) = &t.album_artist {
        scrobb.insert("album_artist".into(), json!(album_artist));
    }
    if !t.ext_ids.is_empty() {
        let ids: BTreeMap<&String, &String> = t.ext_ids.iter().collect();
        scrobb.insert("ext_ids".into(), json!(ids));
    }
    out.insert(
        "extension".into(),
        json!({
            MB_TRACK: { "added_at": iso_utc(item.added_at_ms) },
            SCROBB_TRACK: scrobb,
        }),
    );
    Value::Object(out)
}
