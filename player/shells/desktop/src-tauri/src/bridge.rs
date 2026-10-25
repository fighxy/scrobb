//! Мост «ядро ↔ окно». Команды Tauri вызывают фасад `player-core`, события
//! ядра уходят в окно как [`EVENT`], события библиотеки — как [`LIBRARY_EVENT`].
//! Формат JSON закреплён тестом ядра `player-core/tests/json.rs`; TS-типы
//! окна — `src/core.ts`.

use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use player_core::{
    Command, Core, CoreConfig, CoreError, Event, Observer, Ports, ScrobbSettings, ScrobbleStatus,
    Stores,
};
use player_domain::{PlayerState, Track};
use player_history::PlayRecord;
use player_library::MemoryLibrary;
use player_output_cpal::CpalOutput;
use player_store_sqlite::SqliteHistory;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::adapters::{KeyringSecrets, LocalFiles, SystemClock, UreqHttp};
use crate::library::{Added, LibraryEvent, LocalLibrary, Scanning};

/// Событие окна с [`Event`] ядра.
pub const EVENT: &str = "core://event";
/// Событие окна с [`LibraryEvent`].
pub const LIBRARY_EVENT: &str = "library://event";
/// Сервер Scrobb по умолчанию; для разработки — переменная `SCROBB_BASE_URL`.
const DEFAULT_SCROBB_URL: &str = "https://scrobb.ru";

/// Состояние оболочки: ядро и то, что окну нужно сверх него.
pub struct Player {
    /// `Option` — чтобы при выходе удалить ядро явно: оно закроет открытое
    /// прослушивание и снимет «сейчас играет».
    core: Mutex<Option<Core>>,
    library: Arc<LocalLibrary>,
    /// Очередь папок для потока сканера: сканирование не блокирует окно.
    scanner: mpsc::Sender<String>,
    forwarder: Arc<Forwarder>,
    scrobbling_available: bool,
}

impl Player {
    /// Собрать ядро: адаптеры десктопа, история в каталоге данных приложения.
    pub fn start(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let data_dir = app.path().app_data_dir()?;
        std::fs::create_dir_all(&data_dir)?;
        let history = Arc::new(SqliteHistory::open(data_dir.join("history.db"))?);
        let memory = Arc::new(MemoryLibrary::new());
        let library = Arc::new(LocalLibrary::open(
            data_dir.join("library.json"),
            memory.clone(),
        ));
        let scanner = spawn_scanner(library.clone(), app.clone());
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
        };
        let core = Core::new(ports, stores, config, forwarder.clone());
        let scrobbling_available = core.capabilities().scrobbling;
        Ok(Self {
            core: Mutex::new(Some(core)),
            library,
            scanner,
            forwarder,
            scrobbling_available,
        })
    }

    /// Аргументы командной строки («Открыть с помощью» в файловом менеджере):
    /// папки становятся источниками, файлы добавляются и играют с первого.
    pub fn open_paths(&self, paths: Vec<String>) {
        let (folders, files): (Vec<_>, Vec<_>) =
            paths.into_iter().partition(|p| Path::new(p).is_dir());
        for folder in folders {
            if let Err(e) = self.add_folder(&folder) {
                self.forwarder.emit_library(LibraryEvent::Error(e));
            }
        }
        let added = self.library.add_files(files);
        if let Some(first) = added.tracks.first() {
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
