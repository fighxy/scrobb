//! M3U и M3U8: общий минимум всех плееров. Строка — путь или URI, перед ней
//! необязательные `#EXTINF:<секунды>,<Артист> - <Название>`; `#PLAYLIST` —
//! название, `#EXTALB` и `#EXTART` — альбом и артист альбома (обычно один раз
//! в начале списка-альбома); действуют на все записи после них.
//! Формальной спецификации нет, поэтому разбор терпимый, а запись — самая
//! распространённая форма: UTF-8, `#EXTM3U`, `#PLAYLIST`, `#EXTINF`.

use super::{Entry, Exported, Imported, track_from_label};
use crate::paths::{self, from_file_uri, relative_to, resolve};
use crate::{Playlist, PlaylistItem};

pub fn parse(text: &str, base_dir: Option<&str>) -> Imported {
    let mut out = Imported::default();
    let mut info: Option<(Option<u64>, String)> = None;
    let mut album: Option<String> = None;
    let mut artist: Option<String> = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if let Some(directive) = line.strip_prefix('#') {
            let (name, value) = directive.split_once(':').unwrap_or((directive, ""));
            match name.to_ascii_uppercase().as_str() {
                "EXTINF" => info = Some(extinf(value)),
                "PLAYLIST" if !value.trim().is_empty() => out.title = Some(value.trim().to_owned()),
                "EXTALB" => album = Some(value.trim().to_owned()).filter(|a| !a.is_empty()),
                "EXTART" => artist = Some(value.trim().to_owned()).filter(|a| !a.is_empty()),
                _ => {}
            }
            continue;
        }
        let location = match paths::scheme(line) {
            Some(s) if s.eq_ignore_ascii_case("file") => {
                from_file_uri(line).unwrap_or_else(|| line.to_owned())
            }
            Some(_) => line.to_owned(),
            None => resolve(base_dir, line),
        };
        let (duration_ms, label) = info.take().unwrap_or_default();
        let mut track = track_from_label(&label, Some(&location), duration_ms);
        track.album.clone_from(&album);
        track.album_artist.clone_from(&artist);
        // Без «Артист - Название» артист альбома — лучшее, что есть.
        if track.artists.is_empty() {
            track.artists.extend(artist.clone());
        }
        out.entries.push(Entry {
            location: Some(location),
            track,
        });
    }
    out
}

/// `#EXTINF:-1 tvg-id="…",Артист - Название` → (длительность, подпись).
fn extinf(value: &str) -> (Option<u64>, String) {
    let (head, label) = value.split_once(',').unwrap_or((value, ""));
    let seconds = head
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<f64>().ok());
    let duration = seconds
        .filter(|s| *s > 0.0)
        .map(|s| (s * 1000.0).round() as u64);
    (duration, label.trim().to_owned())
}

pub fn write(playlist: &Playlist, items: &[PlaylistItem], target_dir: Option<&str>) -> Exported {
    let mut text = String::from("#EXTM3U\n");
    text.push_str(&format!("#PLAYLIST:{}\n", one_line(&playlist.title)));
    let mut skipped = 0;
    for item in items {
        // Без файла на этом устройстве плееру нечего открыть.
        let Some(location) = &item.location else {
            skipped += 1;
            continue;
        };
        let seconds = item
            .track
            .duration_ms
            .map_or(-1, |ms| i64::try_from(ms.div_ceil(1000)).unwrap_or(-1));
        text.push_str(&format!(
            "#EXTINF:{seconds},{}\n",
            one_line(&crate::display_name(&item.track))
        ));
        let path = target_dir
            .and_then(|dir| relative_to(location, dir))
            .unwrap_or_else(|| location.clone());
        text.push_str(&path);
        text.push('\n');
    }
    Exported { text, skipped }
}

fn one_line(s: &str) -> String {
    s.replace(['\r', '\n'], " ")
}
