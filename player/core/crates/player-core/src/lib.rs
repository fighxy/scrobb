//! Фасад ядра: единственная точка входа для оболочек (через `bindings/ffi`).
//!
//! Однонаправленный поток данных: оболочка шлёт [`Command`], ядро меняет
//! состояние и сообщает [`Event`] наблюдателю. Модули связываются здесь:
//! очередь (`player-playback`) решает, что играть, библиотека
//! (`player-library`) превращает идентификатор в файл, движок
//! (`player-engine`) играет и сообщает, что реально звучит.
//!
//! События движка обрабатывает отдельный поток. Наблюдатель и медиасессия
//! вызываются без удержания блокировок, поэтому из колбэка можно сразу
//! отправить новую команду.
//!
//! Пока не подключены: история и скробблинг, сеть, привязка аккаунта Scrobb.

use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::{self, JoinHandle};

use player_domain::{PlaybackStatus, PlayerState, RepeatMode, Track, TrackId};
use player_engine::{Engine, EngineConfig, EngineEvent, EngineMessage, EngineTrack};
use player_library::TrackStore;
use player_playback::Queue;
use player_ports::{AudioOutput, Clock, FileAccess, HttpTransport, MediaSession, SecureStore};

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

/// Настройки ядра.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CoreConfig {
    /// Звуковой движок.
    pub engine: EngineConfig,
}

/// Команда от оболочки: кнопки, медиаклавиши, экран блокировки, автомобиль.
#[derive(Debug, Clone, PartialEq)]
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
    /// Перемотка.
    Seek {
        /// Позиция.
        position_ms: u64,
    },
    /// Режим повтора.
    SetRepeat(RepeatMode),
    /// Громкость 0.0–1.0.
    SetVolume(f32),
    /// Привязать устройство к аккаунту Scrobb (`docs/architecture.md` §18.4).
    LinkScrobbAccount {
        /// Имя устройства в «Приложениях».
        device_name: String,
    },
}

/// Событие для оболочки.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Новый снимок состояния.
    State(Box<PlayerState>),
    /// Показать код привязки и открыть страницу на сайте.
    ScrobbLinkCode {
        /// Код для сверки.
        user_code: String,
        /// Адрес страницы с подставленным кодом.
        verification_uri_complete: String,
    },
    /// Ошибка, которую стоит показать пользователю.
    Error(String),
}

/// Получатель событий: реализуется оболочкой через callback-интерфейс привязок.
pub trait Observer: Send + Sync {
    /// Новое событие.
    fn on_event(&self, event: Event);
}

/// Что доступно на этой платформе; оболочка показывает только доступное.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Вывод без изменений через USB-ЦАП или монопольный режим.
    pub bit_perfect: bool,
    /// Установка сторонних плагинов (на iOS — нет).
    pub install_plugins: bool,
    /// Отправка прослушиваний в Scrobb.
    pub scrobbling: bool,
}

/// Ошибка команды.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// Команда ещё не реализована в заготовке.
    NotImplemented(&'static str),
    /// Нет нужного адаптера.
    MissingPort(&'static str),
    /// Пустая очередь или начальная позиция за её пределами.
    InvalidQueue,
}

/// Ядро плеера.
pub struct Core {
    /// `Option` — только чтобы в `drop` отпустить ядро раньше, чем ждать поток событий.
    inner: Option<Arc<Inner>>,
    events: Option<JoinHandle<()>>,
}

impl Core {
    /// Создать ядро и запустить движок.
    pub fn new(
        ports: Ports,
        library: Arc<dyn TrackStore>,
        config: CoreConfig,
        observer: Arc<dyn Observer>,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let engine = Engine::new(ports.audio, ports.files, config.engine, tx);
        let inner = Arc::new(Inner {
            engine,
            library,
            observer,
            media_session: ports.media_session,
            http: ports.http,
            secrets: ports.secrets,
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

    /// Возможности платформы.
    pub fn capabilities(&self) -> Capabilities {
        let inner = self.inner();
        Capabilities {
            bit_perfect: false,
            install_plugins: !cfg!(target_os = "ios"),
            scrobbling: inner.http.is_some() && inner.secrets.is_some(),
        }
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
                    }
                });
            }
            Command::Resume => inner.update(|s| match s.state.status {
                PlaybackStatus::Stopped => inner.start_current(s),
                _ => {
                    inner.engine.resume();
                    if s.state.current.is_some() {
                        s.state.status = PlaybackStatus::Playing;
                    }
                }
            }),
            Command::Stop => inner.update(|s| {
                s.epoch = inner.engine.stop();
                stop_session(s);
            }),
            Command::Next => inner.update(|s| {
                if s.queue.peek(1).is_some() {
                    s.queue.advance();
                    s.skips = 0;
                    inner.start_current(s);
                } else {
                    // Конец очереди: остановиться на последнем треке.
                    s.epoch = inner.engine.stop();
                    stop_session(s);
                }
            }),
            Command::Previous => {
                let position = inner.engine.playhead().map_or(0, |p| p.position_ms);
                if position > RESTART_THRESHOLD_MS {
                    inner.engine.seek(0);
                } else {
                    inner.update(|s| {
                        s.queue.back();
                        s.skips = 0;
                        inner.start_current(s);
                    });
                }
            }
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
            Command::LinkScrobbAccount { .. } => {
                if inner.http.is_none() {
                    return Err(CoreError::MissingPort("http"));
                }
                return Err(CoreError::NotImplemented(
                    "привязка: сетевой поток не подключён",
                ));
            }
        }
        Ok(())
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        // Последняя сильная ссылка: движок останавливается, канал событий закрывается.
        drop(self.inner.take());
        if let Some(events) = self.events.take() {
            // Ядро удалили из колбэка наблюдателя — поток событий завершится сам.
            if events.thread().id() != thread::current().id() {
                let _ = events.join();
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
    observer: Arc<dyn Observer>,
    media_session: Option<Arc<dyn MediaSession>>,
    http: Option<Arc<dyn HttpTransport>>,
    secrets: Option<Arc<dyn SecureStore>>,
    session: Mutex<Session>,
}

impl Inner {
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
        if let (Some(playhead), Some(track)) = (self.engine.playhead(), &state.current) {
            if playhead.track == track.id {
                state.position_ms = playhead.position_ms;
            }
        }
        state
    }

    /// Начать текущий трек очереди с начала; треки, которых нет в библиотеке, пропустить.
    fn start_current(&self, s: &mut Session) {
        loop {
            let Some(id) = s.queue.current() else {
                return stop_session(s);
            };
            let Some(track) = self.library.track(id) else {
                s.outbox
                    .push(Event::Error(format!("трека {} нет в библиотеке", id.0)));
                s.skips += 1;
                if s.skips >= MAX_SKIPS.min(s.queue.len()) || s.queue.advance().is_none() {
                    return stop_session(s);
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
                }
            }),
            EngineEvent::Finished => self.update(|s| {
                if s.epoch == epoch {
                    stop_session(s);
                }
            }),
            EngineEvent::Error { track, message } => self.update(|s| {
                if s.epoch != epoch {
                    return;
                }
                s.outbox.push(Event::Error(message));
                // Если не открылся следующий трек, делать ничего не нужно: движок
                // сам возьмёт следующий из списка (`LOOKAHEAD`) без паузы.
                if track.is_some() && track == s.starting {
                    s.starting = None;
                    s.skips += 1;
                    if s.skips < MAX_SKIPS.min(s.queue.len()) && s.queue.advance().is_some() {
                        self.start_current(s);
                    } else {
                        stop_session(s);
                    }
                }
            }),
        }
    }
}

/// Остановка: трек и позиция в очереди сохраняются, чтобы «играть» начало заново.
fn stop_session(s: &mut Session) {
    s.state.status = PlaybackStatus::Stopped;
    s.state.position_ms = 0;
    s.starting = None;
    s.decoding = None;
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
