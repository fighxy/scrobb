//! Поиск трека библиотеки по тому, что известно из плейлиста или другого
//! устройства: путь, MBID, ISRC, «артист + название». Так плейлист из файла,
//! из ListenBrainz или с другого компьютера находит свои треки здесь.
//!
//! Порядок — от надёжного к приблизительному:
//! 1. путь к файлу (точно, затем без учёта регистра и вида разделителей);
//! 2. MusicBrainz recording ID;
//! 3. ISRC;
//! 4. ключ «артист + название», общий с сервером (`scrobb_sync::TrackKey`).
//!    Если кандидатов несколько — тот же альбом, затем ближе по длительности.
//!    Если у обоих длительность известна и расходится больше чем на 10 с, это
//!    другая запись (концертная, ремикс) — лучше не найти, чем подменить.

use std::collections::HashMap;

use player_domain::{Track, TrackId, TrackSnapshot};
use scrobb_sync::{TrackKey, norm};

/// Насколько может расходиться длительность у одной записи.
const SAME_RECORDING_MS: u64 = 10_000;

/// Что известно о треке, который ищем.
#[derive(Debug, Clone, Copy)]
pub struct TrackQuery<'a> {
    /// Путь или URI на этом устройстве.
    pub location: Option<&'a str>,
    /// Метаданные и идентификаторы.
    pub track: &'a TrackSnapshot,
}

#[derive(Debug, Clone)]
struct Meta {
    album: Option<String>,
    duration_ms: Option<u64>,
}

/// Индекс библиотеки для [`TrackQuery`].
#[derive(Debug, Default)]
pub struct LibraryIndex {
    by_uri: HashMap<String, TrackId>,
    by_folded_uri: HashMap<String, TrackId>,
    by_mbid: HashMap<String, Vec<TrackId>>,
    by_isrc: HashMap<String, Vec<TrackId>>,
    by_key: HashMap<(String, String), Vec<TrackId>>,
    meta: HashMap<TrackId, Meta>,
}

/// Путь без учёта регистра и вида разделителей: `C:\Music\A.flac` = `c:/music/a.flac`.
fn fold_uri(uri: &str) -> String {
    uri.replace('\\', "/").to_lowercase()
}

/// Ключи «артист + название» трека библиотеки: по первому артисту и, если
/// артистов несколько, по всем вместе — как их пишут стриминги.
fn library_keys(track: &Track) -> Vec<(String, String)> {
    let mut artists: Vec<String> = track.artists.first().cloned().into_iter().collect();
    if track.artists.len() > 1 {
        artists.push(track.artists.join(", "));
    }
    if artists.is_empty() {
        artists.extend(track.album_artist.clone());
    }
    keys(&artists, &track.title)
}

/// Ключи для поиска: все артисты вместе, первый, и первый до запятой
/// («Кино, Виктор Цой» в CSV стриминга → «Кино»).
fn query_keys(track: &TrackSnapshot) -> Vec<(String, String)> {
    let mut artists = Vec::new();
    if !track.artists.is_empty() {
        artists.push(track.artists.join(", "));
        artists.extend(track.artists.first().cloned());
        if let Some((first, _)) = track.artists[0].split_once(',') {
            artists.push(first.to_owned());
        }
    }
    artists.extend(track.album_artist.clone());
    keys(&artists, &track.title)
}

fn keys(artists: &[String], title: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for artist in artists {
        let key = TrackKey::new(artist, title, None);
        let pair = (key.artist, key.title);
        if !pair.1.is_empty() && !out.contains(&pair) {
            out.push(pair);
        }
    }
    out
}

fn push(map: &mut HashMap<String, Vec<TrackId>>, key: String, id: TrackId) {
    let ids = map.entry(key).or_default();
    if !ids.contains(&id) {
        ids.push(id);
    }
}

fn pull<K: std::hash::Hash + Eq>(map: &mut HashMap<K, Vec<TrackId>>, key: &K, id: TrackId) {
    if let Some(ids) = map.get_mut(key) {
        ids.retain(|x| *x != id);
        if ids.is_empty() {
            map.remove(key);
        }
    }
}

impl LibraryIndex {
    /// Добавить трек (если трек с этим id уже был — сначала уберите его).
    pub fn insert(&mut self, track: &Track) {
        self.by_uri.insert(track.uri.0.clone(), track.id);
        self.by_folded_uri.insert(fold_uri(&track.uri.0), track.id);
        if let Some(mbid) = &track.recording_mbid {
            push(&mut self.by_mbid, mbid.to_lowercase(), track.id);
        }
        if let Some(isrc) = &track.isrc {
            push(&mut self.by_isrc, isrc.clone(), track.id);
        }
        for key in library_keys(track) {
            let ids = self.by_key.entry(key).or_default();
            if !ids.contains(&track.id) {
                ids.push(track.id);
            }
        }
        self.meta.insert(
            track.id,
            Meta {
                album: track.album.as_deref().map(norm),
                duration_ms: track.duration_ms,
            },
        );
    }

    /// Убрать трек.
    pub fn remove(&mut self, track: &Track) {
        if self.by_uri.get(&track.uri.0) == Some(&track.id) {
            self.by_uri.remove(&track.uri.0);
        }
        let folded = fold_uri(&track.uri.0);
        if self.by_folded_uri.get(&folded) == Some(&track.id) {
            self.by_folded_uri.remove(&folded);
        }
        if let Some(mbid) = &track.recording_mbid {
            pull(&mut self.by_mbid, &mbid.to_lowercase(), track.id);
        }
        if let Some(isrc) = &track.isrc {
            pull(&mut self.by_isrc, isrc, track.id);
        }
        for key in library_keys(track) {
            pull(&mut self.by_key, &key, track.id);
        }
        self.meta.remove(&track.id);
    }

    /// Найти трек библиотеки.
    pub fn resolve(&self, query: &TrackQuery<'_>) -> Option<TrackId> {
        if let Some(location) = query.location
            && let Some(id) = self
                .by_uri
                .get(location)
                .or_else(|| self.by_folded_uri.get(&fold_uri(location)))
        {
            return Some(*id);
        }
        let t = query.track;
        if let Some(id) = t
            .recording_mbid
            .as_ref()
            .and_then(|mbid| self.by_mbid.get(&mbid.to_lowercase()))
            .and_then(|ids| self.best(ids, t))
        {
            return Some(id);
        }
        if let Some(id) = t
            .isrc()
            .and_then(|isrc| self.by_isrc.get(isrc))
            .and_then(|ids| self.best(ids, t))
        {
            return Some(id);
        }
        query_keys(t)
            .iter()
            .find_map(|key| self.by_key.get(key).and_then(|ids| self.best(ids, t)))
    }

    /// Лучший кандидат: тот же альбом, затем ближе по длительности; явно
    /// другие записи (длительность расходится больше [`SAME_RECORDING_MS`]) — нет.
    fn best(&self, ids: &[TrackId], t: &TrackSnapshot) -> Option<TrackId> {
        let album = t.album.as_deref().map(norm);
        ids.iter()
            .filter_map(|id| Some((*id, self.meta.get(id)?)))
            .filter(|(_, m)| match (m.duration_ms, t.duration_ms) {
                (Some(a), Some(b)) => a.abs_diff(b) <= SAME_RECORDING_MS,
                _ => true,
            })
            .min_by_key(|(id, m)| {
                let other_album = album.is_some() && m.album != album;
                let gap = match (m.duration_ms, t.duration_ms) {
                    (Some(a), Some(b)) => a.abs_diff(b),
                    _ => SAME_RECORDING_MS,
                };
                (other_album, gap, *id)
            })
            .map(|(id, _)| id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_domain::{MediaUri, ReplayGain};

    fn track(id: u64, uri: &str, artist: &str, title: &str, album: &str, ms: u64) -> Track {
        Track {
            id: TrackId(id),
            uri: MediaUri(uri.into()),
            title: title.into(),
            artists: vec![artist.into()],
            album: Some(album.into()).filter(|a: &String| !a.is_empty()),
            album_artist: None,
            duration_ms: Some(ms),
            track_no: None,
            disc_no: None,
            recording_mbid: None,
            isrc: None,
            replay_gain: ReplayGain::default(),
        }
    }

    fn snap(artist: &str, title: &str) -> TrackSnapshot {
        TrackSnapshot {
            title: title.into(),
            artists: if artist.is_empty() {
                vec![]
            } else {
                vec![artist.into()]
            },
            ..TrackSnapshot::default()
        }
    }

    fn find(index: &LibraryIndex, location: Option<&str>, t: &TrackSnapshot) -> Option<u64> {
        index
            .resolve(&TrackQuery { location, track: t })
            .map(|id| id.0)
    }

    fn library() -> LibraryIndex {
        let mut index = LibraryIndex::default();
        let mut a = track(
            1,
            "C:\\Музыка\\Кино\\Кукушка.flac",
            "Кино",
            "Кукушка",
            "Чёрный альбом",
            397_000,
        );
        a.recording_mbid = Some("6F1C2F5B-1C5C-4D6B-9A3E-0A1B2C3D4E5F".into());
        a.isrc = Some("RUA018700012".into());
        index.insert(&a);
        // Та же песня на сборнике — длительность почти та же.
        index.insert(&track(
            2,
            "/m/best/Кукушка.mp3",
            "КИНО",
            "Кукушка",
            "Лучшее",
            396_000,
        ));
        // Концертная запись — сильно длиннее.
        index.insert(&track(
            3,
            "/m/live/Кукушка.mp3",
            "Кино",
            "Кукушка",
            "Концерт",
            460_000,
        ));
        index.insert(&track(
            4,
            "/m/Звезда.flac",
            "Кино",
            "Звезда по имени Солнце (feat. Никто)",
            "Звезда",
            225_000,
        ));
        index
    }

    #[test]
    fn location_beats_identifiers_and_ignores_case_and_slashes() {
        let index = library();
        let mut t = snap("Кино", "Кукушка");
        t.recording_mbid = Some("6f1c2f5b-1c5c-4d6b-9a3e-0a1b2c3d4e5f".into());
        assert_eq!(find(&index, Some("/m/best/Кукушка.mp3"), &t), Some(2));
        assert_eq!(
            find(&index, Some("c:/музыка/кино/кукушка.flac"), &snap("", "")),
            Some(1)
        );
        // Пути нет в библиотеке — по MBID (регистр MBID не важен).
        assert_eq!(find(&index, Some("/нет/такого.flac"), &t), Some(1));
    }

    #[test]
    fn isrc_then_key_with_album_and_duration() {
        let index = library();
        let mut by_isrc = snap("Другой артист", "Другое название");
        by_isrc.ext_ids.insert("isrc".into(), "RUA018700012".into());
        assert_eq!(find(&index, None, &by_isrc), Some(1));

        let mut best = snap("кино", "КУКУШКА");
        best.album = Some("лучшее".into());
        assert_eq!(find(&index, None, &best), Some(2), "тот же альбом важнее");
        let mut close = snap("Кино", "Кукушка");
        close.duration_ms = Some(395_500);
        assert_eq!(find(&index, None, &close), Some(2), "ближе по длительности");
        let mut live = snap("Кино", "Кукушка");
        live.duration_ms = Some(458_000);
        assert_eq!(find(&index, None, &live), Some(3));
        let mut nothing_close = snap("Кино", "Кукушка");
        nothing_close.duration_ms = Some(600_000);
        assert_eq!(
            find(&index, None, &nothing_close),
            None,
            "лучше не найти, чем подменить"
        );
    }

    #[test]
    fn key_ignores_feat_yo_and_extra_artists() {
        let index = library();
        assert_eq!(
            find(&index, None, &snap("Кино", "Звезда по имени Солнце")),
            Some(4)
        );
        assert_eq!(
            find(
                &index,
                None,
                &snap("Кино, Виктор Цой", "Звезда по имени Солнце")
            ),
            Some(4)
        );
        assert_eq!(
            find(
                &index,
                None,
                &snap("Кино feat. Гость", "Звезда по имени Солнце")
            ),
            Some(4)
        );
        assert_eq!(
            find(&index, None, &snap("Аквариум", "Звезда по имени Солнце")),
            None
        );
        assert_eq!(find(&index, None, &snap("Кино", "")), None);
    }

    #[test]
    fn removed_tracks_are_not_found() {
        let mut index = library();
        let gone = track(
            2,
            "/m/best/Кукушка.mp3",
            "КИНО",
            "Кукушка",
            "Лучшее",
            396_000,
        );
        index.remove(&gone);
        assert_eq!(
            find(&index, Some("/m/best/Кукушка.mp3"), &snap("", "x")),
            None
        );
        let mut best = snap("Кино", "Кукушка");
        best.album = Some("Лучшее".into());
        assert_ne!(find(&index, None, &best), Some(2));
    }
}
