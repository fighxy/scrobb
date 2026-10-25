//! Нормализация ключа трека «артист + название» — та же, что у сервера
//! (`docs/architecture.md` §5.3). Сверяется с векторами `contracts/match-key/v1.json`:
//! их же проходит бэкенд, поэтому плеер и сервер узнают один трек одинаково.

use unicode_normalization::UnicodeNormalization;

/// Вставки о приглашённых артистах. Порядок неважен: после метки всегда
/// должен идти пробел, поэтому «feat» не съест начало «featuring».
const FEAT: &[&str] = &[
    "featuring",
    "feat.",
    "feat",
    "ft.",
    "ft",
    "при участии",
    "при уч.",
    "при уч",
];

/// Строка для сравнения: NFKC, «ё» → «е», пробелы схлопнуты, регистр сложен
/// (casefold). Показывается всегда оригинал, это — только для ключа.
pub fn norm(s: &str) -> String {
    let s: String = s
        .nfkc()
        .map(|c| match c {
            'ё' => 'е',
            'Ё' => 'Е',
            c => c,
        })
        .collect();
    casefold(&s.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Главный артист: всё до «feat.», «ft.», «featuring», «при участии».
/// «&» и запятая не делят: «Simon & Garfunkel» — один артист.
pub fn primary_artist(artist: &str) -> &str {
    let mut from = 0;
    while let Some((start, end)) = find_marker(artist, from) {
        let before = &artist[..start];
        if before.ends_with(char::is_whitespace) && artist[end..].starts_with(char::is_whitespace) {
            return before.trim_end();
        }
        from = end;
    }
    artist
}

/// Название без вставок «(feat. …)», «[ft. …]», «feat. … до конца».
/// Остальные скобки остаются: «Song (featuring X) (Remix)» → «Song (Remix)».
pub fn strip_feat(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut rest = 0;
    let mut from = 0;
    while let Some((start, end)) = find_marker(title, from) {
        if !title[end..].starts_with(char::is_whitespace) {
            from = end;
            continue;
        }
        // Начало вставки — с пробелов и одной открывающей скобки перед меткой.
        let head = title[..start].trim_end();
        let head = head
            .strip_suffix(['(', '['])
            .map_or(head, str::trim_end)
            .len()
            .max(rest);
        // Конец — первая закрывающая скобка (включительно) или конец строки.
        let tail = &title[end..];
        let close = tail.find([')', ']']).map_or(title.len(), |i| end + i + 1);
        out.push_str(&title[rest..head]);
        rest = close;
        from = close;
    }
    out.push_str(&title[rest..]);
    out
}

/// Следующая метка из [`FEAT`] не раньше `from`, на границе слова, без учёта
/// регистра: «Left Behind» и «Lift Off» — не вставки.
fn find_marker(s: &str, from: usize) -> Option<(usize, usize)> {
    let mut prev: Option<char> = s[..from].chars().next_back();
    for (i, c) in s[from..].char_indices() {
        let at = from + i;
        let boundary = prev.is_none_or(|p| !is_word(p));
        prev = Some(c);
        if !boundary {
            continue;
        }
        if let Some(len) = FEAT
            .iter()
            .filter_map(|m| starts_with_ci(&s[at..], m))
            .max()
        {
            return Some((at, at + len));
        }
    }
    None
}

/// Длина префикса `s`, совпадающего с `marker` без учёта регистра.
fn starts_with_ci(s: &str, marker: &str) -> Option<usize> {
    let mut len = 0;
    let mut chars = s.chars();
    for m in marker.chars() {
        let c = chars.next()?;
        if !c.to_lowercase().eq(m.to_lowercase()) {
            return None;
        }
        len += c.len_utf8();
    }
    Some(len)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Сложение регистра: строчные плюс то, чего нет в `to_lowercase` (ß → ss).
fn casefold(s: &str) -> String {
    s.to_lowercase().replace('ß', "ss").replace('ς', "σ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_needs_a_following_space_and_a_word_boundary() {
        assert_eq!(strip_feat("Featuring"), "Featuring");
        assert_eq!(strip_feat("Song (feat.X)"), "Song (feat.X)");
        assert_eq!(strip_feat("Raft Song"), "Raft Song");
        assert_eq!(strip_feat("A (feat. B) C (ft. D)"), "A C");
        assert_eq!(primary_artist("Kanye West Feat. Rihanna"), "Kanye West");
        assert_eq!(primary_artist("feat. Someone"), "feat. Someone");
    }
}
