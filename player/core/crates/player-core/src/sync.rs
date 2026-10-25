//! Синхронизация плейлистов через сервер Scrobb (`docs/playlists.md` §7).
//!
//! - [`EditClock`] — гибридные часы устройства: каждая правка плейлиста
//!   получает метку, а чужие метки, пришедшие с сервера, часы догоняют.
//!   Общие для потока команд (правки) и сетевого потока (слияние).
//! - [`PlaylistSync`] — обмен с сервером в потоке истории, рядом с отправкой
//!   скробблов: один запрос отправляет неотправленное и забирает изменённое
//!   после курсора; так пачками, пока есть что отправить или получить.
//!   Курсор сдвигается только после слияния: сбой посередине — повтор той же
//!   пачки, а слияние идемпотентно.

use std::sync::{Arc, Mutex};

use player_domain::TrackSnapshot;
use player_playlists::{Edit, Hlc, PlaylistStore, Unsent};
use player_ports::{Clock, HttpTransport};
use scrobb_client::retry::{Backoff, SendStatus};
use scrobb_client::{ApiError, ScrobbClient};
use scrobb_sync::playlists::{MAX_RECORDS, SyncRequest};
use serde::{Deserialize, Serialize};

use crate::{Event, Observer, lock};

/// Как часто забирать правки с других устройств, если здесь ничего не менялось.
pub(crate) const SYNC_INTERVAL_MS: u64 = 60_000;
/// Пауза после правки: несколько правок подряд уходят одним запросом.
pub(crate) const SYNC_DEBOUNCE_MS: u64 = 1_500;
/// Сервер не умеет синхронизацию (`404`): спросить снова через час.
pub(crate) const UNSUPPORTED_RETRY_MS: u64 = 3_600_000;
/// Предел запросов за один обмен: защита от сервера, который всегда «ещё есть».
const MAX_ROUNDS: usize = 1_000;

/// Состояние синхронизации плейлистов для оболочки.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistSyncStatus {
    /// Что с синхронизацией.
    pub state: PlaylistSyncState,
    /// Последний удачный обмен, миллисекунды Unix UTC.
    pub last_synced_ms: Option<i64>,
}

/// Что с синхронизацией плейлистов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistSyncState {
    /// Устройство не привязано к аккаунту: плейлисты только здесь.
    NotLinked,
    /// Идёт обмен с сервером.
    Syncing,
    /// Всё отправлено и получено.
    Synced,
    /// Сеть или сервер недоступны — повтор позже; правки копятся.
    Retrying,
    /// Токен отозван: нужно войти заново.
    Unauthorized,
    /// Версия плеера устарела.
    UpgradeRequired,
    /// Сервер Scrobb пока не умеет синхронизацию плейлистов.
    Unsupported,
}

/// Гибридные часы правок этого устройства.
pub(crate) struct EditClock {
    hlc: Mutex<Hlc>,
    clock: Arc<dyn Clock>,
}

impl EditClock {
    /// Часы, продолжающие с самой поздней метки хранилища: после перезапуска
    /// (и с отставшими системными часами) правки не уходят в прошлое. Узел
    /// назначается при первом запуске и хранится вместе с плейлистами.
    pub fn start(store: &dyn PlaylistStore, clock: Arc<dyn Clock>) -> (Self, Option<String>) {
        let mut problem = None;
        let state = store.sync_state().unwrap_or_else(|e| {
            problem = Some(e.0);
            Default::default()
        });
        let node = match state.node {
            Some(node) => node,
            None => {
                let node = new_node();
                if let Err(e) = store.set_node(node) {
                    problem = Some(e.0);
                }
                node
            }
        };
        let mut hlc = Hlc::new(node);
        if let Some(latest) = state.latest {
            hlc = hlc.observe(latest, 0);
        }
        let clock = Self {
            hlc: Mutex::new(hlc),
            clock,
        };
        (clock, problem)
    }

    /// Метка новой правки.
    pub fn edit(&self) -> Edit {
        let at_ms = self.clock.unix_ms();
        let mut hlc = lock(&self.hlc);
        *hlc = hlc.tick(at_ms.max(0) as u64);
        Edit { at_ms, hlc: *hlc }
    }

    /// Увидели чужую метку: следующая правка будет позже неё.
    pub fn observe(&self, remote: Hlc) {
        let now = self.clock.unix_ms().max(0) as u64;
        let mut hlc = lock(&self.hlc);
        *hlc = hlc.observe(remote, now);
    }
}

/// Узел часов: случайный, не 0 (0 — метки плейлистов, сохранённых до синхронизации).
fn new_node() -> u32 {
    loop {
        match getrandom::u32() {
            Ok(0) => continue,
            Ok(n) => return n,
            // Без источника случайности — хотя бы время: узлы разных устройств
            // всё равно почти наверняка разные.
            Err(_) => return (std::process::id() ^ 0x5c0b_b000).max(1),
        }
    }
}

/// Обмен с сервером остановлен до действия пользователя: решает связь со Scrobb.
pub(crate) type Blocked = ApiError;

pub(crate) struct PlaylistSync {
    store: Arc<dyn PlaylistStore>,
    edits: Arc<EditClock>,
    clock: Arc<dyn Clock>,
    observer: Arc<dyn Observer>,
    backoff: Backoff,
    /// Когда обмениваться в следующий раз (монотонное время).
    next_ms: u64,
    state: PlaylistSyncState,
    last_synced_ms: Option<i64>,
    reported: Option<PlaylistSyncStatus>,
}

impl PlaylistSync {
    pub fn new(
        store: Arc<dyn PlaylistStore>,
        edits: Arc<EditClock>,
        clock: Arc<dyn Clock>,
        observer: Arc<dyn Observer>,
    ) -> Self {
        Self {
            store,
            edits,
            clock,
            observer,
            backoff: Backoff::default(),
            next_ms: 0,
            state: PlaylistSyncState::NotLinked,
            last_synced_ms: None,
            reported: None,
        }
    }

    /// Когда обмениваться в следующий раз.
    pub fn deadline(&self) -> u64 {
        match self.backoff.status() {
            SendStatus::Waiting { until_ms } => until_ms.max(self.next_ms),
            _ => self.next_ms,
        }
    }

    /// Здесь правили плейлисты в `at_ms`: отправить вскоре, одним запросом
    /// за несколько правок подряд.
    pub fn edited(&mut self, at_ms: u64) {
        self.next_ms = self.next_ms.min(at_ms + SYNC_DEBOUNCE_MS);
    }

    /// Пользователь попросил синхронизировать сейчас: без ожидания повтора.
    pub fn now(&mut self) {
        self.backoff.on_success();
        self.next_ms = self.clock.monotonic_ms();
        // Попросили — ответить состоянием, даже если оно не изменилось.
        self.reported = None;
    }

    /// Привязан другой (или тот же) аккаунт: начать с нуля — отправить всё
    /// местное и получить всё с сервера.
    pub fn restart(&mut self) {
        if let Err(e) = self.store.reset_sync() {
            self.observer.on_event(Event::Error(e.0));
        }
        self.backoff.on_success();
        self.last_synced_ms = None;
        self.next_ms = self.clock.monotonic_ms();
    }

    /// Обменяться с сервером. `Err` — токен отозван или версия устарела:
    /// это касается всей связи со Scrobb, решает вызывающий.
    pub fn run(&mut self, client: &ScrobbClient, http: &dyn HttpTransport) -> Result<(), Blocked> {
        let now = self.clock.monotonic_ms();
        self.next_ms = now + SYNC_INTERVAL_MS;
        if !self.backoff.ready(now) {
            return Ok(());
        }
        self.state = PlaylistSyncState::Syncing;
        self.publish(None);
        let mut changed = 0;
        let outcome = self.exchange(client, http, &mut changed);
        if changed > 0 {
            self.observer.on_event(Event::PlaylistsChanged);
        }
        match outcome {
            Ok(()) => {
                self.backoff.on_success();
                self.state = PlaylistSyncState::Synced;
                self.last_synced_ms = Some(self.clock.unix_ms());
            }
            Err(Failure::Network) => {
                self.backoff.on_network_error(now, crate::scrobb::jitter());
                self.state = PlaylistSyncState::Retrying;
            }
            Err(Failure::Unsupported) => {
                self.next_ms = now + UNSUPPORTED_RETRY_MS;
                self.state = PlaylistSyncState::Unsupported;
            }
            Err(Failure::Api(e @ (ApiError::Unauthorized | ApiError::UpgradeRequired))) => {
                return Err(e);
            }
            Err(Failure::Api(e)) => {
                self.backoff.on_error(&e, now, crate::scrobb::jitter());
                self.state = PlaylistSyncState::Retrying;
            }
            Err(Failure::Store(message)) => {
                self.observer
                    .on_event(Event::Error(format!("Синхронизация плейлистов: {message}")));
                self.state = PlaylistSyncState::Retrying;
            }
        }
        self.publish(None);
        Ok(())
    }

    /// Запросы, пока есть что отправить или получить.
    fn exchange(
        &mut self,
        client: &ScrobbClient,
        http: &dyn HttpTransport,
        changed: &mut usize,
    ) -> Result<(), Failure> {
        let store = |e: player_playlists::PlaylistError| Failure::Store(e.0);
        for _ in 0..MAX_ROUNDS {
            let cursor = self.store.sync_state().map_err(store)?.cursor;
            let unsent: Unsent = self.store.unsent(MAX_RECORDS).map_err(store)?;
            let full_page = unsent.items.len() >= MAX_RECORDS;
            let request = SyncRequest {
                cursor,
                limit: MAX_RECORDS as u32,
                playlists: unsent.playlists.iter().map(|(r, _)| r.clone()).collect(),
                items: unsent.items.iter().map(|(r, _)| r.clone()).collect(),
            };
            let response = http
                .send(&client.sync_playlists(&request))
                .map_err(|_| Failure::Network)?;
            let answer = match client.parse_sync_playlists::<TrackSnapshot>(&response) {
                Ok(answer) => answer,
                Err(ApiError::Unexpected(404)) => return Err(Failure::Unsupported),
                Err(e) => return Err(Failure::Api(e)),
            };
            self.store.mark_sent(&unsent).map_err(store)?;
            // Сначала догнать чужие часы: правка, сделанная здесь сразу после
            // слияния, должна быть позже пришедших.
            let latest = answer
                .playlists
                .iter()
                .map(|p| p.latest())
                .chain(answer.items.iter().map(|i| i.latest()))
                .max();
            if let Some(latest) = latest {
                self.edits.observe(latest);
            }
            let merged = self
                .store
                .merge(answer.playlists, answer.items)
                .map_err(store)?;
            *changed += merged.changed;
            self.store.set_cursor(Some(&answer.cursor)).map_err(store)?;
            if !answer.more && !full_page {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Сообщить оболочке состояние, если оно изменилось. `blocked` — почему
    /// обмен невозможен вообще (не привязано, токен отозван).
    pub fn publish(&mut self, blocked: Option<PlaylistSyncState>) {
        let status = PlaylistSyncStatus {
            state: blocked.unwrap_or(self.state),
            last_synced_ms: self.last_synced_ms,
        };
        if self.reported != Some(status) {
            self.reported = Some(status);
            self.observer.on_event(Event::PlaylistSync(status));
        }
    }
}

/// Почему обмен прервался.
enum Failure {
    Network,
    Unsupported,
    Api(ApiError),
    Store(String),
}
