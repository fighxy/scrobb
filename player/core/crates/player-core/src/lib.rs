//! Фасад ядра: единственная точка входа для оболочек (через `bindings/ffi`).
//!
//! Однонаправленный поток данных: оболочка шлёт [`Command`], ядро меняет
//! состояние и сообщает [`Event`] наблюдателю. Модули связываются здесь:
//! очередь (`player-playback`) решает, что играть, библиотека
//! (`player-library`) превращает идентификатор в файл, движок
//! (`player-engine`) играет и сообщает, что реально звучит, история
//! (`player-history`) запоминает прослушивания и отправляет засчитанные.
//!
//! События движка обрабатывает отдельный поток, историю и связь со Scrobb
//! (привязка устройства, «сейчас играет», отправка) — ещё один.
//! Наблюдатель и медиасессия вызываются без удержания блокировок, поэтому
//! из колбэка можно сразу отправить новую команду.

use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::{self, JoinHandle};

use player_domain::{PlaybackStatus, PlayerState, RepeatMode, Track, TrackId};
use player_engine::{Engine, EngineConfig, EngineEvent, EngineMessage, EngineTrack};
use player_history::{HistoryStore, Outcome, PlayRecord};
use player_library::TrackStore;
use player_playback::Queue;
use player_ports::{AudioOutput, Clock, FileAccess, HttpTransport, MediaSession, SecureStore};
use scrobb_api::{ClientKind, Thresholds};
use scrobb_client::{ClientInfo, ScrobbClient};
use serde::{Deserialize, Serialize};

mod history;
mod scrobb;

use history::{HistoryService, Message, Stamp};
use scrobb::{Remote, ScrobbPorts};

pub use scrobb::SCROBB_TOKEN_KEY;

/// «Назад» в первые 3 секунды трека — предыдущий трек, позже — начало текущего.
pub const RESTART_THRESHOLD_MS: u64 = 3_000;
/// Сколько треков подряд можно пропустить из-за ошибок, прежде чем остановиться.
const MAX_SKIPS: usize = 10;
/// Сколько следующих треков передавать движку: сломанный он перешагнёт сам, без ожидания.
const LOOKAHEAD: usize = 3;

/// Адаптеры, которые оболочка передаёт ядру при создании.
pub struct Ports {
    /// Часы.
    pub clock: Arc<dyn Clock>,
    /// Вывод звука.
    pub audio: Box<dyn AudioOutput>,
    /// Доступ к файлам.
    pub files: Arc<dyn FileAccess>,
    /// Сеть; без неё плеер работает офлайн, скробблы копятся в очереди.
    pub http: Option<Arc<dyn HttpTransport>>,
    /// Хранилище секретов: токен устройства Scrobb.
    pub secrets: Option<Arc<dyn SecureStore>>,
    /// Системная медиасессия.
    pub media_session: Option<Arc<dyn MediaSession>>,
}

/// Хранилища, которые оболочка открывает и передаёт ядру.
pub struct Stores {
    /// Библиотека: трек по идентификатору.
    pub library: Arc<dyn TrackStore>,
    /// История прослушиваний (`player-store-sqlite`); без неё ничего не запоминается
    /// и не скробблится.
    pub history: Option<Arc<dyn HistoryStore>>,
}

/// Настройки ядра.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoreConfig {
    /// Звуковой движок.
    pub engine: EngineConfig,
    /// Пороги скроббла; позже приходят из `GET /api/v1/scrobble/config`.
    pub thresholds: Thresholds,
    /// Куда отправлять скробблы; `None` — только локальная история.
    pub scrobb: Option<ScrobbSettings>,
}

/// Сервер Scrobb и то, как плеер себя называет (`User-Agent`, §18.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrobbSettings {
    /// Адрес API, например `https://scrobb.ru`.
    pub base_url: String,
    /// Версия плеера.
    pub version: String,
    /// Платформа: `android`, `ios`, `windows`, `macos`, `linux`.
    pub platform: String,
}

/// Команда от оболочки: кнопки, медиаклавиши, экран блокировки, автомобиль.
///
/// В JSON — `{"type": "play_queue", "data": {"tracks": [1, 2], "start": 0}}`,
/// `{"type": "pause"}`, `{"type": "set_volume", "data": 0.5}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Command {
    /// Играть очередь с позиции `start`.
    PlayQueue {
        /// Треки.
        tracks: Vec<TrackId>,
        /// С какого начать.
        start: usize,
    },
    /// Пауза.
    Pause,
    /// Продолжить; после остановки — играть текущий трек очереди с начала.
    Resume,
    /// Остановить; очередь и позиция в ней сохраняются.
    Stop,
    /// Следующий трек.
    Next,
    /// В первые [`RESTART_THRESHOLD_MS`] — предыдущий трек, позже — начало текущего.
    Previous,
    /// Перемотка; в самое начало — новое прослушивание того же трека (§18.7).
    Seek {
        /// Позиция.
        position_ms: u64,
    },
    /// Режим повтора.
    SetRepeat(RepeatMode),
    /// Громкость 0.0–1.0.
    SetVolume(f32),
    /// Привязать устройство к аккаунту Scrobb (`docs/architecture.md` §18.4):
    /// придёт [`Event::ScrobbLinkCode`], пользователь подтверждает код на сайте.
    LinkScrobbAccount {
        /// Имя устройства в «Приложениях».
        device_name: String,
    },
    /// Прервать привязку: пользователь закрыл окно с кодом.
    CancelScrobbLink,
    /// Отвязать устройство: отозвать токен и удалить его; очередь сохраняется.
    UnlinkScrobbAccount,
}

/// Событие для оболочки. В JSON — `{"type": "state", "data": {…}}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// Новый снимок состояния.
    State(Box<PlayerState>),
    /// Показать код привязки и открыть страницу на сайте (системным браузером:
    /// пользователь сверяет код и подтверждает привязку там).
    ScrobbLinkCode {
        /// Код для сверки.
        user_code: String,
        /// Адрес страницы с подставленным кодом.
        verification_uri_complete: String,
    },
    /// Отправка в Scrobb: «в очереди N», «войдите заново» (§18.2).
    Scrobbling(ScrobbleStatus),
    /// Ошибка, которую стоит показать пользователю.
    Error(String),
}

/// Состояние отправки в Scrobb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrobbleStatus {
    /// Сколько засчитанных прослушиваний ждёт отправки.
    pub pending: usize,
    /// Что с отправкой.
    pub state: ScrobbleState,
}

/// Что с отправкой в Scrobb.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrobbleState {
    /// Устройство не привязано: история пишется, в очередь ничего не ставится.
    NotLinked,
    /// Идёт привязка: ждём, пока пользователь подтвердит код на сайте.
    Linking,
    /// Всё отправляется.
    Ready,
    /// Сеть или сервер недоступны — повтор позже (§18.8).
    Retrying,
    /// Токен отозван: нужно войти заново; очередь сохранена.
    Unauthorized,
    /// Версия плеера устарела: нужно обновиться; очередь сохранена.
    UpgradeRequired,
}

/// Получатель событий: реализуется оболочкой через callback-интерфейс привязок.
pub trait Observer: Send + Sync {
    /// Новое событие.
    fn on_event(&self, event: Event);
}

/// Что доступно на этой платформе; оболочка показывает только доступное.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Вывод без изменений через USB-ЦАП или монопольный режим.
    pub bit_perfect: bool,
    /// Установка сторонних плагинов (на iOS — нет).
    pub install_plugins: bool,
    /// Отправка прослушиваний в Scrobb.
    pub scrobbling: bool,
}

/// Ошибка команды. В JSON — `{"type": "missing_port", "data": "http"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum CoreError {
    /// Команда ещё не реализована в заготовке.
    NotImplemented(&'static str),
    /// Нет нужного адаптера или хранилища.
    MissingPort(&'static str),
    /// Не задана настройка, например сервер Scrobb в [`CoreConfig::scrobb`].
    NotConfigured(&'static str),
    /// Пустая очередь или начальная позиция за её пределами.
    InvalidQueue,
    /// Ошибка хранилища истории.
    History(String),
}

/// Ядро плеера.
pub struct Core {
    /// `Option` — только чтобы в `drop` отпустить ядро раньше, чем ждать потоки.
    inner: Option<Arc<Inner>>,
    events: Option<JoinHandle<()>>,
    history: Option<JoinHandle<()>>,
}

impl Core {
    /// Создать ядро и запустить движок.
    pub fn new(
        ports: Ports,
        stores: Stores,
        config: CoreConfig,
        observer: Arc<dyn Observer>,
    ) -> Self {
        // Связь со Scrobb нужна вся сразу: сеть, секреты, история и адрес сервера.
        let (scrobb, remote) = match (&ports.http, &ports.secrets, &stores.history, &config.scrobb)
        {
            (None, ..) => (Err(CoreError::MissingPort("http")), None),
            (_, None, ..) => (Err(CoreError::MissingPort("secrets")), None),
            (_, _, None, _) => (Err(CoreError::MissingPort("history")), None),
            (_, _, _, None) => (Err(CoreError::NotConfigured("scrobb")), None),
            (Some(http), Some(secrets), Some(store), Some(settings)) => {
                let scrobb_ports = ScrobbPorts {
                    client: ScrobbClient::new(
                        settings.base_url.clone(),
                        ClientInfo {
                            kind: ClientKind::Player,
                            version: settings.version.clone(),
                            platform: settings.platform.clone(),
                        },
                    ),
                    http: http.clone(),
                    secrets: secrets.clone(),
                };
                let remote = Remote::new(
                    scrobb_ports,
                    store.clone(),
                    ports.clock.clone(),
                    observer.clone(),
                );
                (Ok(()), Some(remote))
            }
        };
        let (messages, history) = match &stores.history {
            Some(store) => {
                let (tx, rx) = mpsc::channel();
                let service = HistoryService::new(
                    store.clone(),
                    ports.clock.clone(),
                    observer.clone(),
                    config.thresholds,
                    remote,
                );
                (Some(tx), Some(service.spawn(rx)))
            }
            None => (None, None),
        };
        let (tx, rx) = mpsc::channel();
        let engine = Engine::new(ports.audio, ports.files, config.engine, tx);
        let inner = Arc::new(Inner {
            engine,
            library: stores.library,
            history: stores.history,
            messages,
            clock: ports.clock,
            observer,
            media_session: ports.media_session,
            scrobb,
            session: Mutex::new(Session {
                state: PlayerState {
                    volume: 1.0,
                    ..PlayerState::default()
                },
                queue: Queue::default(),
                starting: None,
                decoding: None,
                epoch: 0,
                skips: 0,
                outbox: Vec::new(),
            }),
        });
        // Поток событий держит слабую ссылку: когда ядро удалено, движок
        // останавливается, канал закрывается, и поток завершается сам.
        let weak: Weak<Inner> = Arc::downgrade(&inner);
        let events = thread::Builder::new()
            .name("player-core-events".into())
            .spawn(move || {
                for message in rx {
                    let Some(inner) = weak.upgrade() else {
                        break;
                    };
                    inner.on_engine_message(message);
                }
            })
            .expect("поток событий запускается");
        Self {
            inner: Some(inner),
            events: Some(events),
            history,
        }
    }

    fn inner(&self) -> &Inner {
        self.inner.as_ref().expect("ядро живо до drop")
    }

    /// Текущий снимок состояния; позиция — та, что звучит прямо сейчас.
    pub fn state(&self) -> PlayerState {
        let inner = self.inner();
        let state = lock(&inner.session).state.clone();
        inner.with_live_position(state)
    }

    /// Треки очереди по порядку; где в ней текущий — `PlayerState::queue_position`.
    /// Отдельно от снимка: очередь бывает длиной во всю библиотеку, а снимок
    /// уходит в окно при каждой смене состояния.
    pub fn queue(&self) -> Vec<TrackId> {
        lock(&self.inner().session).queue.tracks().to_vec()
    }

    /// Возможности платформы.
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            bit_perfect: false,
            install_plugins: !cfg!(target_os = "ios"),
            scrobbling: self.inner().scrobb.is_ok(),
        }
    }

    /// Лента истории: новые первыми, начатые строго раньше `before_ms`
    /// (для следующей страницы — время начала последней полученной записи).
    pub fn recent_plays(
        &self,
        before_ms: Option<i64>,
        limit: usize,
    ) -> Result<Vec<PlayRecord>, CoreError> {
        let history = self
            .inner()
            .history
            .as_ref()
            .ok_or(CoreError::MissingPort("history"))?;
        history
            .recent(before_ms, limit)
            .map_err(|e| CoreError::History(e.0))
    }

    /// Выполнить команду.
    pub fn dispatch(&self, command: Command) -> Result<(), CoreError> {
        let inner = self.inner();
        match command {
            Command::PlayQueue { tracks, start } => {
                if start >= tracks.len() {
                    return Err(CoreError::InvalidQueue);
                }
                inner.update(|s| {
                    inner.report(|at| Message::Ended {
                        outcome: Outcome::Skipped,
                        at,
                    });
                    let repeat = s.queue.repeat;
                    s.queue = Queue::new(tracks, start);
                    s.queue.repeat = repeat;
                    s.skips = 0;
                    inner.start_current(s);
                });
            }
            Command::Pause => {
                inner.engine.pause();
                inner.update(|s| {
                    if s.state.current.is_some() && s.state.status != PlaybackStatus::Stopped {
                        s.state.status = PlaybackStatus::Paused;
                        inner.report(|at| Message::Paused { at });
                    }
                });
            }
            Command::Resume => inner.update(|s| match s.state.status {
                PlaybackStatus::Stopped => inner.start_current(s),
                _ => {
                    inner.engine.resume();
                    if s.state.current.is_some() {
                        s.state.status = PlaybackStatus::Playing;
                        let position_ms = inner.engine.playhead().map(|p| p.position_ms);
                        inner.report(|at| Message::Resumed { at, position_ms });
                    }
                }
            }),
            Command::Stop => inner.update(|s| {
                s.epoch = inner.engine.stop();
                inner.stop_session(s);
            }),
            Command::Next => inner.update(|s| {
                inner.report(|at| Message::Ended {
                    outcome: Outcome::Skipped,
                    at,
                });
                if s.queue.peek(1).is_some() {
                    s.queue.advance();
                    s.skips = 0;
                    inner.start_current(s);
                } else {
                    // Конец очереди: остановиться на последнем треке.
                    s.epoch = inner.engine.stop();
                    inner.stop_session(s);
                }
            }),
            Command::Previous => {
                let position = inner.engine.playhead().map_or(0, |p| p.position_ms);
                if position > RESTART_THRESHOLD_MS {
                    inner.restart();
                } else {
                    inner.update(|s| {
                        inner.report(|at| Message::Ended {
                            outcome: Outcome::Skipped,
                            at,
                        });
                        s.queue.back();
                        s.skips = 0;
                        inner.start_current(s);
                    });
                }
            }
            Command::Seek { position_ms: 0 } => inner.restart(),
            Command::Seek { position_ms } => inner.engine.seek(position_ms),
            Command::SetRepeat(mode) => inner.update(|s| {
                s.queue.repeat = mode;
                s.state.repeat = mode;
                // Пересчитать следующие треки от того, что движок готовит сейчас.
                if let Some(decoding) = s.decoding {
                    let steps = match s.queue.current() {
                        Some(current) if current == decoding => Some(0),
                        _ => s.queue.steps_to(decoding),
                    };
                    if let Some(steps) = steps {
                        inner.send_upcoming(s, decoding, steps + 1);
                    }
                }
            }),
            Command::SetVolume(volume) => {
                inner.engine.set_volume(volume);
                inner.update(|s| s.state.volume = volume.clamp(0.0, 1.0));
            }
            Command::LinkScrobbAccount { device_name } => {
                inner.scrobb.clone()?;
                inner.report(|_| Message::Link { device_name });
            }
            Command::CancelScrobbLink => {
                inner.scrobb.clone()?;
                inner.report(|_| Message::CancelLink);
            }
            Command::UnlinkScrobbAccount => {
                inner.scrobb.clone()?;
                inner.report(|_| Message::Unlink);
            }
        }
        Ok(())
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        // Закрыть открытое прослушивание и снять «сейчас играет».
        if let Some(inner) = &self.inner {
            inner.report(|at| Message::Stopped { at });
        }
        // Последняя сильная ссылка: движок останавливается, каналы событий и истории закрываются.
        drop(self.inner.take());
        // Ядро удалили из колбэка наблюдателя — этот поток завершится сам.
        for handle in [self.events.take(), self.history.take()]
            .into_iter()
            .flatten()
        {
            if handle.thread().id() != thread::current().id() {
                let _ = handle.join();
            }
        }
    }
}

/// Изменяемое состояние ядра; всё под одной блокировкой.
struct Session {
    state: PlayerState,
    queue: Queue,
    /// Трек, который фасад попросил движок начать, а тот ещё не начал.
    starting: Option<TrackId>,
    /// Трек, который движок готовит сейчас: к нему привязан список следующих.
    decoding: Option<TrackId>,
    /// Эпоха движка от последних `play` или `stop` фасада: события других эпох
    /// относятся к отменённому и отбрасываются.
    epoch: u64,
    /// Сколько треков подряд пропущено из-за ошибок.
    skips: usize,
    /// События, накопленные под блокировкой; отправляются после её снятия.
    outbox: Vec<Event>,
}

struct Inner {
    engine: Engine,
    library: Arc<dyn TrackStore>,
    history: Option<Arc<dyn HistoryStore>>,
    /// Сообщения потоку истории; `None` — история не ведётся.
    messages: Option<mpsc::Sender<Message>>,
    clock: Arc<dyn Clock>,
    observer: Arc<dyn Observer>,
    media_session: Option<Arc<dyn MediaSession>>,
    /// Доступна ли связь со Scrobb; если нет — почему.
    scrobb: Result<(), CoreError>,
    session: Mutex<Session>,
}

impl Inner {
    /// Сообщить потоку истории. Факты вызываются под блокировкой сессии,
    /// поэтому идут в том же порядке, что и изменения состояния.
    fn report(&self, make: impl FnOnce(Stamp) -> Message) {
        if let Some(messages) = &self.messages {
            let at = Stamp {
                mono_ms: self.clock.monotonic_ms(),
                unix_ms: self.clock.unix_ms(),
            };
            let _ = messages.send(make(at));
        }
    }

    /// Остановка: трек и позиция в очереди сохраняются, чтобы «играть» начало заново.
    fn stop_session(&self, s: &mut Session) {
        s.state.status = PlaybackStatus::Stopped;
        s.state.position_ms = 0;
        s.starting = None;
        s.decoding = None;
        self.report(|at| Message::Stopped { at });
    }

    /// Текущий трек зазвучал (или зазвучал заново).
    fn report_started(&self, s: &Session) {
        if let Some(track) = &s.state.current {
            self.report(|at| Message::Started {
                track: Box::new(track.clone()),
                at,
            });
            if s.state.status == PlaybackStatus::Paused {
                self.report(|at| Message::Paused { at });
            }
        }
    }

    /// Текущий трек — с начала: это новое прослушивание (§18.7).
    fn restart(&self) {
        self.engine.seek(0);
        let s = lock(&self.session);
        if s.state.status != PlaybackStatus::Stopped && s.starting.is_none() {
            self.report(|at| Message::Ended {
                outcome: Outcome::Skipped,
                at,
            });
            self.report_started(&s);
        }
    }

    /// Изменить сессию, затем — уже без блокировки — разослать события и новое состояние.
    fn update<R>(&self, f: impl FnOnce(&mut Session) -> R) -> R {
        let (result, outbox, state) = {
            let mut s = lock(&self.session);
            let result = f(&mut s);
            (result, std::mem::take(&mut s.outbox), s.state.clone())
        };
        for event in outbox {
            self.observer.on_event(event);
        }
        let state = self.with_live_position(state);
        if let Some(session) = &self.media_session {
            match &state.current {
                Some(track) if state.status != PlaybackStatus::Stopped => {
                    session.publish(track, state.status, state.position_ms)
                }
                _ => session.clear(),
            }
        }
        self.observer.on_event(Event::State(Box::new(state)));
        result
    }

    /// Подставить позицию, которая звучит прямо сейчас, если это текущий трек.
    fn with_live_position(&self, mut state: PlayerState) -> PlayerState {
        if let (Some(playhead), Some(track)) = (self.engine.playhead(), &state.current)
            && playhead.track == track.id
        {
            state.position_ms = playhead.position_ms;
        }
        state
    }

    /// Начать текущий трек очереди с начала; треки, которых нет в библиотеке, пропустить.
    fn start_current(&self, s: &mut Session) {
        loop {
            let Some(id) = s.queue.current() else {
                return self.stop_session(s);
            };
            let Some(track) = self.library.track(id) else {
                s.outbox
                    .push(Event::Error(format!("трека {} нет в библиотеке", id.0)));
                s.skips += 1;
                if s.skips >= MAX_SKIPS.min(s.queue.len()) || s.queue.advance().is_none() {
                    return self.stop_session(s);
                }
                continue;
            };
            s.epoch = self.engine.play(engine_track(&track));
            s.starting = Some(id);
            s.decoding = Some(id);
            s.state.current = Some(track);
            s.state.status = PlaybackStatus::Buffering;
            s.state.position_ms = 0;
            s.state.queue_position = s.queue.position();
            self.send_upcoming(s, id, 1);
            return;
        }
    }

    /// Передать движку треки, идущие за `after`: начиная с `from_steps` переходов вперёд.
    fn send_upcoming(&self, s: &Session, after: TrackId, from_steps: usize) {
        let tracks = (from_steps..from_steps + LOOKAHEAD)
            .map_while(|steps| s.queue.peek(steps))
            .filter_map(|id| self.library.track(id))
            .map(|t| engine_track(&t))
            .collect();
        self.engine.set_upcoming(after, tracks);
    }

    fn on_engine_message(&self, message: EngineMessage) {
        let EngineMessage { epoch, event } = message;
        // Событие отменённого (трек сменили, пока событие шло): не трогаем состояние.
        // Проверка повторяется под блокировкой, где эпоха не может смениться.
        if lock(&self.session).epoch != epoch {
            return;
        }
        match event {
            EngineEvent::TrackStarted { track, .. } => self.update(|s| {
                if s.epoch != epoch {
                    return;
                }
                // При переходе без паузы движок сам ушёл вперёд: догнать его в очереди.
                s.queue.advance_to(track);
                s.state.current = self.library.track(track);
                s.state.status = if self.engine.is_paused() {
                    PlaybackStatus::Paused
                } else {
                    PlaybackStatus::Playing
                };
                s.state.position_ms = 0;
                s.state.queue_position = s.queue.position();
                s.starting = None;
                s.skips = 0;
                // Незакрытое прослушивание поток истории закроет как дослушанное.
                self.report_started(s);
            }),
            EngineEvent::Preloading { track } => {
                let mut s = lock(&self.session);
                if s.epoch != epoch {
                    return;
                }
                s.decoding = Some(track);
                // Движок взял следующий трек — передать те, что за ним.
                if let Some(steps) = s.queue.steps_to(track) {
                    self.send_upcoming(&s, track, steps + 1);
                }
            }
            EngineEvent::Seeked { track, position_ms } => self.update(|s| {
                if s.epoch == epoch && s.state.current.as_ref().is_some_and(|t| t.id == track) {
                    s.state.position_ms = position_ms;
                    self.report(|at| Message::Seeked { position_ms, at });
                }
            }),
            EngineEvent::Finished => self.update(|s| {
                if s.epoch == epoch {
                    self.report(|at| Message::Ended {
                        outcome: Outcome::Finished,
                        at,
                    });
                    self.stop_session(s);
                }
            }),
            EngineEvent::Error { track, message } => self.update(|s| {
                if s.epoch != epoch {
                    return;
                }
                s.outbox.push(Event::Error(message));
                if let Some(track) = track {
                    self.report(|at| Message::Failed { track, at });
                }
                // Если не открылся следующий трек, делать ничего не нужно: движок
                // сам возьмёт следующий из списка (`LOOKAHEAD`) без паузы.
                if track.is_some() && track == s.starting {
                    s.starting = None;
                    s.skips += 1;
                    if s.skips < MAX_SKIPS.min(s.queue.len()) && s.queue.advance().is_some() {
                        self.start_current(s);
                    } else {
                        self.stop_session(s);
                    }
                }
            }),
        }
    }
}

fn engine_track(track: &Track) -> EngineTrack {
    EngineTrack {
        id: track.id,
        uri: track.uri.clone(),
        replay_gain: track.replay_gain,
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
