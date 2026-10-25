//! Мост «ядро ↔ окно». Команды Tauri вызывают фасад `player-core`, события
//! ядра уходят в окно как [`EVENT`], события библиотеки — как [`LIBRARY_EVENT`].
//! Формат JSON закреплён тестом ядра `player-core/tests/json.rs`; TS-типы
//! окна — `src/core.ts`.

use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use player_core::{
    Command, Core, CoreConfig, CoreError, Event, ImportReport, Observer, PlaylistCard,
    PlaylistView, Ports, ScrobbSettings, ScrobbleStatus, Stores,
};
use player_domain::{PlayerState, Track, TrackId};
use player_history::PlayRecord;
use player_library::MemoryLibrary;
use player_output_cpal::CpalOutput;
use player_playlists::formats::{Exported, Format};
use player_playlists::{ItemId, Playlist, PlaylistId};
use player_store_sqlite::{SqliteHistory, SqlitePlaylists};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::adapters::{KeyringSecrets, LocalFiles, SystemClock, UreqHttp};
use crate::covers::Covers;
use crate::library::{Added, LibraryEvent, LocalLibrary, Scanning};

/// Событие окна с [`Event`] ядра.
pub const EVENT: &str = "core://event";
/// Событие окна с [`LibraryEvent`].
pub const LIBRARY_EVENT: &str = "library://event";
/// Сервер Scrobb по умолчанию; для разработки — переменная `SCROBB_BASE_URL`.
const DEFAULT_SCROBB_URL: &str = "https://scrobb.ru";
/// Файлы, которые «Открыть с помощью» превращает в плейлист, а не в треки.
/// CSV и TXT — только через «Импорт»: по расширению их не отличить от таблиц и заметок.
const PLAYLIST_EXTENSIONS: [&str; 5] = ["m3u", "m3u8", "pls", "xspf", "jspf"];

/// Состояние оболочки: ядро и то, что окну нужно сверх него.
pub struct Player {
    /// `Option` — чтобы при выходе удалить ядро явно: оно закроет открытое
    /// прослушивание и снимет «сейчас играет».
    core: Mutex<Option<Core>>,
    library: Arc<LocalLibrary>,
    /// Очередь папок для потока сканера: сканирование не блокирует окно.
    scanner: mpsc::Sender<String>,
    forwarder: Arc<Forwarder>,
    covers: Arc<Covers>,
    scrobbling_available: bool,
    /// Плейлист из аргументов запуска: окно откроет его сразу.
    opened: Mutex<Option<PlaylistId>>,
}

impl Player {
    /// Собрать ядро: адаптеры десктопа, история в каталоге данных приложения.
    pub fn start(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let data_dir = app.path().app_data_dir()?;
        std::fs::create_dir_all(&data_dir)?;
        let history = Arc::new(SqliteHistory::open(data_dir.join("history.db"))?);
        // Правки пользователя (плейлисты) — отдельно от пересобираемой библиотеки.
        let playlists = Arc::new(SqlitePlaylists::open(data_dir.join("user.db"))?);
        let memory = Arc::new(MemoryLibrary::new());
        let library = Arc::new(LocalLibrary::open(
            data_dir.join("library.json"),
            memory.clone(),
        ));
        let scanner = spawn_scanner(library.clone(), app.clone());
        let covers = Covers::new(memory.clone(), Arc::new(LocalFiles));
        // Базы библиотеки пока нет: запомненные папки сканируются при каждом запуске.
        for folder in library.folders() {
            let _ = scanner.send(folder);
        }
        let forwarder = Arc::new(Forwarder {
            app: app.clone(),
            scrobbling: Mutex::new(None),
        });
        let ports = Ports {
            clock: Arc::new(SystemClock::new()),
            audio: Box::new(CpalOutput::new()),
            files: Arc::new(LocalFiles),
            http: Some(Arc::new(UreqHttp::new())),
            secrets: Some(Arc::new(KeyringSecrets)),
            media_session: None,
        };
        let config = CoreConfig {
            scrobb: Some(ScrobbSettings {
                base_url: std::env::var("SCROBB_BASE_URL")
                    .unwrap_or_else(|_| DEFAULT_SCROBB_URL.into()),
                version: env!("CARGO_PKG_VERSION").into(),
                platform: std::env::consts::OS.into(),
            }),
            ..CoreConfig::default()
        };
        let stores = Stores {
            library: memory,
            history: Some(history),
            playlists: Some(playlists),
        };
        let core = Core::new(ports, stores, config, forwarder.clone());
        let scrobbling_available = core.capabilities().scrobbling;
        Ok(Self {
            core: Mutex::new(Some(core)),
            library,
            scanner,
            forwarder,
            covers,
            scrobbling_available,
            opened: Mutex::new(None),
        })
    }

    /// Аргументы командной строки («Открыть с помощью» в файловом менеджере):
    /// папки становятся источниками, плейлисты импортируются (последний окно
    /// откроет), остальные файлы добавляются и играют с первого.
    pub fn open_paths(&self, paths: Vec<String>) {
        let (folders, files): (Vec<_>, Vec<_>) =
            paths.into_iter().partition(|p| Path::new(p).is_dir());
        for folder in folders {
            if let Err(e) = self.add_folder(&folder) {
                self.forwarder.emit_library(LibraryEvent::Error(e));
            }
        }
        let (lists, files): (Vec<_>, Vec<_>) = files.into_iter().partition(|p| is_playlist_file(p));
        for list in lists {
            match self.import_file(&list) {
                Ok(report) => {
                    *lock(&self.opened) = Some(report.playlist.id);
                    self.forwarder.emit_library(LibraryEvent::Playlists);
                }
                Err(e) => self
                    .forwarder
                    .emit_library(LibraryEvent::Error(describe(&e))),
            }
        }
        let first_file = files.first().cloned();
        self.library.add_files(files);
        // Первый файл мог уже найти сканер папки — тогда он не «добавлен», но играть его надо.
        if let Some(first) = first_file.and_then(|path| self.library.track_by_path(&path)) {
            let all: Vec<_> = self.library.tracks().iter().map(|t| t.id).collect();
            let start = all.iter().position(|id| *id == first.id).unwrap_or(0);
            let play = Command::PlayQueue { tracks: all, start };
            if let Err(e) = self.with_core(|core| core.dispatch(play)) {
                let _ = self
                    .forwarder
                    .app
                    .emit(EVENT, Event::Error(format!("{e:?}")));
            }
        }
    }

    fn add_folder(&self, path: &str) -> Result<bool, String> {
        let added = self.library.add_folder(path)?;
        if let Some(folder) = &added {
            let _ = self.scanner.send(folder.clone());
        }
        Ok(added.is_some())
    }

    /// Плейлист из файла: формат по расширению и содержимому, относительные
    /// пути — от папки файла.
    fn import_file(&self, path: &str) -> Result<ImportReport, CoreError> {
        let bytes = std::fs::read(path).map_err(|e| CoreError::Playlist(format!("{path}: {e}")))?;
        let file = Path::new(path);
        let name = file
            .file_name()
            .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
        let dir = file.parent().map(|d| d.to_string_lossy().into_owned());
        self.with_core(|core| core.import_playlist(&name, &bytes, dir.as_deref()))
    }

    /// Обложки для схемы `cover://`.
    pub fn covers(&self) -> Arc<Covers> {
        self.covers.clone()
    }

    /// Остановить ядро перед выходом.
    pub fn shutdown(&self) {
        drop(lock(&self.core).take());
    }

    fn with_core<T>(&self, f: impl FnOnce(&Core) -> Result<T, CoreError>) -> Result<T, CoreError> {
        match lock(&self.core).as_ref() {
            Some(core) => f(core),
            None => Err(CoreError::NotConfigured("ядро остановлено")),
        }
    }
}

/// Наблюдатель ядра: события → окно; код привязки → системный браузер.
pub struct Forwarder {
    app: AppHandle,
    /// Последнее состояние отправки: окно может открыться позже первого события.
    scrobbling: Mutex<Option<ScrobbleStatus>>,
}

impl Forwarder {
    fn emit_library(&self, event: LibraryEvent) {
        let _ = self.app.emit(LIBRARY_EVENT, &event);
    }
}

impl Observer for Forwarder {
    fn on_event(&self, event: Event) {
        match &event {
            Event::ScrobbLinkCode {
                verification_uri_complete,
                ..
            } => {
                // Страница привязки открывается сразу (§18.4, шаг 2); код окно показывает само.
                if let Err(e) = self
                    .app
                    .opener()
                    .open_url(verification_uri_complete, None::<&str>)
                {
                    let message = format!("Не удалось открыть браузер: {e}");
                    let _ = self.app.emit(EVENT, Event::Error(message));
                }
            }
            Event::Scrobbling(status) => *lock(&self.scrobbling) = Some(*status),
            _ => {}
        }
        let _ = self.app.emit(EVENT, &event);
    }
}

/// Всё, что окну нужно при открытии.
#[derive(Debug, Serialize)]
pub struct Snapshot {
    state: PlayerState,
    tracks: Vec<Track>,
    folders: Vec<String>,
    scanning: Option<Scanning>,
    scrobbling: Option<ScrobbleStatus>,
    scrobbling_available: bool,
    /// Плейлист, с которым запустили приложение.
    opened_playlist: Option<PlaylistId>,
}

#[tauri::command(rename_all = "snake_case")]
pub fn dispatch(player: State<'_, Player>, command: Command) -> Result<(), CoreError> {
    player.with_core(|core| core.dispatch(command))
}

#[tauri::command(rename_all = "snake_case")]
pub fn snapshot(player: State<'_, Player>) -> Result<Snapshot, CoreError> {
    let state = player.with_core(|core| Ok(core.state()))?;
    Ok(Snapshot {
        state,
        tracks: player.library.tracks(),
        folders: player.library.folders(),
        scanning: player.library.scanning(),
        scrobbling: *lock(&player.forwarder.scrobbling),
        scrobbling_available: player.scrobbling_available,
        opened_playlist: lock(&player.opened).clone(),
    })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn add_files(player: State<'_, Player>, paths: Vec<String>) -> Result<Added, String> {
    Ok(player.library.add_files(paths))
}

/// Добавить папку-источник и просканировать её в фоне. `false` — папка уже
/// входит в одну из добавленных.
#[tauri::command(rename_all = "snake_case")]
pub fn add_folder(player: State<'_, Player>, path: String) -> Result<bool, String> {
    player.add_folder(&path)
}

/// Убрать папку и её треки из библиотеки; файлы на диске не трогаются.
#[tauri::command(rename_all = "snake_case")]
pub fn remove_folder(player: State<'_, Player>, path: String) -> Result<(), String> {
    player.library.remove_folder(&path)?;
    player.forwarder.emit_library(LibraryEvent::Changed);
    Ok(())
}

/// Пересканировать все папки: новые файлы добавятся, удалённые исчезнут.
#[tauri::command(rename_all = "snake_case")]
pub fn rescan_library(player: State<'_, Player>) {
    player.covers.clear();
    for folder in player.library.folders() {
        let _ = player.scanner.send(folder);
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn recent_plays(
    player: State<'_, Player>,
    before_ms: Option<i64>,
    limit: usize,
) -> Result<Vec<PlayRecord>, CoreError> {
    player.with_core(|core| core.recent_plays(before_ms, limit))
}

/// Треки очереди по порядку — для «Далее».
#[tauri::command(rename_all = "snake_case")]
pub fn queue(player: State<'_, Player>) -> Result<Vec<TrackId>, CoreError> {
    player.with_core(|core| Ok(core.queue()))
}

/// Цвет обложки трека для фона (`#rrggbb`) или `null`, если обложки нет.
/// Картинка раскодируется в отдельном потоке, а не в потоке окна.
#[tauri::command(rename_all = "snake_case")]
pub async fn cover_color(player: State<'_, Player>, id: TrackId) -> Result<Option<String>, ()> {
    let covers = player.covers();
    Ok(
        tauri::async_runtime::spawn_blocking(move || covers.color(id))
            .await
            .ok()
            .flatten(),
    )
}

/// Плейлисты: недавно изменённые первыми, с мозаикой обложек.
#[tauri::command(rename_all = "snake_case")]
pub async fn playlists(player: State<'_, Player>) -> Result<Vec<PlaylistCard>, CoreError> {
    player.with_core(|core| core.playlists())
}

/// Плейлист с элементами; `track_id` — найденный в библиотеке трек.
#[tauri::command(rename_all = "snake_case")]
pub async fn playlist(
    player: State<'_, Player>,
    id: PlaylistId,
) -> Result<PlaylistView, CoreError> {
    player.with_core(|core| core.playlist(&id))
}

/// Новый плейлист, сразу с треками (`tracks` может быть пустым).
#[tauri::command(rename_all = "snake_case")]
pub async fn create_playlist(
    player: State<'_, Player>,
    title: String,
    tracks: Vec<TrackId>,
) -> Result<Playlist, CoreError> {
    player.with_core(|core| {
        let playlist = core.create_playlist(&title, None)?;
        if !tracks.is_empty() {
            core.add_to_playlist(&playlist.id, &tracks, None)?;
        }
        Ok(playlist)
    })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn update_playlist(
    player: State<'_, Player>,
    id: PlaylistId,
    title: String,
    description: Option<String>,
) -> Result<(), CoreError> {
    player.with_core(|core| core.update_playlist(&id, &title, description.as_deref()))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_playlist(player: State<'_, Player>, id: PlaylistId) -> Result<(), CoreError> {
    player.with_core(|core| core.delete_playlist(&id))
}

/// Добавить треки на место `at` (`null` — в конец); ответ — сколько добавлено.
#[tauri::command(rename_all = "snake_case")]
pub async fn add_to_playlist(
    player: State<'_, Player>,
    id: PlaylistId,
    tracks: Vec<TrackId>,
    at: Option<usize>,
) -> Result<usize, CoreError> {
    player.with_core(|core| core.add_to_playlist(&id, &tracks, at))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn remove_from_playlist(
    player: State<'_, Player>,
    id: PlaylistId,
    items: Vec<ItemId>,
) -> Result<(), CoreError> {
    player.with_core(|core| core.remove_from_playlist(&id, &items))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn move_in_playlist(
    player: State<'_, Player>,
    id: PlaylistId,
    item: ItemId,
    to: usize,
) -> Result<(), CoreError> {
    player.with_core(|core| core.move_in_playlist(&id, &item, to))
}

/// Импорт файла плейлиста; файл читает Rust, у окна доступа к диску нет.
#[tauri::command(rename_all = "snake_case")]
pub async fn import_playlist(
    player: State<'_, Player>,
    path: String,
) -> Result<ImportReport, CoreError> {
    player.import_file(&path)
}

/// Экспорт в файл `path`; пути M3U8 — относительно его папки, если музыка рядом.
#[tauri::command(rename_all = "snake_case")]
pub async fn export_playlist(
    player: State<'_, Player>,
    id: PlaylistId,
    format: Format,
    path: String,
) -> Result<Exported, CoreError> {
    let dir = Path::new(&path)
        .parent()
        .map(|d| d.to_string_lossy().into_owned());
    let out = player.with_core(|core| core.export_playlist(&id, format, dir.as_deref()))?;
    std::fs::write(&path, &out.text).map_err(|e| CoreError::Playlist(format!("{path}: {e}")))?;
    Ok(out)
}

fn is_playlist_file(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PLAYLIST_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Текст ошибки ядра для окна.
fn describe(e: &CoreError) -> String {
    match e {
        CoreError::Playlist(message) | CoreError::History(message) => message.clone(),
        other => format!("{other:?}"),
    }
}

/// Поток сканера: папки по одной, в порядке очереди. Папку, которую успели
/// убрать, пока она ждала, не сканирует.
fn spawn_scanner(library: Arc<LocalLibrary>, app: AppHandle) -> mpsc::Sender<String> {
    let (tx, rx) = mpsc::channel::<String>();
    thread::Builder::new()
        .name("library-scanner".into())
        .spawn(move || {
            let notify = |event: LibraryEvent| {
                let _ = app.emit(LIBRARY_EVENT, &event);
            };
            for folder in rx {
                if library.folders().contains(&folder) {
                    library.scan(&folder, &notify);
                }
            }
        })
        .expect("поток сканера запускается");
    tx
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_files_are_imported_audio_files_are_played() {
        for list in [
            "/m/Вечер.m3u8",
            "C:\\m\\a.M3U",
            "/m/radio.pls",
            "/m/vlc.xspf",
            "/m/lb.jspf",
        ] {
            assert!(is_playlist_file(list), "{list}");
        }
        for other in [
            "/m/a.flac",
            "/m/list.txt",
            "/m/export.csv",
            "/m/m3u8",
            "/m/папка",
        ] {
            assert!(!is_playlist_file(other), "{other}");
        }
    }
}
