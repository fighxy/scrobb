//! Текст «Артист - Название» построчно: так список принимают страница
//! импорта Яндекс Музыки и переносчики. Нумерация «1.» в начале строки
//! и строки-комментарии `#` пропускаются.

use super::{Entry, Exported, Imported, split_artist_title};
use crate::{PlaylistItem, TrackSnapshot, display_name};

pub fn parse(text: &str) -> Imported {
    let entries = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|line| {
            let line = strip_number(line);
            let (artist, title) = split_artist_title(line);
            Entry {
                location: None,
                track: TrackSnapshot {
                    title: title.to_owned(),
                    artists: artist.map(|a| vec![a.to_owned()]).unwrap_or_default(),
                    ..TrackSnapshot::default()
                },
            }
        })
        .filter(|e| !e.track.title.is_empty())
        .collect();
    Imported {
        entries,
        ..Imported::default()
    }
}

/// «12. Кино - Кукушка», «3) Кино - Кукушка» → без номера.
fn strip_number(line: &str) -> &str {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return line;
    }
    match line[digits..].strip_prefix(['.', ')']) {
        Some(rest) if rest.starts_with(' ') => rest.trim_start(),
        _ => line,
    }
}

pub fn write(items: &[PlaylistItem]) -> Exported {
    let mut text = String::new();
    for item in items {
        text.push_str(&display_name(&item.track).replace(['\r', '\n'], " "));
        text.push('\n');
    }
    Exported { text, skipped: 0 }
}
