//! Пути в плейлистах: относительные — от папки файла плейлиста, `file://`-URI
//! для XSPF и JSPF. Только строки, без файловой системы: путь из плейлиста,
//! сохранённого на Windows, разбирается и на Linux, и в браузере.

/// Путь абсолютный: `/…`, `C:\…`, `C:/…`, `\\сервер\…`, или это URI со схемой.
pub fn is_absolute(path: &str) -> bool {
    path.starts_with('/') || path.starts_with("\\\\") || has_drive(path) || scheme(path).is_some()
}

/// Схема URI (`file`, `http`, `content`…), если это URI, а не путь.
pub fn scheme(s: &str) -> Option<&str> {
    let (scheme, rest) = s.split_once(':')?;
    let valid = scheme.len() > 1
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    (valid && rest.starts_with("//") || valid && scheme.eq_ignore_ascii_case("spotify"))
        .then_some(scheme)
}

fn has_drive(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// Разделитель пути в стиле `path`: `\` у путей Windows, иначе `/`.
fn separator(path: &str) -> char {
    if has_drive(path) || path.starts_with("\\\\") || (path.contains('\\') && !path.contains('/')) {
        '\\'
    } else {
        '/'
    }
}

/// Путь из плейлиста относительно папки плейлиста `base`. Абсолютные пути и
/// URI возвращаются как есть; `.` и `..` сворачиваются.
pub fn resolve(base: Option<&str>, path: &str) -> String {
    match base {
        Some(base) if !is_absolute(path) => {
            let sep = separator(base);
            let joined = format!("{}{sep}{path}", base.trim_end_matches(['/', '\\']));
            normalize(&joined)
        }
        _ => path.to_owned(),
    }
}

/// Свернуть `.` и `..`; разделители — в стиле пути.
pub fn normalize(path: &str) -> String {
    if scheme(path).is_some() {
        return path.to_owned();
    }
    let sep = separator(path);
    let (root, rest) = split_root(path);
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." if parts.last().is_some_and(|p| *p != "..") => {
                parts.pop();
            }
            ".." if !root.is_empty() => {}
            _ => parts.push(part),
        }
    }
    let body = parts.join(&sep.to_string());
    format!("{}{body}", root.replace(['/', '\\'], &sep.to_string()))
}

/// Корень пути (`/`, `C:\`, `\\`) и остальное.
fn split_root(path: &str) -> (&str, &str) {
    if path.starts_with("\\\\") {
        path.split_at(2)
    } else if has_drive(path) {
        let root = if path[2..].starts_with(['/', '\\']) {
            3
        } else {
            2
        };
        path.split_at(root)
    } else if path.starts_with(['/', '\\']) {
        path.split_at(1)
    } else {
        ("", path)
    }
}

/// `path` относительно папки `dir`, если у них общий корень (тот же диск);
/// иначе `None`. Разделители — в стиле `path`.
pub fn relative_to(path: &str, dir: &str) -> Option<String> {
    if scheme(path).is_some() || !is_absolute(path) || !is_absolute(dir) {
        return None;
    }
    let (root_a, rest_a) = split_root(path);
    let (root_b, rest_b) = split_root(dir);
    let windows = separator(path) == '\\';
    let same_root = if windows {
        root_a.eq_ignore_ascii_case(root_b)
            || root_a
                .replace('/', "\\")
                .eq_ignore_ascii_case(&root_b.replace('/', "\\"))
    } else {
        root_a == root_b
    };
    if !same_root {
        return None;
    }
    let parts = |s: &str| -> Vec<String> {
        s.split(['/', '\\'])
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let a = parts(rest_a);
    let b = parts(rest_b);
    let eq = |x: &String, y: &String| {
        if windows {
            x.eq_ignore_ascii_case(y)
        } else {
            x == y
        }
    };
    let common = a.iter().zip(&b).take_while(|(x, y)| eq(x, y)).count();
    // Совсем без общих папок (только корень) — абсолютный путь понятнее.
    if common == 0 {
        return None;
    }
    let sep = separator(path).to_string();
    let mut out: Vec<String> = vec!["..".into(); b.len() - common];
    out.extend(a[common..].iter().cloned());
    Some(out.join(&sep))
}

/// Путь → `file://`-URI (RFC 8089): кириллица и пробелы — в процентах.
pub fn to_file_uri(path: &str) -> String {
    if scheme(path).is_some() {
        return path.to_owned();
    }
    let path = path.replace('\\', "/");
    let path = if let Some(unc) = path.strip_prefix("//") {
        return format!("file://{}", encode(unc));
    } else if has_drive(&path) {
        format!("/{path}")
    } else {
        path
    };
    format!("file://{}", encode(&path))
}

/// `file://`-URI → путь этой системы; `None`, если это не `file://`.
pub fn from_file_uri(uri: &str) -> Option<String> {
    let rest = uri
        .strip_prefix("file://")
        .or_else(|| uri.strip_prefix("FILE://"))?;
    let decoded = decode(rest);
    // file:///C:/… → C:\…; file://сервер/папка → \\сервер\папка; file:///home → /home.
    if let Some(path) = decoded.strip_prefix('/') {
        if has_drive(path) {
            return Some(path.replace('/', "\\"));
        }
        return Some(format!("/{path}"));
    }
    if decoded.is_empty() {
        return None;
    }
    Some(format!("\\\\{}", decoded.replace('/', "\\")))
}

fn encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Проценты → байты → UTF-8 (неверные последовательности — заменой).
pub fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Имя файла без папки и расширения — название трека, если других данных нет.
pub fn file_stem(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}

/// Папка файла (без завершающего разделителя); `None` — у пути нет папки.
pub fn parent(path: &str) -> Option<&str> {
    let (dir, _) = path.rsplit_once(['/', '\\'])?;
    Some(if dir.is_empty() { "/" } else { dir })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_paths_in_both_styles() {
        assert_eq!(
            resolve(Some("/home/ivan/Плейлисты"), "../Музыка/Кино/01.flac"),
            "/home/ivan/Музыка/Кино/01.flac"
        );
        assert_eq!(resolve(Some("/m"), "./a/./b.mp3"), "/m/a/b.mp3");
        assert_eq!(
            resolve(Some("C:\\Users\\Иван\\Music"), "Кино\\01.flac"),
            "C:\\Users\\Иван\\Music\\Кино\\01.flac"
        );
        assert_eq!(
            resolve(Some("D:\\Lists"), "..\\Music\\a.mp3"),
            "D:\\Music\\a.mp3"
        );
        assert_eq!(resolve(Some("/m"), "/abs/a.flac"), "/abs/a.flac");
        assert_eq!(resolve(Some("/m"), "C:\\a.flac"), "C:\\a.flac");
        assert_eq!(
            resolve(Some("/m"), "http://radio.example/stream"),
            "http://radio.example/stream"
        );
        assert_eq!(resolve(None, "a.flac"), "a.flac");
        assert_eq!(normalize("/a/../../b"), "/b");
    }

    #[test]
    fn makes_paths_relative_only_with_a_common_folder() {
        assert_eq!(
            relative_to("/home/ivan/Музыка/Кино/01.flac", "/home/ivan/Плейлисты").as_deref(),
            Some("../Музыка/Кино/01.flac")
        );
        assert_eq!(
            relative_to("/m/a/b.flac", "/m/a").as_deref(),
            Some("b.flac")
        );
        assert_eq!(
            relative_to("C:\\Music\\a.mp3", "c:\\music\\lists").as_deref(),
            Some("..\\a.mp3")
        );
        assert_eq!(relative_to("D:\\Music\\a.mp3", "C:\\Lists"), None);
        assert_eq!(relative_to("/mnt/usb/a.flac", "/home/ivan"), None);
        assert_eq!(relative_to("content://media/1", "/home"), None);
    }

    #[test]
    fn file_uris_round_trip() {
        let unix = "/home/ivan/Музыка/Группа крови #1.flac";
        let uri = to_file_uri(unix);
        assert_eq!(
            uri,
            "file:///home/ivan/%D0%9C%D1%83%D0%B7%D1%8B%D0%BA%D0%B0/%D0%93%D1%80%D1%83%D0%BF%D0%BF%D0%B0%20%D0%BA%D1%80%D0%BE%D0%B2%D0%B8%20%231.flac"
        );
        assert_eq!(from_file_uri(&uri).as_deref(), Some(unix));
        let win = "C:\\Музыка\\a b.mp3";
        assert_eq!(
            to_file_uri(win),
            "file:///C:/%D0%9C%D1%83%D0%B7%D1%8B%D0%BA%D0%B0/a%20b.mp3"
        );
        assert_eq!(from_file_uri(&to_file_uri(win)).as_deref(), Some(win));
        assert_eq!(
            from_file_uri("file://nas/music/a.mp3").as_deref(),
            Some("\\\\nas\\music\\a.mp3")
        );
        assert_eq!(from_file_uri("http://x/a.mp3"), None);
        assert_eq!(decode("a%2"), "a%2");
    }

    #[test]
    fn schemes_and_stems() {
        assert_eq!(scheme("file:///a"), Some("file"));
        assert_eq!(
            scheme("spotify:track:4uLU6hMCjMI75M1A2tKUQC"),
            Some("spotify")
        );
        assert_eq!(scheme("C:\\a"), None);
        assert_eq!(scheme("Artist: Title"), None);
        assert_eq!(file_stem("C:\\m\\01 - Кукушка.flac"), "01 - Кукушка");
        assert_eq!(parent("/m/a.flac"), Some("/m"));
        assert_eq!(parent("/a.flac"), Some("/"));
        assert_eq!(parent("a.flac"), None);
    }
}
