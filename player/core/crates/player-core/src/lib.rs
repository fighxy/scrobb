//! Фасад ядра: единственная точка входа для оболочек (через `bindings/ffi`).
//!
//! Однонаправленный поток данных: оболочка шлёт [`Command`], ядро меняет
//! состояние и сообщает [`Event`] наблюдателю. Модули связываются здесь.
//!
//! Заготовка: движок, библиотека и сеть ещё не подключены — команды очереди
//! меняют только состояние, остальные возвращают [`CoreError::NotImplemented`].

use std::sync::{Arc, Mutex, MutexGuard};

use player_domain::{PlaybackStatus, PlayerState, RepeatMode, TrackId};
use player_playback::Queue;
use player_ports::{Clock, HttpTransport, MediaSession, SecureStore};

/// Адаптеры, которые оболочка передаёт ядру при создании.
pub struct Ports {
    /// Часы.
    pub clock: Arc<dyn Clock>,
    /// Сеть; без неё плеер работает офлайн, скробблы копятся в очереди.
    pub http: Option<Arc<dyn HttpTransport>>,
    /// Хранилище секретов: токен устройства Scrobb.
    pub secrets: Option<Arc<dyn SecureStore>>,
    /// Системная медиасессия.
    pub media_session: Option<Arc<dyn MediaSession>>,
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
    /// Продолжить.
    Resume,
    /// Следующий трек.
    Next,
    /// Предыдущий трек.
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
}

/// Ядро плеера.
pub struct Core {
    ports: Ports,
    observer: Arc<dyn Observer>,
    state: Mutex<PlayerState>,
    queue: Mutex<Queue>,
}

impl Core {
    /// Создать ядро.
    pub fn new(ports: Ports, observer: Arc<dyn Observer>) -> Self {
        let state = PlayerState {
            volume: 1.0,
            ..PlayerState::default()
        };
        Self {
            ports,
            observer,
            state: Mutex::new(state),
            queue: Mutex::new(Queue::default()),
        }
    }

    /// Текущий снимок состояния.
    pub fn state(&self) -> PlayerState {
        lock(&self.state).clone()
    }

    /// Возможности платформы.
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            bit_perfect: false,
            install_plugins: !cfg!(target_os = "ios"),
            scrobbling: self.ports.http.is_some() && self.ports.secrets.is_some(),
        }
    }

    /// Выполнить команду.
    pub fn dispatch(&self, command: Command) -> Result<(), CoreError> {
        match command {
            Command::PlayQueue { tracks, start } => {
                // Блокировку очереди снимаем до уведомления: наблюдатель может
                // сразу прислать новую команду.
                let position = {
                    let mut queue = lock(&self.queue);
                    let repeat = queue.repeat;
                    *queue = Queue::new(tracks, start);
                    queue.repeat = repeat;
                    queue.position()
                };
                self.update(|s| {
                    s.status = PlaybackStatus::Buffering;
                    s.position_ms = 0;
                    s.queue_position = position;
                });
            }
            Command::Pause => self.update(|s| s.status = PlaybackStatus::Paused),
            Command::Resume => self.update(|s| s.status = PlaybackStatus::Playing),
            Command::Next | Command::Previous => {
                let (moved, position) = {
                    let mut queue = lock(&self.queue);
                    let moved = if command == Command::Next {
                        queue.advance()
                    } else {
                        queue.back()
                    };
                    (moved, queue.position())
                };
                self.update(|s| {
                    s.position_ms = 0;
                    s.queue_position = position;
                    if moved.is_none() {
                        s.status = PlaybackStatus::Stopped;
                    }
                });
            }
            Command::SetRepeat(mode) => {
                lock(&self.queue).repeat = mode;
                self.update(|s| s.repeat = mode);
            }
            Command::SetVolume(v) => self.update(|s| s.volume = v.clamp(0.0, 1.0)),
            Command::Seek { .. } => {
                return Err(CoreError::NotImplemented("seek: движок не подключён"));
            }
            Command::LinkScrobbAccount { .. } => {
                if self.ports.http.is_none() {
                    return Err(CoreError::MissingPort("http"));
                }
                return Err(CoreError::NotImplemented(
                    "привязка: сетевой поток не подключён",
                ));
            }
        }
        Ok(())
    }

    fn update(&self, f: impl FnOnce(&mut PlayerState)) {
        let snapshot = {
            let mut state = lock(&self.state);
            f(&mut state);
            state.clone()
        };
        if let (Some(session), Some(track)) = (&self.ports.media_session, &snapshot.current) {
            session.publish(track, snapshot.status, snapshot.position_ms);
        }
        self.observer.on_event(Event::State(Box::new(snapshot)));
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedClock;
    impl Clock for FixedClock {
        fn monotonic_ms(&self) -> u64 {
            0
        }
        fn unix_ms(&self) -> i64 {
            0
        }
    }

    #[derive(Default)]
    struct Recorder(Mutex<Vec<Event>>);
    impl Observer for Recorder {
        fn on_event(&self, event: Event) {
            lock(&self.0).push(event);
        }
    }

    fn core() -> (Core, Arc<Recorder>) {
        let recorder = Arc::new(Recorder::default());
        let ports = Ports {
            clock: Arc::new(FixedClock),
            http: None,
            secrets: None,
            media_session: None,
        };
        (Core::new(ports, recorder.clone()), recorder)
    }

    #[test]
    fn queue_commands_emit_state_snapshots() {
        let (core, recorder) = core();
        core.dispatch(Command::PlayQueue {
            tracks: vec![TrackId(1), TrackId(2)],
            start: 0,
        })
        .unwrap();
        core.dispatch(Command::Next).unwrap();
        core.dispatch(Command::Next).unwrap();
        assert_eq!(core.state().status, PlaybackStatus::Stopped);
        assert_eq!(lock(&recorder.0).len(), 3);
    }

    #[test]
    fn scrobbling_needs_network_and_secrets() {
        let (core, _) = core();
        assert!(!core.capabilities().scrobbling);
        assert_eq!(
            core.dispatch(Command::LinkScrobbAccount {
                device_name: "Pixel".into()
            }),
            Err(CoreError::MissingPort("http"))
        );
    }

    /// Наблюдатель, который из колбэка сразу шлёт следующую команду.
    struct Reentrant(Mutex<Option<Arc<Core>>>);
    impl Observer for Reentrant {
        fn on_event(&self, _: Event) {
            let core = lock(&self.0).take();
            if let Some(core) = core {
                core.dispatch(Command::Next).unwrap();
            }
        }
    }

    #[test]
    fn observer_may_dispatch_from_callback() {
        let observer = Arc::new(Reentrant(Mutex::new(None)));
        let ports = Ports {
            clock: Arc::new(FixedClock),
            http: None,
            secrets: None,
            media_session: None,
        };
        let core = Arc::new(Core::new(ports, observer.clone()));
        *lock(&observer.0) = Some(core.clone());
        core.dispatch(Command::PlayQueue {
            tracks: vec![TrackId(1), TrackId(2)],
            start: 0,
        })
        .unwrap();
        assert_eq!(core.state().queue_position, Some((1, 2)));
    }
}
