//! Мост «ядро ↔ окно». Команды Tauri вызывают фасад `player-core`, события
//! ядра уходят в окно как [`EVENT`]. Формат JSON закреплён тестом ядра
//! `player-core/tests/json.rs`; TS-типы окна — `src/core.ts`.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use player_core::{
    Command, Core, CoreConfig, CoreError, Event, Observer, Ports, ScrobbSettings, ScrobbleStatus,
    Stores,
};
use player_domain::{MediaUri, PlayerState, Track, TrackId};
use player_history::PlayRecord;
use player_library::{MemoryLibrary, tags};
use player_output_cpal::CpalOutput;
use player_store_sqlite::SqliteHistory;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::adapters::{KeyringSecrets, LocalFiles, SystemClock, UreqHttp};

/// Событие окна с [`Event`] ядра.
pub const EVENT: &str = "core://event";
/// Сервер Scrobb по умолчанию; для разработки — переменная `SCROBB_BASE_URL`.
const DEFAULT_SCROBB_URL: &str = "https://scrobb.ru";

/// Состояние оболочки: ядро и то, что окну нужно сверх него.
pub struct Player {
    /// `Option` — чтобы при выходе удалить ядро явно: оно закроет открытое
    /// прослушивание и снимет «сейчас играет».
    core: Mutex<Option<Core>>,
    library: Arc<MemoryLibrary>,
    /// Добавленные треки по порядку — для списка в окне. Сканера и базы
    /// библиотеки пока нет, поэтому список живёт до закрытия приложения.
    tracks: Mutex<Vec<Track>>,
    forwarder: Arc<Forwarder>,
    scrobbling_available: bool,
}

impl Player {
    /// Собрать ядро: адаптеры десктопа, история в каталоге данных приложения.
    pub fn start(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let data_dir = app.path().app_data_dir()?;
        std::fs::create_dir_all(&data_dir)?;
        let history = Arc::new(SqliteHistory::open(data_dir.join("history.db"))?);
        let library = Arc::new(MemoryLibrary::new());
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
            library: library.clone(),
            history: Some(history),
        };
        let core = Core::new(ports, stores, config, forwarder.clone());
        let scrobbling_available = core.capabilities().scrobbling;
        Ok(Self {
            core: Mutex::new(Some(core)),
            library,
            tracks: Mutex::new(Vec::new()),
            forwarder,
            scrobbling_available,
        })
    }

    /// Файлы из командной строки («Открыть с помощью» в файловом менеджере):
    /// добавить и играть с первого.
    pub fn open_files(&self, paths: Vec<String>) {
        let added = add(&self.library, &self.tracks, paths);
        if let Some(first) = added.tracks.first() {
            let all: Vec<_> = lock(&self.tracks).iter().map(|t| t.id).collect();
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
    scrobbling: Option<ScrobbleStatus>,
    scrobbling_available: bool,
}

/// Итог добавления файлов.
#[derive(Debug, Serialize)]
pub struct Added {
    tracks: Vec<Track>,
    failed: Vec<String>,
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
        tracks: lock(&player.tracks).clone(),
        scrobbling: *lock(&player.forwarder.scrobbling),
        scrobbling_available: player.scrobbling_available,
    })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn add_files(player: State<'_, Player>, paths: Vec<String>) -> Result<Added, String> {
    Ok(add(&player.library, &player.tracks, paths))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn recent_plays(
    player: State<'_, Player>,
    before_ms: Option<i64>,
    limit: usize,
) -> Result<Vec<PlayRecord>, CoreError> {
    player.with_core(|core| core.recent_plays(before_ms, limit))
}

/// Прочитать теги и добавить в библиотеку; уже добавленные пути пропускаются.
fn add(library: &MemoryLibrary, list: &Mutex<Vec<Track>>, paths: Vec<String>) -> Added {
    let mut known: HashSet<String> = lock(list).iter().map(|t| t.uri.0.clone()).collect();
    let mut tracks = Vec::new();
    let mut failed = Vec::new();
    for path in paths {
        if !known.insert(path.clone()) {
            continue;
        }
        let mut all = lock(list);
        let id = TrackId(all.len() as u64 + 1);
        match tags::read_track(&LocalFiles, &MediaUri(path.clone()), id) {
            Ok(track) => {
                library.insert(track.clone());
                all.push(track.clone());
                tracks.push(track);
            }
            Err(_) => failed.push(file_name(&path)),
        }
    }
    Added { tracks, failed }
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

    fn fixture(name: &str) -> String {
        format!(
            "{}/../../../core/crates/player-testkit/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        )
    }

    #[test]
    fn added_files_get_tags_ids_and_are_not_duplicated() {
        let library = MemoryLibrary::new();
        let list = Mutex::new(Vec::new());
        let tagged = fixture("tagged.flac");
        let added = add(
            &library,
            &list,
            vec![
                tagged.clone(),
                fixture("a.flac"),
                tagged.clone(),
                fixture("nope.flac"),
            ],
        );
        let titles: Vec<_> = added.tracks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["Кукушка", "a"]);
        assert_eq!(added.failed, ["nope.flac"]);
        assert_eq!(library.track(TrackId(1)).unwrap().artists, ["Кино"]);

        // Повторное добавление того же файла ничего не меняет.
        let again = add(&library, &list, vec![tagged]);
        assert!(again.tracks.is_empty() && again.failed.is_empty());
        assert_eq!(lock(&list).len(), 2);
        assert_eq!(lock(&list)[1].id, TrackId(2));
    }
}
