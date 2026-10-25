//! XSPF 1 (Xiph, <https://xspf.org/spec>): XML с метаданными трека и
//! идентификаторами. Его пишут VLC, Strawberry, Rhythmbox, Audacious, а
//! ListenBrainz отдаёт плейлисты и в нём. `location` — URI (`file://`),
//! `identifier` — `https://musicbrainz.org/recording/<MBID>`.

use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::Event;

use super::{
    Entry, Exported, Imported, ext_id_from_identifier, iso_utc, mbid_from_identifier,
    mbid_identifier,
};
use crate::paths::{self, decode, from_file_uri, resolve, to_file_uri};
use crate::{Playlist, PlaylistError, PlaylistItem, TrackSnapshot};

#[derive(Default)]
struct RawTrack {
    locations: Vec<String>,
    identifiers: Vec<String>,
    title: Option<String>,
    creator: Option<String>,
    album: Option<String>,
    duration: Option<u64>,
}

pub fn parse(text: &str, base_dir: Option<&str>) -> Result<Imported, PlaylistError> {
    // Без trim_text: ссылки на сущности (`&amp;`) приходят отдельными событиями,
    // и обрезка кусков съела бы пробелы вокруг них. Обрезается значение целиком.
    let mut reader = Reader::from_str(text);
    let mut out = Imported::default();
    let mut stack: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut track: Option<RawTrack> = None;
    loop {
        let event = reader
            .read_event()
            .map_err(|e| PlaylistError(format!("XSPF: {e}")))?;
        match event {
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_owned();
                if name == "track" && stack.last().is_some_and(|p| p == "trackList") {
                    track = Some(RawTrack::default());
                }
                stack.push(name);
                buf.clear();
            }
            Event::Text(t) => buf.push_str(&t.xml10_content()),
            Event::CData(c) => buf.push_str(&c.xml10_content()),
            Event::GeneralRef(r) => {
                if let Ok(Some(c)) = r.resolve_char_ref() {
                    buf.push(c);
                } else {
                    buf.push_str(match r.xml10_content().as_ref() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => "",
                    });
                }
            }
            Event::End(_) => {
                let name = stack.pop().unwrap_or_default();
                let value = std::mem::take(&mut buf).trim().to_owned();
                let parent = stack.last().map(String::as_str);
                match (&mut track, name.as_str()) {
                    (Some(_), "track") => {
                        if let Some(raw) = track.take() {
                            out.entries.push(entry(raw, base_dir));
                        }
                    }
                    (Some(t), field) if parent == Some("track") && !value.is_empty() => match field
                    {
                        "location" => t.locations.push(value),
                        "identifier" => t.identifiers.push(value),
                        "title" => t.title = Some(value),
                        "creator" => t.creator = Some(value),
                        "album" => t.album = Some(value),
                        "duration" => t.duration = value.parse().ok().filter(|ms| *ms > 0),
                        _ => {}
                    },
                    (None, "title") if parent == Some("playlist") && !value.is_empty() => {
                        out.title = Some(value)
                    }
                    (None, "annotation") if parent == Some("playlist") && !value.is_empty() => {
                        out.description = Some(value)
                    }
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(out)
}

fn entry(raw: RawTrack, base_dir: Option<&str>) -> Entry {
    let location = raw.locations.iter().find_map(|l| location(l, base_dir));
    let mut track = TrackSnapshot {
        title: raw
            .title
            .or_else(|| location.as_deref().map(|l| paths::file_stem(l).to_owned()))
            .unwrap_or_default(),
        artists: raw.creator.into_iter().collect(),
        album: raw.album,
        duration_ms: raw.duration,
        ..TrackSnapshot::default()
    };
    for id in &raw.identifiers {
        if let Some(mbid) = mbid_from_identifier(id) {
            track.recording_mbid.get_or_insert(mbid);
        } else if let Some((key, value)) = ext_id_from_identifier(id) {
            track.ext_ids.entry(key.to_owned()).or_insert(value);
        }
    }
    Entry { location, track }
}

/// `location` XSPF — URI: `file://` → путь, другие схемы — как есть,
/// относительный URI — от папки плейлиста.
fn location(uri: &str, base_dir: Option<&str>) -> Option<String> {
    match paths::scheme(uri) {
        Some(s) if s.eq_ignore_ascii_case("file") => from_file_uri(uri),
        Some(_) => Some(uri.to_owned()),
        None => Some(resolve(base_dir, &decode(uri))),
    }
}

pub fn write(playlist: &Playlist, items: &[PlaylistItem]) -> Exported {
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x.push_str("<playlist version=\"1\" xmlns=\"http://xspf.org/ns/0/\">\n");
    element(&mut x, 1, "title", &playlist.title);
    if let Some(description) = &playlist.description {
        element(&mut x, 1, "annotation", description);
    }
    element(&mut x, 1, "date", &iso_utc(playlist.updated_at_ms));
    x.push_str("  <trackList>\n");
    for item in items {
        let t = &item.track;
        x.push_str("    <track>\n");
        if let Some(location) = &item.location {
            element(&mut x, 3, "location", &to_file_uri(location));
        }
        if let Some(mbid) = &t.recording_mbid {
            element(&mut x, 3, "identifier", &mbid_identifier(mbid));
        }
        element(&mut x, 3, "title", &t.title);
        if !t.artists.is_empty() {
            element(&mut x, 3, "creator", &t.artists.join(", "));
        }
        if let Some(album) = &t.album {
            element(&mut x, 3, "album", album);
        }
        if let Some(ms) = t.duration_ms {
            element(&mut x, 3, "duration", &ms.to_string());
        }
        x.push_str("    </track>\n");
    }
    x.push_str("  </trackList>\n</playlist>\n");
    Exported {
        text: x,
        skipped: 0,
    }
}

fn element(out: &mut String, depth: usize, name: &str, value: &str) {
    out.push_str(&"  ".repeat(depth));
    out.push_str(&format!("<{name}>{}</{name}>\n", escape(value)));
}
