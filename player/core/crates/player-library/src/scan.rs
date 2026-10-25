//! Сканер папок: какие аудиофайлы лежат в папке и во вложенных.
//!
//! Обходит дерево через порт `FileAccess` — на Android и Apple папка не обычный
//! путь, поэтому её содержимое отдаёт порт. Теги здесь не читаются: это
//! делает [`crate::tags`], а размер и время изменения из [`FileEntry`] позже
//! понадобятся базе библиотеки, чтобы не перечитывать неизменившиеся файлы.

use player_domain::MediaUri;
use player_ports::{FileAccess, FileEntry, PortError};

/// Расширения файлов, которые играет движок (Symphonia 0.6): MP3, FLAC, AAC и
/// ALAC в MP4, Vorbis в Ogg, WAV, AIFF, CAF, Matroska. Opus в Symphonia пока
/// нет — такие файлы не добавляются, чтобы не захламлять список неиграющим.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "m4a", "mp4", "aac", "ogg", "oga", "wav", "wave", "aif", "aiff", "aifc", "caf",
    "mka",
];

/// Глубже не спускаемся: защита от слишком глубоких и зацикленных деревьев.
pub const MAX_DEPTH: usize = 32;

/// Аудиофайл ли это — по расширению, без учёта регистра.
pub fn is_audio(uri: &str) -> bool {
    let name = uri.rsplit(['/', '\\']).next().unwrap_or(uri);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => AUDIO_EXTENSIONS
            .iter()
            .any(|known| known.eq_ignore_ascii_case(ext)),
        _ => false,
    }
}

/// Все аудиофайлы в `root` и вложенных папках, упорядоченные по пути без учёта
/// регистра (обычно это «артист / альбом / номер — название»).
///
/// Скрытые файлы и папки (имя с точки) пропускаются. Ошибка — только если не
/// читается сама `root`; недоступная вложенная папка просто пропускается.
/// `progress` получает число найденных файлов по мере обхода.
pub fn audio_files(
    files: &dyn FileAccess,
    root: &MediaUri,
    mut progress: impl FnMut(usize),
) -> Result<Vec<FileEntry>, PortError> {
    let mut found = Vec::new();
    let mut pending = vec![(files.list(root)?, 0)];
    while let Some((entries, depth)) = pending.pop() {
        for entry in entries {
            if is_hidden(&entry.uri.0) {
                continue;
            }
            if entry.is_dir {
                if depth < MAX_DEPTH
                    && let Ok(children) = files.list(&entry.uri)
                {
                    pending.push((children, depth + 1));
                }
            } else if is_audio(&entry.uri.0) {
                found.push(entry);
                progress(found.len());
            }
        }
    }
    found.sort_by_cached_key(|e| e.uri.0.to_lowercase());
    Ok(found)
}

/// Лежит ли `uri` внутри папки `folder` (или совпадает с ней).
pub fn is_inside(uri: &str, folder: &str) -> bool {
    let folder = folder.trim_end_matches(['/', '\\']);
    uri == folder
        || uri
            .strip_prefix(folder)
            .is_some_and(|rest| rest.starts_with(['/', '\\']))
}

fn is_hidden(uri: &str) -> bool {
    uri.rsplit(['/', '\\'])
        .next()
        .is_some_and(|name| name.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_testkit::disk::{Disk, TempDir};
    use std::path::Path;

    fn names(root: &Path, entries: &[FileEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|e| {
                Path::new(&e.uri.0)
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    #[test]
    fn finds_audio_in_nested_folders_in_path_order() {
        let dir = TempDir::new("nested");
        for f in [
            "Кино/Группа крови/02 - Закрой за мной дверь.flac",
            "Кино/Группа крови/01 - Группа крови.FLAC",
            "Кино/Группа крови/cover.jpg",
            "Кино/Группа крови/.01 - скрытый.flac",
            "аквариум/Десять стрел/01 - Город золотой.mp3",
            "одиночный.m4a",
            "заметки.txt",
            "голос.opus",
            ".cache/спрятано.flac",
        ] {
            dir.file(f);
        }
        let mut seen = Vec::new();
        let found = audio_files(&Disk, &dir.uri(), |n| seen.push(n)).unwrap();
        // Без учёта регистра: «аквариум» раньше «Кино» (с учётом — наоборот,
        // заглавная «К» в Юникоде стоит раньше строчной «а»).
        assert_eq!(
            names(&dir.0, &found),
            [
                "аквариум/Десять стрел/01 - Город золотой.mp3",
                "Кино/Группа крови/01 - Группа крови.FLAC",
                "Кино/Группа крови/02 - Закрой за мной дверь.flac",
                "одиночный.m4a",
            ]
        );
        assert_eq!(seen, [1, 2, 3, 4]);
        assert!(
            found
                .iter()
                .all(|e| !e.is_dir && e.size == 1 && e.modified_ms > 0)
        );
    }

    #[test]
    fn missing_root_is_an_error_and_empty_folder_is_not() {
        assert!(audio_files(&Disk, &MediaUri("/nope/nope".into()), |_| {}).is_err());
        let dir = TempDir::new("empty");
        assert!(audio_files(&Disk, &dir.uri(), |_| {}).unwrap().is_empty());
    }

    #[test]
    fn recognises_extensions_and_folders() {
        assert!(is_audio("/m/a.FLAC") && is_audio("C:\\m\\b.Mp3") && is_audio("c.aiff"));
        assert!(!is_audio("/m/.flac") && !is_audio("/m/a.opus") && !is_audio("/m/flac"));
        assert!(is_inside("/music/Кино/a.flac", "/music"));
        assert!(is_inside("/music/Кино/a.flac", "/music/"));
        assert!(is_inside("C:\\Музыка\\a.mp3", "C:\\Музыка"));
        assert!(!is_inside("/music2/a.flac", "/music"));
        assert!(is_inside("/music", "/music"));
    }
}
