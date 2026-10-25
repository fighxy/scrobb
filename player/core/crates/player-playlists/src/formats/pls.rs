//! PLS — INI-подобный формат старых плееров и интернет-радио:
//! `File1=`, `Title1=`, `Length1=` (секунды, `-1` — неизвестно). Только импорт.

use std::collections::BTreeMap;

use super::{Entry, Imported, track_from_label};
use crate::paths::{self, from_file_uri, resolve};

pub fn parse(text: &str, base_dir: Option<&str>) -> Imported {
    #[derive(Default)]
    struct Raw {
        file: Option<String>,
        title: Option<String>,
        length: Option<u64>,
    }
    let mut raw: BTreeMap<u32, Raw> = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        let (field, n) = key.split_at(key.find(|c: char| c.is_ascii_digit()).unwrap_or(key.len()));
        let Ok(n) = n.parse::<u32>() else {
            continue;
        };
        let entry = raw.entry(n).or_default();
        match field {
            "file" => entry.file = Some(value.to_owned()),
            "title" => entry.title = Some(value.to_owned()),
            "length" => {
                entry.length = value
                    .parse::<i64>()
                    .ok()
                    .and_then(|s| u64::try_from(s).ok())
                    .filter(|s| *s > 0)
                    .map(|s| s * 1000)
            }
            _ => {}
        }
    }
    let entries = raw
        .into_values()
        .filter_map(|r| {
            let file = r.file?;
            let location = match paths::scheme(&file) {
                Some(s) if s.eq_ignore_ascii_case("file") => from_file_uri(&file).unwrap_or(file),
                Some(_) => file,
                None => resolve(base_dir, &file),
            };
            let track =
                track_from_label(r.title.as_deref().unwrap_or(""), Some(&location), r.length);
            Some(Entry {
                location: Some(location),
                track,
            })
        })
        .collect();
    Imported {
        entries,
        ..Imported::default()
    }
}
