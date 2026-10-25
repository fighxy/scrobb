//! Плейлисты в фасаде: правки, поиск треков в библиотеке, импорт и экспорт.
//!
//! Хранилище (`player-playlists::PlaylistStore`) только хранит; здесь
//! задаются id (UUIDv7), время и ключи порядка, а элементы связываются
//! с библиотекой этого устройства (`TrackStore::resolve`). Трек, которого
//! в библиотеке нет, остаётся в плейлисте с `track_id: null`.

use std::collections::HashSet;
use std::sync::Arc;

use player_domain::{TrackId, TrackSnapshot};
use player_library::TrackQuery;
use player_playlists::formats::{self, Exported, Format};
use player_playlists::{
    ItemId, Playlist, PlaylistContents, PlaylistId, PlaylistItem, PlaylistStore, order, paths,
};
use serde::Serialize;
use uuid::{Timestamp, Uuid};

use crate::{Core, CoreError, Inner, lock};

/// Сколько обложек в мозаике плейлиста.
const COVERS: usize = 4;

/// Плейлист в списке.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlaylistCard {
    /// Плейлист.
    pub playlist: Playlist,
    /// Сколько элементов.
    pub items: usize,
    /// Общая длительность известных.
    pub duration_ms: u64,
    /// Треки библиотеки для мозаики обложки: до четырёх разных альбомов.
    pub covers: Vec<TrackId>,
}

/// Элемент плейлиста и трек библиотеки, если он нашёлся.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedItem {
    /// Элемент.
    #[serde(flatten)]
    pub item: PlaylistItem,
    /// Трек библиотеки; `None` — на этом устройстве такого нет.
    pub track_id: Option<TrackId>,
}

/// Плейлист с элементами.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlaylistView {
    /// Плейлист.
    pub playlist: Playlist,
    /// Элементы по порядку.
    pub items: Vec<ResolvedItem>,
}

/// Итог импорта.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    /// Новый плейлист.
    pub playlist: Playlist,
    /// Сколько записей было в файле.
    pub total: usize,
    /// Сколько из них нашлось в библиотеке.
    pub matched: usize,
}

fn store(inner: &Inner) -> Result<&Arc<dyn PlaylistStore>, CoreError> {
    inner
        .playlists
        .as_ref()
        .ok_or(CoreError::MissingPort("playlists"))
}

fn failed(e: player_playlists::PlaylistError) -> CoreError {
    CoreError::Playlist(e.0)
}

impl Inner {
    fn new_id(&self, unix_ms: i64) -> String {
        let ids = lock(&self.playlist_ids);
        let ms = unix_ms.max(0) as u64;
        let ts = Timestamp::from_unix(&*ids, ms / 1000, (ms % 1000) as u32 * 1_000_000);
        Uuid::new_v7(ts).to_string()
    }

    fn contents(&self, id: &PlaylistId) -> Result<PlaylistContents, CoreError> {
        store(self)?
            .get(id)
            .map_err(failed)?
            .ok_or_else(|| CoreError::Playlist(format!("нет плейлиста {}", id.0)))
    }

    fn resolve(&self, location: Option<&str>, track: &TrackSnapshot) -> Option<TrackId> {
        self.library.resolve(&TrackQuery { location, track })
    }

    fn create(
        &self,
        title: &str,
        description: Option<&str>,
        origin: Option<String>,
    ) -> Result<Playlist, CoreError> {
        let now = self.clock.unix_ms();
        let playlist = Playlist {
            id: PlaylistId(self.new_id(now)),
            title: title.trim().to_owned(),
            description: description
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_owned),
            origin,
            created_at_ms: now,
            updated_at_ms: now,
        };
        if playlist.title.is_empty() {
            return Err(CoreError::Playlist("у плейлиста нет названия".into()));
        }
        store(self)?.create(&playlist).map_err(failed)?;
        Ok(playlist)
    }
}

impl Core {
    /// Все плейлисты, недавно изменённые первыми, с мозаикой обложек.
    pub fn playlists(&self) -> Result<Vec<PlaylistCard>, CoreError> {
        let inner = self.inner();
        let summaries = store(inner)?.list().map_err(failed)?;
        let mut cards = Vec::with_capacity(summaries.len());
        for summary in summaries {
            let contents = inner.contents(&summary.playlist.id)?;
            let mut albums = HashSet::new();
            let mut covers = Vec::new();
            for item in &contents.items {
                if covers.len() == COVERS {
                    break;
                }
                let Some(id) = inner.resolve(item.location.as_deref(), &item.track) else {
                    continue;
                };
                let album = inner.library.track(id).map(|t| {
                    (
                        t.album_artist.or_else(|| t.artists.first().cloned()),
                        t.album,
                    )
                });
                // Без альбома каждый трек — своя обложка.
                if album.as_ref().is_some_and(|(_, a)| a.is_none()) || albums.insert(album) {
                    covers.push(id);
                }
            }
            cards.push(PlaylistCard {
                playlist: summary.playlist,
                items: summary.items,
                duration_ms: summary.duration_ms,
                covers,
            });
        }
        Ok(cards)
    }

    /// Плейлист с элементами и найденными в библиотеке треками.
    pub fn playlist(&self, id: &PlaylistId) -> Result<PlaylistView, CoreError> {
        let inner = self.inner();
        let contents = inner.contents(id)?;
        let items = contents
            .items
            .into_iter()
            .map(|item| ResolvedItem {
                track_id: inner.resolve(item.location.as_deref(), &item.track),
                item,
            })
            .collect();
        Ok(PlaylistView {
            playlist: contents.playlist,
            items,
        })
    }

    /// Новый пустой плейлист.
    pub fn create_playlist(
        &self,
        title: &str,
        description: Option<&str>,
    ) -> Result<Playlist, CoreError> {
        self.inner().create(title, description, None)
    }

    /// Переименовать и сменить описание.
    pub fn update_playlist(
        &self,
        id: &PlaylistId,
        title: &str,
        description: Option<&str>,
    ) -> Result<(), CoreError> {
        let inner = self.inner();
        let title = title.trim();
        if title.is_empty() {
            return Err(CoreError::Playlist("у плейлиста нет названия".into()));
        }
        let description = description.map(str::trim).filter(|d| !d.is_empty());
        store(inner)?
            .update(id, title, description, inner.clock.unix_ms())
            .map_err(failed)
    }

    /// Удалить плейлист.
    pub fn delete_playlist(&self, id: &PlaylistId) -> Result<(), CoreError> {
        let inner = self.inner();
        store(inner)?
            .delete(id, inner.clock.unix_ms())
            .map_err(failed)
    }

    /// Добавить треки библиотеки на место `at` (`None` — в конец). Неизвестные
    /// id пропускаются; возвращает, сколько добавлено.
    pub fn add_to_playlist(
        &self,
        id: &PlaylistId,
        tracks: &[TrackId],
        at: Option<usize>,
    ) -> Result<usize, CoreError> {
        let inner = self.inner();
        let contents = inner.contents(id)?;
        let found: Vec<_> = tracks
            .iter()
            .filter_map(|t| inner.library.track(*t))
            .collect();
        let at = at.unwrap_or(contents.items.len()).min(contents.items.len());
        let before = at
            .checked_sub(1)
            .map(|i| contents.items[i].position.as_str());
        let after = contents.items.get(at).map(|i| i.position.as_str());
        let now = inner.clock.unix_ms();
        let items: Vec<PlaylistItem> = found
            .iter()
            .zip(order::spread(before, after, found.len()))
            .map(|(track, position)| PlaylistItem {
                id: ItemId(inner.new_id(now)),
                position,
                added_at_ms: now,
                location: Some(track.uri.0.clone()),
                track: TrackSnapshot::from(track),
            })
            .collect();
        if !items.is_empty() {
            store(inner)?.add_items(id, &items, now).map_err(failed)?;
        }
        Ok(items.len())
    }

    /// Убрать элементы.
    pub fn remove_from_playlist(&self, id: &PlaylistId, items: &[ItemId]) -> Result<(), CoreError> {
        let inner = self.inner();
        store(inner)?
            .remove_items(id, items, inner.clock.unix_ms())
            .map_err(failed)
    }

    /// Переставить элемент так, чтобы он стал `to`-м по счёту.
    pub fn move_in_playlist(
        &self,
        id: &PlaylistId,
        item: &ItemId,
        to: usize,
    ) -> Result<(), CoreError> {
        let inner = self.inner();
        let contents = inner.contents(id)?;
        let from = contents
            .items
            .iter()
            .position(|i| i.id == *item)
            .ok_or_else(|| CoreError::Playlist(format!("нет элемента {}", item.0)))?;
        let others: Vec<&PlaylistItem> = contents.items.iter().filter(|i| i.id != *item).collect();
        let to = to.min(others.len());
        if to == from {
            return Ok(());
        }
        let before = to.checked_sub(1).map(|i| others[i].position.as_str());
        let after = others.get(to).map(|i| i.position.as_str());
        // Равные ключи соседей (правки с двух устройств) — место между ними
        // не найти: тогда ключи всего плейлиста раздаются заново.
        if let (Some(b), Some(a)) = (before, after)
            && b >= a
        {
            return self.renumber(id, &others, item, to);
        }
        let position = order::between(before, after);
        store(inner)?
            .set_position(id, item, &position, inner.clock.unix_ms())
            .map_err(failed)
    }

    fn renumber(
        &self,
        id: &PlaylistId,
        others: &[&PlaylistItem],
        item: &ItemId,
        to: usize,
    ) -> Result<(), CoreError> {
        let inner = self.inner();
        let store = store(inner)?;
        let mut ids: Vec<&ItemId> = others.iter().map(|i| &i.id).collect();
        ids.insert(to, item);
        let now = inner.clock.unix_ms();
        for (id_, key) in ids
            .into_iter()
            .zip(order::spread(None, None, others.len() + 1))
        {
            store.set_position(id, id_, &key, now).map_err(failed)?;
        }
        Ok(())
    }

    /// Плейлист из файла. `file_name` — для формата и названия по умолчанию,
    /// `base_dir` — папка файла: от неё считаются относительные пути.
    pub fn import_playlist(
        &self,
        file_name: &str,
        bytes: &[u8],
        base_dir: Option<&str>,
    ) -> Result<ImportReport, CoreError> {
        let inner = self.inner();
        let format = formats::detect(file_name, bytes);
        let imported = formats::parse(format, bytes, base_dir).map_err(failed)?;
        let title = imported
            .title
            .clone()
            .unwrap_or_else(|| paths::file_stem(file_name).to_owned());
        let origin = format!("import:{}", serde_name(format));
        let playlist = inner.create(&title, imported.description.as_deref(), Some(origin))?;
        let now = inner.clock.unix_ms();
        let mut matched = 0;
        let positions = order::spread(None, None, imported.entries.len());
        let items: Vec<PlaylistItem> = imported
            .entries
            .into_iter()
            .zip(positions)
            .map(|(entry, position)| {
                let found = inner.resolve(entry.location.as_deref(), &entry.track);
                matched += usize::from(found.is_some());
                // Найденный трек дополняет снимок тем, чего не было в файле.
                let (location, track) = match found.and_then(|id| inner.library.track(id)) {
                    Some(t) => (
                        entry.location.or_else(|| Some(t.uri.0.clone())),
                        merge(entry.track, TrackSnapshot::from(&t)),
                    ),
                    None => (entry.location, entry.track),
                };
                PlaylistItem {
                    id: ItemId(inner.new_id(now)),
                    position,
                    added_at_ms: now,
                    location,
                    track,
                }
            })
            .collect();
        let total = items.len();
        if !items.is_empty() {
            store(inner)?
                .add_items(&playlist.id, &items, now)
                .map_err(failed)?;
        }
        Ok(ImportReport {
            playlist,
            total,
            matched,
        })
    }

    /// Плейлист в файл формата `format`. `target_dir` — папка, куда ляжет
    /// файл: пути M3U8 будут относительно неё, если музыка рядом. Элементам
    /// без пути, но найденным в библиотеке, путь берётся из библиотеки.
    pub fn export_playlist(
        &self,
        id: &PlaylistId,
        format: Format,
        target_dir: Option<&str>,
    ) -> Result<Exported, CoreError> {
        let inner = self.inner();
        let contents = inner.contents(id)?;
        let items: Vec<PlaylistItem> = contents
            .items
            .into_iter()
            .map(|mut item| {
                if let Some(t) = inner
                    .resolve(item.location.as_deref(), &item.track)
                    .and_then(|id| inner.library.track(id))
                {
                    item.location = Some(t.uri.0);
                }
                item
            })
            .collect();
        formats::write(format, &contents.playlist, &items, target_dir).map_err(failed)
    }
}

/// Снимок из файла, дополненный данными найденного трека: из файла — то,
/// что в нём было (название как у пользователя), из библиотеки — пробелы.
fn merge(mut from_file: TrackSnapshot, library: TrackSnapshot) -> TrackSnapshot {
    if from_file.artists.is_empty() {
        from_file.artists = library.artists;
    }
    from_file.album = from_file.album.or(library.album);
    from_file.album_artist = from_file.album_artist.or(library.album_artist);
    from_file.duration_ms = from_file.duration_ms.or(library.duration_ms);
    from_file.recording_mbid = from_file.recording_mbid.or(library.recording_mbid);
    for (key, value) in library.ext_ids {
        from_file.ext_ids.entry(key).or_insert(value);
    }
    from_file
}

fn serde_name(format: Format) -> &'static str {
    match format {
        Format::M3u => "m3u",
        Format::Pls => "pls",
        Format::Xspf => "xspf",
        Format::Jspf => "jspf",
        Format::Csv => "csv",
        Format::Txt => "txt",
    }
}
