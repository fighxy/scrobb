//! Поток истории: по фактам от фасада («зазвучал», «пауза», «закончился так-то»)
//! ведёт счётчик прослушивания, пишет историю и отправляет засчитанное в Scrobb
//! (`docs/player-history.md`, `docs/architecture.md` §18.7–§18.8).
//!
//! Таймеры порога, контрольных точек и повторов отправки — в одном цикле
//! `recv_timeout`: любое изменение приходит сообщением, поэтому срок ожидания
//! всегда пересчитывается вовремя. Сеть блокирует только этот поток; факты,
//! пришедшие за это время, несут своё время и учитываются точно.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use player_domain::{Track, TrackId};
use player_history::{
    DeliveryState, HistoryError, HistoryStore, NewPlay, Outcome, PlayId, Settlement, TARGET_SCROBB,
    TrackSnapshot, submission,
};
use player_ports::{Clock, HttpTransport, SecureStore};
use scrobb_api::{ListenStatus, MAX_LISTENS_PER_BATCH, Thresholds};
use scrobb_client::ScrobbClient;
use scrobb_client::retry::{Backoff, SendStatus};
use scrobb_rules::ListenTracker;
use uuid::{ContextV7, Timestamp, Uuid};

use crate::{Event, Observer, ScrobbleState, ScrobbleStatus};

/// Ключ токена устройства Scrobb в хранилище секретов.
pub const SCROBB_TOKEN_KEY: &str = "scrobb.device_token";
/// Как часто сохранять время звучания незакрытой записи: столько теряет падение.
const CHECKPOINT_MS: u64 = 15_000;
/// Как часто пробовать отправить очередь (§18.8).
const SEND_INTERVAL_MS: u64 = 30_000;

/// Момент события: монотонное время — для счётчика, Unix — для истории.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stamp {
    pub mono_ms: u64,
    pub unix_ms: i64,
}

/// Факт от фасада.
#[derive(Debug)]
pub(crate) enum Report {
    /// Трек зазвучал; незакрытое прослушивание — дослушано (переход без паузы).
    Started {
        track: Box<Track>,
        at: Stamp,
    },
    Paused {
        at: Stamp,
    },
    Resumed {
        at: Stamp,
    },
    /// Текущее прослушивание закончилось.
    Ended {
        outcome: Outcome,
        at: Stamp,
    },
    /// Ошибка чтения трека: закрыть прослушивание, если звучал он.
    Failed {
        track: TrackId,
        at: Stamp,
    },
}

/// Куда и как отправлять засчитанное.
pub(crate) struct Delivery {
    pub client: ScrobbClient,
    pub http: Arc<dyn HttpTransport>,
    pub secrets: Arc<dyn SecureStore>,
}

pub(crate) struct HistoryService {
    store: Arc<dyn HistoryStore>,
    clock: Arc<dyn Clock>,
    observer: Arc<dyn Observer>,
    thresholds: Thresholds,
    listening: Option<Listening>,
    sender: Option<Sender>,
    /// Счётчик UUIDv7: id прослушиваний устройства растут даже в пределах миллисекунды.
    ids: ContextV7,
}

struct Listening {
    id: PlayId,
    track: TrackId,
    tracker: ListenTracker,
    paused: bool,
    /// Время звучания на последней контрольной точке.
    checkpointed_ms: u64,
}

/// Состояние отправки в Scrobb.
struct Sender {
    delivery: Delivery,
    backoff: Backoff,
    /// Токен, на который сервер ответил `401`: пока он не сменится, не отправляем.
    revoked: Option<String>,
    linked: bool,
    pending: usize,
    next_send_ms: u64,
    reported: Option<ScrobbleStatus>,
}

impl HistoryService {
    pub fn new(
        store: Arc<dyn HistoryStore>,
        clock: Arc<dyn Clock>,
        observer: Arc<dyn Observer>,
        thresholds: Thresholds,
        delivery: Option<Delivery>,
    ) -> Self {
        let sender = delivery.map(|delivery| Sender {
            delivery,
            backoff: Backoff::default(),
            revoked: None,
            linked: false,
            pending: 0,
            next_send_ms: 0,
            reported: None,
        });
        Self {
            store,
            clock,
            observer,
            thresholds,
            listening: None,
            sender,
            ids: ContextV7::new(),
        }
    }

    pub fn spawn(self, reports: Receiver<Report>) -> JoinHandle<()> {
        thread::Builder::new()
            .name("player-core-history".into())
            .spawn(move || self.run(reports))
            .expect("поток истории запускается")
    }

    fn run(mut self, reports: Receiver<Report>) {
        let recovered = self.store.recover_interrupted();
        self.check(recovered);
        self.deliver();
        loop {
            let received = match self.next_deadline() {
                Some(at) => {
                    let wait = at.saturating_sub(self.clock.monotonic_ms());
                    reports.recv_timeout(Duration::from_millis(wait))
                }
                None => reports.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(report) => self.on_report(report),
                Err(RecvTimeoutError::Timeout) => self.tick(),
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        // Ядро закрыто, а запись открыта (фасад обычно закрывает её сам).
        let at = self.stamp();
        self.end(Outcome::Stopped, at);
    }

    fn stamp(&self) -> Stamp {
        Stamp {
            mono_ms: self.clock.monotonic_ms(),
            unix_ms: self.clock.unix_ms(),
        }
    }

    fn on_report(&mut self, report: Report) {
        match report {
            Report::Started { track, at } => {
                self.end(Outcome::Finished, at);
                self.begin(&track, at);
            }
            Report::Paused { at } => {
                if let Some(mut l) = self.listening.take() {
                    if !l.paused {
                        l.tracker.pause(at.mono_ms);
                        l.paused = true;
                        self.poll(&mut l, at.mono_ms, true);
                    }
                    self.listening = Some(l);
                }
            }
            Report::Resumed { at } => {
                if let Some(l) = self.listening.as_mut().filter(|l| l.paused) {
                    l.tracker.resume(at.mono_ms);
                    l.paused = false;
                }
            }
            Report::Ended { outcome, at } => self.end(outcome, at),
            Report::Failed { track, at } => {
                if self.listening.as_ref().is_some_and(|l| l.track == track) {
                    self.end(Outcome::Error, at);
                }
            }
        }
    }

    fn begin(&mut self, track: &Track, at: Stamp) {
        let play = NewPlay {
            id: new_play_id(&self.ids, at.unix_ms),
            started_at_ms: at.unix_ms,
            track: TrackSnapshot::from(track),
            track_id: Some(track.id),
        };
        let begun = self.store.begin(&play);
        if self.check(begun).is_none() {
            return;
        }
        self.listening = Some(Listening {
            id: play.id,
            track: track.id,
            tracker: ListenTracker::start(
                track.duration_ms,
                at.unix_ms.div_euclid(1000),
                at.mono_ms,
                &self.thresholds,
            ),
            paused: false,
            checkpointed_ms: 0,
        });
    }

    fn end(&mut self, outcome: Outcome, at: Stamp) {
        let Some(mut l) = self.listening.take() else {
            return;
        };
        l.tracker.stop(at.mono_ms);
        self.poll(&mut l, at.mono_ms, false);
        let played_ms = l.tracker.played_ms(at.mono_ms);
        let finished = self.store.finish(&l.id, at.unix_ms, played_ms, outcome);
        self.check(finished);
    }

    /// Проверить порог и, если пора, сохранить контрольную точку.
    fn poll(&mut self, l: &mut Listening, now_ms: u64, checkpoint: bool) {
        if let Some(scrobble) = l.tracker.poll(now_ms) {
            self.scrobbled(&l.id, scrobble.played_ms);
        }
        let played_ms = l.tracker.played_ms(now_ms);
        if checkpoint || played_ms >= l.checkpointed_ms + CHECKPOINT_MS {
            let saved = self.store.checkpoint(&l.id, played_ms);
            self.check(saved);
            l.checkpointed_ms = played_ms;
        }
    }

    /// Порог пройден: отметить и, если устройство привязано, поставить в очередь и отправить.
    fn scrobbled(&mut self, id: &PlayId, played_ms: u64) {
        let linked = self.sender.as_ref().is_some_and(|s| s.token().is_some());
        let targets: &[&str] = if linked { &[TARGET_SCROBB] } else { &[] };
        let marked = self
            .store
            .mark_scrobbled(id, played_ms, targets, self.clock.unix_ms());
        if self.check(marked).is_some() && linked {
            self.deliver();
        }
    }

    fn tick(&mut self) {
        let now = self.clock.monotonic_ms();
        if let Some(mut l) = self.listening.take() {
            if !l.paused {
                self.poll(&mut l, now, false);
            }
            self.listening = Some(l);
        }
        if self.sender.as_ref().is_some_and(|s| now >= s.next_send_ms) {
            self.deliver();
        }
    }

    /// Ближайший момент, когда нужно что-то сделать без новых фактов.
    fn next_deadline(&self) -> Option<u64> {
        let now = self.clock.monotonic_ms();
        let listening = self.listening.as_ref().filter(|l| !l.paused).map(|l| {
            let played = l.tracker.played_ms(now);
            let checkpoint = (l.checkpointed_ms + CHECKPOINT_MS).saturating_sub(played);
            let threshold = l.tracker.remaining_ms(now).unwrap_or(u64::MAX);
            now.saturating_add(checkpoint.min(threshold))
        });
        let sending = self.sender.as_ref().and_then(Sender::deadline);
        match (listening, sending) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Отправить очередь пачками, пока сервис принимает (§18.8).
    fn deliver(&mut self) {
        let now = self.clock.monotonic_ms();
        let unix_now = self.clock.unix_ms();
        let Some(sender) = self.sender.as_mut() else {
            return;
        };
        sender.next_send_ms = now + SEND_INTERVAL_MS;
        let token = sender.token();
        sender.linked = token.is_some();
        if let Some(token) = &token {
            // Новый токен после «войдите заново» — снова можно отправлять.
            if sender.backoff.status() == SendStatus::Unauthorized
                && sender.revoked.as_ref() != Some(token)
            {
                sender.backoff.on_success();
                sender.revoked = None;
            }
        }
        let mut errors = Vec::new();
        if let Some(token) = token.filter(|_| sender.backoff.ready(now)) {
            match self.store.expire(TARGET_SCROBB, unix_now) {
                Ok(0) => {}
                Ok(n) => errors.push(format!(
                    "{n} прослушиваний не отправлены: очередь хранит до 14 дней и 10 000 записей"
                )),
                Err(e) => errors.push(e.to_string()),
            }
            let client = sender.delivery.client.clone().with_token(&token);
            loop {
                let batch = match self.store.pending(TARGET_SCROBB, MAX_LISTENS_PER_BATCH) {
                    Ok(batch) if !batch.is_empty() => batch,
                    Ok(_) => break,
                    Err(e) => {
                        errors.push(e.to_string());
                        break;
                    }
                };
                let listens: Vec<_> = batch.iter().map(submission).collect();
                let request = match client.submit_listens(&listens) {
                    Ok(request) => request,
                    Err(e) => {
                        errors.push(format!("{e:?}"));
                        break;
                    }
                };
                let jitter = jitter();
                let response = match sender.delivery.http.send(&request) {
                    Ok(response) => response,
                    Err(_) => {
                        sender.backoff.on_network_error(now, jitter);
                        break;
                    }
                };
                let results = match client.parse_submit_listens(&response) {
                    Ok(answer) => answer.results,
                    Err(e) => {
                        if sender.backoff.on_error(&e, now, jitter) == SendStatus::Unauthorized {
                            sender.revoked = Some(token.clone());
                        }
                        break;
                    }
                };
                sender.backoff.on_success();
                let settled: Vec<_> = results
                    .into_iter()
                    .filter(|r| batch.iter().any(|p| p.id.0 == r.client_id))
                    .map(|r| Settlement {
                        id: PlayId(r.client_id),
                        state: match r.status {
                            ListenStatus::Accepted => DeliveryState::Accepted,
                            ListenStatus::Duplicate => DeliveryState::Duplicate,
                            ListenStatus::Rejected => DeliveryState::Rejected,
                        },
                        reason: r.reason,
                    })
                    .collect();
                if let Err(e) = self.store.settle(TARGET_SCROBB, &settled, unix_now) {
                    errors.push(e.to_string());
                    break;
                }
                // Неполный ответ: остальное — при следующей попытке, без зацикливания.
                if settled.is_empty() || batch.len() < MAX_LISTENS_PER_BATCH {
                    break;
                }
            }
        }
        match self.store.pending_count(TARGET_SCROBB) {
            Ok(n) => sender.pending = n,
            Err(e) => errors.push(e.to_string()),
        }
        let status = sender.status();
        let changed = sender.reported != Some(status);
        sender.reported = Some(status);
        for message in errors {
            self.observer.on_event(Event::Error(message));
        }
        if changed {
            self.observer.on_event(Event::Scrobbling(status));
        }
    }

    /// Сообщить об ошибке хранилища; `None` — была ошибка.
    fn check<T>(&self, result: Result<T, HistoryError>) -> Option<T> {
        result
            .map_err(|e| self.observer.on_event(Event::Error(e.to_string())))
            .ok()
    }
}

impl Sender {
    fn token(&self) -> Option<String> {
        self.delivery
            .secrets
            .get(SCROBB_TOKEN_KEY)
            .ok()
            .flatten()
            .filter(|t| !t.is_empty())
    }

    fn status(&self) -> ScrobbleStatus {
        let state = if !self.linked {
            ScrobbleState::NotLinked
        } else {
            match self.backoff.status() {
                SendStatus::Ready => ScrobbleState::Ready,
                SendStatus::Waiting { .. } => ScrobbleState::Retrying,
                SendStatus::Unauthorized => ScrobbleState::Unauthorized,
                SendStatus::UpgradeRequired => ScrobbleState::UpgradeRequired,
            }
        };
        ScrobbleStatus {
            pending: self.pending,
            state,
        }
    }

    /// Когда снова пробовать: только если есть что отправлять. Отозванный токен
    /// проверяем раз в интервал — вдруг устройство привязали заново.
    fn deadline(&self) -> Option<u64> {
        if self.pending == 0 {
            return None;
        }
        match self.backoff.status() {
            SendStatus::UpgradeRequired => None,
            SendStatus::Waiting { until_ms } => Some(until_ms.max(self.next_send_ms)),
            SendStatus::Ready | SendStatus::Unauthorized => Some(self.next_send_ms),
        }
    }
}

/// UUIDv7: время начала, счётчик и случайные биты; уникален без согласования
/// между устройствами, а на одном устройстве растёт в порядке создания.
fn new_play_id(ids: &ContextV7, unix_ms: i64) -> PlayId {
    let ms = unix_ms.max(0) as u64;
    let ts = Timestamp::from_unix(ids, ms / 1000, (ms % 1000) as u32 * 1_000_000);
    PlayId(Uuid::new_v7(ts).to_string())
}

/// Случайное число из `[0, 1)` для разброса повторов.
fn jitter() -> f64 {
    getrandom::u64().map_or(0.5, |r| (r >> 11) as f64 / (1u64 << 53) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_ids_are_v7_and_sort_by_creation() {
        let ids = ContextV7::new();
        let a = new_play_id(&ids, 1_760_000_000_000);
        let b = new_play_id(&ids, 1_760_000_000_000);
        let c = new_play_id(&ids, 1_760_000_000_001);
        assert_eq!(Uuid::parse_str(&a.0).unwrap().get_version_num(), 7);
        assert!(a < b && b < c, "в одну миллисекунду — тоже по порядку");
        let other_device = ContextV7::new();
        assert_ne!(
            new_play_id(&other_device, 5),
            new_play_id(&ContextV7::new(), 5)
        );
        assert!((0.0..1.0).contains(&jitter()));
    }
}
