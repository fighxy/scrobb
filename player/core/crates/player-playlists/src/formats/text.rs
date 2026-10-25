//! Общее для текстовых форматов: кодировка, «Артист - Название», дата ISO 8601.

/// Верхняя половина Windows-1251 (0x80–0xBF); 0xC0–0xFF — подряд «А»–«я».
/// Таблица — из кодека Python `cp1251`, 0x98 в нём не определён.
const CP1251_HIGH: &str = "\u{0402}\u{0403}\u{201A}\u{0453}\u{201E}\u{2026}\u{2020}\u{2021}\u{20AC}\u{2030}\u{0409}\u{2039}\u{040A}\u{040C}\u{040B}\u{040F}\u{0452}\u{2018}\u{2019}\u{201C}\u{201D}\u{2022}\u{2013}\u{2014}\u{FFFD}\u{2122}\u{0459}\u{203A}\u{045A}\u{045C}\u{045B}\u{045F}\u{00A0}\u{040E}\u{045E}\u{0408}\u{00A4}\u{0490}\u{00A6}\u{00A7}\u{0401}\u{00A9}\u{0404}\u{00AB}\u{00AC}\u{00AD}\u{00AE}\u{0407}\u{00B0}\u{00B1}\u{0406}\u{0456}\u{0491}\u{00B5}\u{00B6}\u{00B7}\u{0451}\u{2116}\u{0454}\u{00BB}\u{0458}\u{0405}\u{0455}\u{0457}";

/// Байты файла → текст. UTF-8 (с BOM или без), UTF-16 с BOM; если это не
/// UTF-8 — Windows-1251: так сохраняют M3U старые AIMP и Winamp на русской
/// Windows. Других однобайтовых кодировок по содержимому не отличить.
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        return utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        return utf16(rest, u16::from_be_bytes);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => cp1251(bytes),
    }
}

fn utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| unit([c[0], c[1]])).collect();
    String::from_utf16_lossy(&units)
}

fn cp1251(bytes: &[u8]) -> String {
    let high: Vec<char> = CP1251_HIGH.chars().collect();
    bytes
        .iter()
        .map(|&b| match b {
            0x00..=0x7F => b as char,
            0x80..=0xBF => high[usize::from(b - 0x80)],
            _ => char::from_u32(0x0410 + u32::from(b - 0xC0)).unwrap_or('\u{FFFD}'),
        })
        .collect()
}

/// «Артист - Название» → (артист, название). Разделитель — первый из
/// ` - `, ` — `, ` – `; без него всё — название.
pub fn split_artist_title(label: &str) -> (Option<&str>, &str) {
    let label = label.trim();
    let split = [" - ", " — ", " – "]
        .iter()
        .filter_map(|sep| label.find(sep).map(|i| (i, sep.len())))
        .min_by_key(|(i, _)| *i);
    match split {
        Some((i, len)) => {
            let artist = label[..i].trim();
            let title = label[i + len..].trim();
            (Some(artist).filter(|a| !a.is_empty()), title)
        }
        None => (None, label),
    }
}

/// Миллисекунды Unix → `2026-10-25T08:31:05Z`.
pub fn iso_utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Гражданская дата из числа дней (алгоритм Говарда Хиннанта).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_bom_utf16_and_cp1251() {
        assert_eq!(decode_text("Кино".as_bytes()), "Кино");
        assert_eq!(decode_text(b"\xEF\xBB\xBF#EXTM3U"), "#EXTM3U");
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("Ёж".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode_text(&le), "Ёж");
        // «Кино — Ёлка» в Windows-1251.
        assert_eq!(
            decode_text(b"\xCA\xE8\xED\xEE \x97 \xA8\xEB\xEA\xE0"),
            "Кино — Ёлка"
        );
        assert_eq!(decode_text(b"\xFF\xE0"), "яа");
    }

    #[test]
    fn splits_artist_and_title() {
        assert_eq!(
            split_artist_title("Кино - Группа крови"),
            (Some("Кино"), "Группа крови")
        );
        assert_eq!(
            split_artist_title("Кино — Звезда - по имени Солнце"),
            (Some("Кино"), "Звезда - по имени Солнце")
        );
        assert_eq!(
            split_artist_title("Би-2 – Полковнику"),
            (Some("Би-2"), "Полковнику")
        );
        assert_eq!(
            split_artist_title("Просто название"),
            (None, "Просто название")
        );
        assert_eq!(
            split_artist_title(" - Без артиста"),
            (None, "- Без артиста")
        );
    }

    #[test]
    fn formats_utc_dates() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_761_381_065_000), "2025-10-25T08:31:05Z");
        assert_eq!(iso_utc(951_782_400_000), "2000-02-29T00:00:00Z");
    }
}
