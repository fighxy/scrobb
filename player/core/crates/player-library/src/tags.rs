//! Чтение тегов файла (lofty) через порт `FileAccess`: на Android и Apple
//! путь к файлу — не обычный путь, поэтому файл открывает порт, а не ядро.
//!
//! Нужны и для экрана, и для скробблинга: по длительности считается порог (§18.7).

use std::io::BufReader;

use lofty::prelude::{Accessor, AudioFile, ItemKey, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::Tag;
use player_domain::{MediaUri, ReplayGain, Track, TrackId};
use player_ports::{FileAccess, PortError};

/// Трек из тегов и заголовка файла.
///
/// Ошибка — только если файл не открылся. Если теги не разобрались,
/// название берётся из имени файла, остальное остаётся пустым.
pub fn read_track(files: &dyn FileAccess, uri: &MediaUri, id: TrackId) -> Result<Track, PortError> {
    let stream = files.open(uri)?;
    let mut track = Track {
        id,
        uri: uri.clone(),
        title: file_stem(&uri.0).to_owned(),
        artists: Vec::new(),
        album: None,
        album_artist: None,
        duration_ms: None,
        track_no: None,
        disc_no: None,
        recording_mbid: None,
        replay_gain: ReplayGain::default(),
    };
    let Ok(tagged) = Probe::new(BufReader::new(stream))
        .guess_file_type()
        .map_err(drop)
        .and_then(|probe| probe.read().map_err(drop))
    else {
        return Ok(track);
    };
    let duration = tagged.properties().duration().as_millis();
    track.duration_ms = u64::try_from(duration).ok().filter(|&ms| ms > 0);
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        apply(&mut track, tag);
    }
    Ok(track)
}

fn apply(track: &mut Track, tag: &Tag) {
    if let Some(title) = tag.title().filter(|t| !t.trim().is_empty()) {
        track.title = title.into_owned();
    }
    track.artists = tag
        .get_strings(ItemKey::TrackArtist)
        .filter(|a| !a.trim().is_empty())
        .map(str::to_owned)
        .collect();
    track.album = tag.album().map(|a| a.into_owned());
    track.album_artist = text(tag, ItemKey::AlbumArtist);
    track.track_no = tag.track();
    track.disc_no = tag.disk();
    track.recording_mbid = text(tag, ItemKey::MusicBrainzRecordingId);
    track.replay_gain = ReplayGain {
        track_gain_db: number(tag, ItemKey::ReplayGainTrackGain),
        track_peak: number(tag, ItemKey::ReplayGainTrackPeak),
        album_gain_db: number(tag, ItemKey::ReplayGainAlbumGain),
        album_peak: number(tag, ItemKey::ReplayGainAlbumPeak),
    };
}

fn text(tag: &Tag, key: ItemKey) -> Option<String> {
    tag.get_string(key)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Число из тега вроде `-6.50 dB` или `0.988`.
fn number(tag: &Tag, key: ItemKey) -> Option<f32> {
    let value = tag.get_string(key)?.trim();
    let value = value
        .strip_suffix("dB")
        .or_else(|| value.strip_suffix("db"))
        .unwrap_or(value);
    value.trim().parse().ok()
}

/// Имя файла без папки и расширения — для путей и для `content://`-адресов.
fn file_stem(uri: &str) -> &str {
    let name = uri.rsplit(['/', '\\']).next().unwrap_or(uri);
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_testkit::Fixtures;

    #[test]
    fn reads_vorbis_comments_and_duration() {
        let track = read_track(&Fixtures, &MediaUri("tagged.flac".into()), TrackId(3)).unwrap();
        assert_eq!(track.id, TrackId(3));
        assert_eq!(track.title, "Кукушка");
        assert_eq!(track.artists, ["Кино"]);
        assert_eq!(track.album.as_deref(), Some("Чёрный альбом"));
        assert_eq!(track.album_artist.as_deref(), Some("Кино"));
        assert_eq!((track.track_no, track.disc_no), (Some(4), Some(1)));
        assert_eq!(
            track.recording_mbid.as_deref(),
            Some("6f1c2f5b-1c5c-4d6b-9a3e-0a1b2c3d4e5f")
        );
        assert_eq!(
            track.replay_gain,
            ReplayGain {
                track_gain_db: Some(-6.5),
                track_peak: Some(0.988),
                album_gain_db: Some(-7.25),
                album_peak: Some(0.999),
            }
        );
        // 11 113 кадров при 44,1 кГц = 251,99 мс; lofty отбрасывает доли миллисекунды.
        assert_eq!(track.duration_ms, Some(251));
    }

    #[test]
    fn untagged_file_is_named_after_itself() {
        let track = read_track(&Fixtures, &MediaUri("a.flac".into()), TrackId(1)).unwrap();
        assert_eq!(track.title, "a");
        assert!(track.artists.is_empty() && track.album.is_none());
        assert_eq!(track.duration_ms, Some(251));
    }

    #[test]
    fn missing_file_is_an_error_and_odd_names_still_have_a_title() {
        assert!(read_track(&Fixtures, &MediaUri("nope.flac".into()), TrackId(1)).is_err());
        assert_eq!(
            file_stem("C:\\Музыка\\Кино - Кукушка.mp3"),
            "Кино - Кукушка"
        );
        assert_eq!(file_stem("content://media/42"), "42");
        assert_eq!(file_stem("/music/.hidden"), ".hidden");
    }
}
