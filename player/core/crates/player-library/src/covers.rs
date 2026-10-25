//! Обложки: картинка, встроенная в теги файла, а если её нет — картинка
//! рядом с треком (`cover.jpg`, `folder.jpg`…).
//!
//! Всё через порт `FileAccess`, как и теги. Уменьшение и кэш — дело
//! оболочки: на мобильных для этого есть системные средства.

use std::io::{BufReader, Read};

use lofty::config::ParseOptions;
use lofty::picture::PictureType;
use lofty::prelude::TaggedFileExt;
use lofty::probe::Probe;
use lofty::tag::Tag;
use player_domain::MediaUri;
use player_ports::{FileAccess, PortError};

/// Имена файлов-обложек в папке альбома, по убыванию приоритета.
pub const FOLDER_NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart"];
/// Расширения файлов-обложек, по убыванию приоритета.
pub const FOLDER_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png"];
/// Картинки больше — не обложка, а что-то другое (скан буклета и т. п.).
pub const MAX_FOLDER_IMAGE: u64 = 16 * 1024 * 1024;

/// Картинка обложки как есть, без перекодирования.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    /// MIME-тип, если известен (`image/jpeg`, `image/png`).
    pub mime: Option<String>,
    /// Байты картинки.
    pub data: Vec<u8>,
}

/// Обложка трека: встроенная, иначе из папки.
///
/// Ошибка — только если не открылся сам трек.
pub fn find(files: &dyn FileAccess, track: &MediaUri) -> Result<Option<Picture>, PortError> {
    if let Some(picture) = embedded(files, track)? {
        return Ok(Some(picture));
    }
    Ok(folder_image(files, track).and_then(|uri| read_image(files, &uri)))
}

/// Встроенная обложка: «передняя обложка», а если такой нет — первая картинка.
/// Сначала основной тег файла, потом остальные (ID3v2 и APE в одном MP3 и т. п.).
pub fn embedded(files: &dyn FileAccess, track: &MediaUri) -> Result<Option<Picture>, PortError> {
    let stream = files.open(track)?;
    let Ok(tagged) = Probe::new(BufReader::new(stream))
        .options(ParseOptions::new().read_properties(false))
        .guess_file_type()
        .map_err(drop)
        .and_then(|probe| probe.read().map_err(drop))
    else {
        return Ok(None);
    };
    fn front(tag: &Tag) -> Option<&lofty::picture::Picture> {
        tag.get_picture_type(PictureType::CoverFront)
    }
    fn any(tag: &Tag) -> Option<&lofty::picture::Picture> {
        tag.pictures().first()
    }
    let primary = tagged.primary_tag();
    let picture = primary
        .and_then(front)
        .or_else(|| tagged.tags().iter().find_map(front))
        .or_else(|| primary.and_then(any))
        .or_else(|| tagged.tags().iter().find_map(any));
    Ok(picture.map(|p| Picture {
        mime: p.mime_type().map(|m| m.as_str().to_owned()),
        data: p.data().to_vec(),
    }))
}

/// Файл-обложка в папке трека (без учёта регистра, по приоритету
/// [`FOLDER_NAMES`], затем [`FOLDER_EXTENSIONS`]). `None`, если такого нет
/// или папка не читается.
pub fn folder_image(files: &dyn FileAccess, track: &MediaUri) -> Option<MediaUri> {
    let (dir, _) = track.0.rsplit_once(['/', '\\'])?;
    let dir = if dir.is_empty() { "/" } else { dir };
    let entries = files.list(&MediaUri(dir.to_owned())).ok()?;
    entries
        .into_iter()
        .filter(|e| !e.is_dir && e.size <= MAX_FOLDER_IMAGE)
        .filter_map(|e| rank(&e.uri.0).map(|rank| (rank, e.uri)))
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, uri)| uri)
}

/// Место имени файла в приоритете или `None`, если это не обложка.
fn rank(uri: &str) -> Option<(usize, usize)> {
    let name = uri.rsplit(['/', '\\']).next()?.to_lowercase();
    let (stem, ext) = name.rsplit_once('.')?;
    Some((
        FOLDER_NAMES.iter().position(|n| *n == stem)?,
        FOLDER_EXTENSIONS.iter().position(|e| *e == ext)?,
    ))
}

fn read_image(files: &dyn FileAccess, uri: &MediaUri) -> Option<Picture> {
    let mut data = Vec::new();
    files
        .open(uri)
        .ok()?
        .take(MAX_FOLDER_IMAGE)
        .read_to_end(&mut data)
        .ok()?;
    let mime = if uri.0.to_lowercase().ends_with(".png") {
        "image/png"
    } else {
        "image/jpeg"
    };
    Some(Picture {
        mime: Some(mime.to_owned()),
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_testkit::Fixtures;
    use player_testkit::disk::{Disk, TempDir};

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

    #[test]
    fn embedded_front_cover_is_read_as_is() {
        let picture = embedded(&Fixtures, &MediaUri("covered.flac".into()))
            .unwrap()
            .unwrap();
        assert_eq!(picture.mime.as_deref(), Some("image/png"));
        assert!(picture.data.starts_with(PNG), "картинка PNG без изменений");
        assert!(
            embedded(&Fixtures, &MediaUri("tagged.flac".into()))
                .unwrap()
                .is_none()
        );
        assert!(embedded(&Fixtures, &MediaUri("nope.flac".into())).is_err());
    }

    #[test]
    fn folder_image_by_priority_and_case() {
        let dir = TempDir::new("covers");
        for f in [
            "Альбом/01.flac",
            "Альбом/Folder.JPG",
            "Альбом/cover.png",
            "Альбом/back.jpg",
            "Альбом/cover.txt",
            "Альбом/scans/cover.jpg",
            "Пусто/01.flac",
            "Пусто/booklet.jpg",
            // Папка с именем обложки — не обложка.
            "Странно/cover.jpg/inside.txt",
            "Странно/01.flac",
            "Странно/front.png",
        ] {
            dir.file(f);
        }
        let found = |track: &str| folder_image(&Disk, &dir.uri_of(track));
        // «cover» важнее «folder», даже если у него расширение хуже.
        assert_eq!(
            found("Альбом/01.flac"),
            Some(dir.uri_of("Альбом/cover.png"))
        );
        assert_eq!(found("Пусто/01.flac"), None);
        assert_eq!(
            found("Странно/01.flac"),
            Some(dir.uri_of("Странно/front.png"))
        );
        assert_eq!(found("Нет такой/01.flac"), None);
        assert_eq!(folder_image(&Disk, &MediaUri("01.flac".into())), None);

        std::fs::remove_file(dir.path().join("Альбом/cover.png")).unwrap();
        assert_eq!(
            found("Альбом/01.flac"),
            Some(dir.uri_of("Альбом/Folder.JPG"))
        );
    }

    #[test]
    fn find_prefers_embedded_then_folder() {
        let dir = TempDir::new("find");
        let covered = std::fs::read(format!("{}/covered.flac", player_testkit::FIXTURES)).unwrap();
        let plain = std::fs::read(format!("{}/tagged.flac", player_testkit::FIXTURES)).unwrap();
        dir.write("a/covered.flac", &covered);
        dir.write("a/plain.flac", &plain);
        dir.write("a/cover.jpg", b"\xff\xd8\xff jpeg");
        dir.write("b/plain.flac", &plain);
        dir.write("c/plain.flac", &plain);
        dir.write("c/Cover.PNG", PNG);

        let embedded = find(&Disk, &dir.uri_of("a/covered.flac")).unwrap().unwrap();
        assert!(embedded.data.starts_with(PNG));
        let folder = find(&Disk, &dir.uri_of("a/plain.flac")).unwrap().unwrap();
        assert_eq!(folder.mime.as_deref(), Some("image/jpeg"));
        assert_eq!(folder.data, b"\xff\xd8\xff jpeg");
        let png = find(&Disk, &dir.uri_of("c/plain.flac")).unwrap().unwrap();
        assert_eq!(png.mime.as_deref(), Some("image/png"));
        assert_eq!(find(&Disk, &dir.uri_of("b/plain.flac")).unwrap(), None);
        assert!(find(&Disk, &dir.uri_of("b/nope.flac")).is_err());
    }

    #[test]
    fn ranks_only_known_names() {
        assert_eq!(rank("/m/COVER.JPEG"), Some((0, 1)));
        assert_eq!(rank("C:\\m\\AlbumArt.png"), Some((4, 2)));
        assert_eq!(rank("/m/cover"), None);
        assert_eq!(rank("/m/cover.gif"), None);
        assert_eq!(rank("/m/my cover.jpg"), None);
    }
}
