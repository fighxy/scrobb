//! Локальная библиотека оболочки: папки-источники и отдельные файлы.
//!
//! Папки запоминаются в `library.json` в каталоге данных приложения и при
//! запуске сканируются заново (`player-library::scan` + теги через lofty).
//! Базы библиотеки пока нет — когда появится (SQLite в ядре), повторное
//! сканирование будет перечитывать только изменившиеся файлы. Отдельные файлы
//! живут до закрытия приложения.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use player_domain::{MediaUri, Track, TrackId};
use player_library::scan::{self, is_inside};
use player_library::{MemoryLibrary, tags};
use serde::{Deserialize, Serialize};

use crate::adapters::LocalFiles;

/// Как часто сообщать окну о ходе сканирования.
const PROGRESS_EVERY: Duration = Duration::from_millis(200);

/// Что сейчас сканируется: окно показывает ход.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Scanning {
    /// Папка.
    pub folder: String,
    /// Сколько аудиофайлов найдено.
    pub found: usize,
    /// У скольких прочитаны теги.
    pub read: usize,
}

/// Событие библиотеки для окна, в JSON — `{ "type": …, "data": … }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum LibraryEvent {
    /// Идёт сканирование.
    Scanning(Scanning),
    /// Список треков или папок изменился — окно перечитывает снимок.
    Changed,
    /// Ошибка, которую стоит показать.
    Error(String),
}

/// Итог добавления отдельных файлов.
#[derive(Debug, Serialize)]
pub struct Added {
    pub tracks: Vec<Track>,
    pub failed: Vec<String>,
}

/// Что лежит в `library.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Config {
    folders: Vec<String>,
}

#[derive(Default)]
struct State {
    folders: Vec<String>,
    /// Треки по порядку: папки в порядке добавления, внутри — по путям.
    tracks: Vec<Track>,
    /// Путь → идентификатор: при пересканировании трек сохраняет id,
    /// и очередь ядра продолжает его узнавать.
    ids: HashMap<String, TrackId>,
    next_id: u64,
    scanning: Option<Scanning>,
}

pub struct LocalLibrary {
    /// Библиотека ядра: из неё фасад берёт трек по id.
    memory: Arc<MemoryLibrary>,
    config: PathBuf,
    state: Mutex<State>,
}

impl LocalLibrary {
    /// Открыть библиотеку с папками из `config` (файла может ещё не быть).
    pub fn open(config: PathBuf, memory: Arc<MemoryLibrary>) -> Self {
        let folders = std::fs::read(&config)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Config>(&bytes).ok())
            .map(|c| c.folders)
            .unwrap_or_default();
        Self {
            memory,
            config,
            state: Mutex::new(State {
                folders,
                ..State::default()
            }),
        }
    }

    pub fn folders(&self) -> Vec<String> {
        self.state().folders.clone()
    }

    pub fn tracks(&self) -> Vec<Track> {
        self.state().tracks.clone()
    }

    pub fn scanning(&self) -> Option<Scanning> {
        self.state().scanning.clone()
    }

    /// Добавить папку-источник; возвращает её путь в том виде, в каком она
    /// запомнена (его и сканировать). `Ok(None)` — папка уже входит в одну из
    /// добавленных. Вложенные в неё папки, добавленные раньше, заменяются ею.
    pub fn add_folder(&self, path: &str) -> Result<Option<String>, String> {
        let folder = normalize(path);
        if !Path::new(&folder).is_dir() {
            return Err(format!("Нет такой папки: {folder}"));
        }
        let mut state = self.state();
        if state.folders.iter().any(|f| is_inside(&folder, f)) {
            return Ok(None);
        }
        state.folders.retain(|f| !is_inside(f, &folder));
        state.folders.push(folder.clone());
        self.save(&state.folders)?;
        Ok(Some(folder))
    }

    /// Убрать папку и её треки из библиотеки (файлы на диске не трогаются).
    pub fn remove_folder(&self, path: &str) -> Result<(), String> {
        let folder = normalize(path);
        let mut state = self.state();
        state.folders.retain(|f| *f != folder);
        self.save(&state.folders)?;
        let memory = &self.memory;
        state.tracks.retain(|t| {
            let keep = !is_inside(&t.uri.0, &folder);
            if !keep {
                memory.remove(t.id);
            }
            keep
        });
        Ok(())
    }

    /// Пересканировать папку: добавить новые файлы, убрать исчезнувшие.
    /// Долго на большой папке — вызывается из потока сканера.
    pub fn scan(&self, folder: &str, notify: &dyn Fn(LibraryEvent)) {
        let mut progress = Progress::new(folder, notify);
        progress.report(&self.state, 0, 0, true);
        let listed = scan::audio_files(&LocalFiles, &MediaUri(folder.to_owned()), |found| {
            progress.report(&self.state, found, 0, false)
        });
        let entries = match listed {
            Ok(entries) => entries,
            Err(e) => {
                self.state().scanning = None;
                notify(LibraryEvent::Error(format!("Папка {folder}: {e:?}")));
                notify(LibraryEvent::Changed);
                return;
            }
        };
        let found = entries.len();
        let on_disk: HashSet<&str> = entries.iter().map(|e| e.uri.0.as_str()).collect();
        let known: HashSet<String> = {
            let mut state = self.state();
            // Папку убрали, пока шёл обход, — результат не нужен.
            if !state.folders.iter().any(|f| f == folder) {
                state.scanning = None;
                return;
            }
            let memory = &self.memory;
            state.tracks.retain(|t| {
                let gone = is_inside(&t.uri.0, folder) && !on_disk.contains(t.uri.0.as_str());
                if gone {
                    memory.remove(t.id);
                }
                !gone
            });
            state.tracks.iter().map(|t| t.uri.0.clone()).collect()
        };
        // Теги читаются без блокировки: окно в это время получает снимки.
        let mut read = 0;
        for entry in entries.iter().filter(|e| !known.contains(&e.uri.0)) {
            let id = self.id_for(&entry.uri.0);
            if let Ok(track) = tags::read_track(&LocalFiles, &entry.uri, id) {
                self.memory.insert(track.clone());
                self.state().tracks.push(track);
            }
            read += 1;
            progress.report(&self.state, found, read, false);
        }
        self.state().scanning = None;
        notify(LibraryEvent::Changed);
    }

    /// Добавить отдельные файлы; уже известные пути пропускаются.
    pub fn add_files(&self, paths: Vec<String>) -> Added {
        let mut known: HashSet<String> = self
            .state()
            .tracks
            .iter()
            .map(|t| t.uri.0.clone())
            .collect();
        let mut tracks = Vec::new();
        let mut failed = Vec::new();
        for path in paths {
            if !known.insert(path.clone()) {
                continue;
            }
            let id = self.id_for(&path);
            match tags::read_track(&LocalFiles, &MediaUri(path.clone()), id) {
                Ok(track) => {
                    self.memory.insert(track.clone());
                    self.state().tracks.push(track.clone());
                    tracks.push(track);
                }
                Err(_) => failed.push(file_name(&path)),
            }
        }
        Added { tracks, failed }
    }

    fn id_for(&self, path: &str) -> TrackId {
        let mut state = self.state();
        if let Some(id) = state.ids.get(path) {
            return *id;
        }
        state.next_id += 1;
        let id = TrackId(state.next_id);
        state.ids.insert(path.to_owned(), id);
        id
    }

    /// Записать список папок: сначала во временный файл, потом заменить —
    /// сбой посреди записи не оставит испорченный `library.json`.
    fn save(&self, folders: &[String]) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(&Config {
            folders: folders.to_vec(),
        })
        .map_err(|e| e.to_string())?;
        let tmp = self.config.with_extension("json.tmp");
        std::fs::write(&tmp, json)
            .and_then(|()| std::fs::rename(&tmp, &self.config))
            .map_err(|e| format!("Не удалось сохранить список папок: {e}"))
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }
}

/// Ход сканирования: в состоянии библиотеки (для снимка окна) и событием,
/// но не чаще [`PROGRESS_EVERY`].
struct Progress<'a> {
    folder: &'a str,
    notify: &'a dyn Fn(LibraryEvent),
    last: Instant,
}

impl<'a> Progress<'a> {
    fn new(folder: &'a str, notify: &'a dyn Fn(LibraryEvent)) -> Self {
        Self {
            folder,
            notify,
            last: Instant::now(),
        }
    }

    fn report(&mut self, state: &Mutex<State>, found: usize, read: usize, force: bool) {
        let scanning = Scanning {
            folder: self.folder.to_owned(),
            found,
            read,
        };
        lock(state).scanning = Some(scanning.clone());
        if force || self.last.elapsed() >= PROGRESS_EVERY {
            self.last = Instant::now();
            (self.notify)(LibraryEvent::Scanning(scanning));
        }
    }
}

/// Путь папки без завершающего разделителя (кроме корня `/`).
fn normalize(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        path.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use player_library::TrackStore;
    use std::fs;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(format!(
            "{}/../../../core/crates/player-testkit/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
    }

    /// Временный каталог: в нём папка с музыкой и `library.json`.
    struct Sandbox(PathBuf);

    impl Sandbox {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("scrobb-lib-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("music")).unwrap();
            Self(dir)
        }

        fn put(&self, fixture_name: &str, to: &str) -> String {
            let path = self.0.join("music").join(to);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::copy(fixture(fixture_name), &path).unwrap();
            path.to_string_lossy().into_owned()
        }

        fn music(&self) -> String {
            self.0.join("music").to_string_lossy().into_owned()
        }

        fn library(&self) -> (LocalLibrary, Arc<MemoryLibrary>) {
            let memory = Arc::new(MemoryLibrary::new());
            (
                LocalLibrary::open(self.0.join("library.json"), memory.clone()),
                memory,
            )
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn titles(library: &LocalLibrary) -> Vec<String> {
        library.tracks().into_iter().map(|t| t.title).collect()
    }

    #[test]
    fn folder_is_scanned_remembered_and_rescanned() {
        let sandbox = Sandbox::new("scan");
        sandbox.put("tagged.flac", "Кино/Чёрный альбом/04.flac");
        let deleted = sandbox.put("a.flac", "Кино/Чёрный альбом/05.flac");
        fs::write(sandbox.0.join("music/Кино/cover.jpg"), b"jpg").unwrap();

        let (library, memory) = sandbox.library();
        assert_eq!(
            library.add_folder(&format!("{}/", sandbox.music())),
            Ok(Some(sandbox.music()))
        );
        let events = Mutex::new(Vec::new());
        library.scan(&sandbox.music(), &|e| lock(&events).push(e));
        assert_eq!(titles(&library), ["Кукушка", "05"]);
        assert!(library.scanning().is_none());
        let events = lock(&events).clone();
        assert!(
            matches!(&events[0], LibraryEvent::Scanning(s) if s.found == 0 && s.folder == sandbox.music())
        );
        assert_eq!(events.last(), Some(&LibraryEvent::Changed));
        let kino = library.tracks()[0].clone();
        assert_eq!(memory.track(kino.id).unwrap().artists, ["Кино"]);

        // Новый запуск: папка запомнилась; на диске один файл исчез, один появился.
        fs::remove_file(&deleted).unwrap();
        sandbox.put("full.flac", "Кино/Чёрный альбом/06.flac");
        let (library, memory) = sandbox.library();
        assert_eq!(library.folders(), [sandbox.music()]);
        library.scan(&sandbox.music(), &|_| {});
        assert_eq!(titles(&library), ["Кукушка", "06"]);

        // Пересканирование в той же сессии: исчезнувший файл уходит и из
        // библиотеки ядра, известные треки сохраняют id.
        let (kino, gone) = (library.tracks()[0].clone(), library.tracks()[1].clone());
        fs::remove_file(&gone.uri.0).unwrap();
        library.scan(&sandbox.music(), &|_| {});
        assert_eq!(titles(&library), ["Кукушка"]);
        assert_eq!(library.tracks()[0].id, kino.id);
        assert!(memory.track(gone.id).is_none());
    }

    #[test]
    fn track_that_comes_back_keeps_its_id() {
        let sandbox = Sandbox::new("ids");
        sandbox.put("tagged.flac", "04.flac");
        let (library, memory) = sandbox.library();
        library.add_folder(&sandbox.music()).unwrap();
        library.scan(&sandbox.music(), &|_| {});
        let id = library.tracks()[0].id;
        // Папку убрали и вернули: очередь ядра снова узнаёт трек по прежнему id.
        library.remove_folder(&sandbox.music()).unwrap();
        library.add_folder(&sandbox.music()).unwrap();
        library.scan(&sandbox.music(), &|_| {});
        assert_eq!(library.tracks()[0].id, id);
        assert_eq!(memory.track(id).unwrap().title, "Кукушка");
    }

    #[test]
    fn nested_and_parent_folders_merge_and_removal_drops_tracks() {
        let sandbox = Sandbox::new("folders");
        sandbox.put("tagged.flac", "Кино/04.flac");
        let (library, memory) = sandbox.library();
        let kino = format!("{}/Кино", sandbox.music());

        assert_eq!(library.add_folder(&kino), Ok(Some(kino.clone())));
        // Родительская папка заменяет вложенную, повтор — ничего не меняет.
        assert_eq!(
            library.add_folder(&sandbox.music()),
            Ok(Some(sandbox.music()))
        );
        assert_eq!(library.folders(), [sandbox.music()]);
        assert_eq!(library.add_folder(&kino), Ok(None));
        assert!(library.add_folder("/nope/nope").is_err());

        library.scan(&sandbox.music(), &|_| {});
        let id = library.tracks()[0].id;
        library.remove_folder(&sandbox.music()).unwrap();
        assert!(library.tracks().is_empty() && library.folders().is_empty());
        assert!(memory.track(id).is_none());
        let (reopened, _) = sandbox.library();
        assert!(reopened.folders().is_empty());
    }

    #[test]
    fn missing_folder_reports_error_and_files_are_deduplicated() {
        let sandbox = Sandbox::new("files");
        let (library, _) = sandbox.library();
        let events = Mutex::new(Vec::new());
        library.scan("/nope/nope", &|e| lock(&events).push(e));
        assert!(
            lock(&events)
                .iter()
                .any(|e| matches!(e, LibraryEvent::Error(_)))
        );
        assert!(library.scanning().is_none());

        let tagged = fixture("tagged.flac").to_string_lossy().into_owned();
        let missing = fixture("nope.flac").to_string_lossy().into_owned();
        let added = library.add_files(vec![tagged.clone(), tagged.clone(), missing]);
        assert_eq!(added.tracks.len(), 1);
        assert_eq!(added.failed, ["nope.flac"]);
        assert!(library.add_files(vec![tagged]).tracks.is_empty());
    }
}
