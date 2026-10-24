//! Связь со Scrobb из потока истории: привязка устройства (RFC 8628,
//! `docs/architecture.md` §18.4), «сейчас играет» (§18.7) и отправка
//! очереди засчитанного (§18.8).
//!
//! Протокол строит и разбирает `scrobb-client`, запросы выполняет порт
//! `HttpTransport`, токен лежит только в хранилище секретов ОС.

use std::sync::Arc;

use player_history::{
    DeliveryState, HistoryStore, PlayId, Settlement, TARGET_SCROBB, TrackSnapshot, now_playing,
    submission,
};
use player_ports::{Clock, HttpTransport, SecureStore};
use scrobb_api::{DeviceToken, ListenStatus, MAX_LISTENS_PER_BATCH};
use scrobb_client::retry::{Backoff, SendStatus};
use scrobb_client::{ApiError, DevicePoll, ScrobbClient};
use scrobb_rules::{NowPlayingAction, NowPlayingGate};

use crate::{Event, Observer, ScrobbleState, ScrobbleStatus};

/// Ключ токена устройства Scrobb в хранилище секретов.
pub const SCROBB_TOKEN_KEY: &str = "scrobb.device_token";
/// Как часто пробовать отправить очередь (§18.8).
const SEND_INTERVAL_MS: u64 = 30_000;
/// Интервал опроса привязки, если сервер его не прислал (RFC 8628, §3.2).
const DEFAULT_POLL_MS: u64 = 5_000;
/// На `slow_down` интервал опроса растёт на 5 с (RFC 8628, §3.5).
const SLOW_DOWN_MS: u64 = 5_000;

const LINK_EXPIRED: &str = "Код привязки истёк — начните привязку заново";
const LINK_DENIED: &str = "Привязка отклонена на сайте Scrobb";
const UPGRADE: &str = "Версия плеера устарела — обновите приложение";

/// Адаптеры и клиент для связи со Scrobb.
pub(crate) struct ScrobbPorts {
    pub client: ScrobbClient,
    pub http: Arc<dyn HttpTransport>,
    pub secrets: Arc<dyn SecureStore>,
}

pub(crate) struct Remote {
    ports: ScrobbPorts,
    store: Arc<dyn HistoryStore>,
    clock: Arc<dyn Clock>,
    observer: Arc<dyn Observer>,
    backoff: Backoff,
    /// Токен, на который сервер ответил `401`: пока он не сменится, к API не обращаемся.
    revoked: Option<String>,
    linked: bool,
    pending: usize,
    next_send_ms: u64,
    reported: Option<ScrobbleStatus>,
    linking: Option<Linking>,
    gate: NowPlayingGate,
    /// Что сейчас звучит — для «сейчас играет».
    playing: Option<TrackSnapshot>,
    paused: bool,
}

/// Идёт привязка: ждём, пока пользователь подтвердит код на сайте.
struct Linking {
    device_code: String,
    interval_ms: u64,
    next_poll_ms: u64,
    expires_at_ms: u64,
}

/// Что делать после опроса привязки.
enum LinkStep {
    Wait { interval_ms: u64 },
    Fail(&'static str),
    Linked(DeviceToken),
}

impl Remote {
    pub fn new(
        ports: ScrobbPorts,
        store: Arc<dyn HistoryStore>,
        clock: Arc<dyn Clock>,
        observer: Arc<dyn Observer>,
    ) -> Self {
        Self {
            ports,
            store,
            clock,
            observer,
            backoff: Backoff::default(),
            revoked: None,
            linked: false,
            pending: 0,
            next_send_ms: 0,
            reported: None,
            linking: None,
            gate: NowPlayingGate::default(),
            playing: None,
            paused: false,
        }
    }

    /// Привязано ли устройство: засчитанное ставится в очередь, только если да.
    pub fn is_linked(&self) -> bool {
        self.token().is_some()
    }

    /// Ближайший момент, когда нужно что-то сделать без новых сообщений.
    pub fn deadline(&self) -> Option<u64> {
        let link = self
            .linking
            .as_ref()
            .map(|l| l.next_poll_ms.min(l.expires_at_ms));
        // Отправлять есть что — пробуем по сроку. Отозванный токен проверяем
        // раз в интервал: вдруг устройство привязали заново.
        let send = match self.backoff.status() {
            _ if self.pending == 0 => None,
            SendStatus::UpgradeRequired => None,
            SendStatus::Waiting { until_ms } => Some(until_ms.max(self.next_send_ms)),
            SendStatus::Ready | SendStatus::Unauthorized => Some(self.next_send_ms),
        };
        [link, send, self.gate.next_poll_ms()]
            .into_iter()
            .flatten()
            .min()
    }

    pub fn tick(&mut self) {
        let now = self.clock.monotonic_ms();
        if self.linking.as_ref().is_some_and(|l| now >= l.next_poll_ms) {
            self.poll_link();
        }
        let action = self.gate.poll(now);
        self.send_now_playing(action, None);
        if now >= self.next_send_ms {
            self.deliver();
        }
    }

    // --- Привязка устройства ---

    /// Начать привязку: получить код и показать его пользователю (§18.4, шаги 1–2).
    pub fn link(&mut self, device_name: &str) {
        let now = self.clock.monotonic_ms();
        let client = &self.ports.client;
        let answer = match self.ports.http.send(&client.device_authorize(device_name)) {
            Ok(resp) => client.parse_device_authorize(&resp).map_err(|e| match e {
                ApiError::UpgradeRequired => UPGRADE.to_owned(),
                other => format!("Привязка: сервер Scrobb ответил ошибкой ({other:?})"),
            }),
            Err(_) => Err("Привязка: нет связи с Scrobb".to_owned()),
        };
        match answer {
            Ok(auth) => {
                let interval_ms = match auth.interval {
                    0 => DEFAULT_POLL_MS,
                    s => u64::from(s) * 1000,
                };
                self.linking = Some(Linking {
                    device_code: auth.device_code,
                    interval_ms,
                    next_poll_ms: now + interval_ms,
                    expires_at_ms: now + u64::from(auth.expires_in) * 1000,
                });
                self.observer.on_event(Event::ScrobbLinkCode {
                    user_code: auth.user_code,
                    verification_uri_complete: auth.verification_uri_complete,
                });
            }
            Err(message) => {
                self.linking = None;
                self.observer.on_event(Event::Error(message));
            }
        }
        self.publish();
    }

    /// Пользователь закрыл окно привязки.
    pub fn cancel_link(&mut self) {
        if self.linking.take().is_some() {
            self.publish();
        }
    }

    /// Опросить сервер: подтвердил ли пользователь код (§18.4, шаг 4).
    fn poll_link(&mut self) {
        let now = self.clock.monotonic_ms();
        let Some(linking) = &self.linking else {
            return;
        };
        let client = &self.ports.client;
        let step = if now >= linking.expires_at_ms {
            LinkStep::Fail(LINK_EXPIRED)
        } else {
            let wait = LinkStep::Wait {
                interval_ms: linking.interval_ms,
            };
            match self
                .ports
                .http
                .send(&client.device_token(&linking.device_code))
            {
                // Нет сети или сбой сервера — пробуем дальше, пока код жив.
                Err(_) => wait,
                Ok(resp) => match client.parse_device_token(&resp) {
                    Ok(DevicePoll::Pending) => wait,
                    Ok(DevicePoll::SlowDown) => LinkStep::Wait {
                        interval_ms: linking.interval_ms + SLOW_DOWN_MS,
                    },
                    Ok(DevicePoll::Denied) => LinkStep::Fail(LINK_DENIED),
                    Ok(DevicePoll::Expired) => LinkStep::Fail(LINK_EXPIRED),
                    Ok(DevicePoll::Linked(token)) => LinkStep::Linked(token),
                    Err(ApiError::UpgradeRequired) => LinkStep::Fail(UPGRADE),
                    Err(_) => wait,
                },
            }
        };
        match step {
            LinkStep::Wait { interval_ms } => {
                if let Some(l) = self.linking.as_mut() {
                    l.interval_ms = interval_ms;
                    l.next_poll_ms = now + interval_ms;
                }
            }
            LinkStep::Fail(message) => {
                self.linking = None;
                self.observer.on_event(Event::Error(message.to_owned()));
                self.publish();
            }
            LinkStep::Linked(token) => self.linked_with(&token),
        }
    }

    /// Пользователь подтвердил код: сохранить токен и начать отправку.
    fn linked_with(&mut self, token: &DeviceToken) {
        self.linking = None;
        if let Err(e) = self
            .ports
            .secrets
            .set(SCROBB_TOKEN_KEY, &token.access_token)
        {
            self.observer.on_event(Event::Error(format!(
                "Привязка: не удалось сохранить токен в хранилище секретов ({e:?})"
            )));
            self.publish();
            return;
        }
        self.backoff.on_success();
        self.revoked = None;
        // Трек уже звучит — сообщить о нём сразу, не дожидаясь следующего.
        if self.playing.is_some() && !self.paused {
            let action = self.gate.on_play(self.clock.monotonic_ms());
            self.send_now_playing(action, None);
        }
        self.deliver();
    }

    /// Отвязать устройство: снять «сейчас играет», отозвать токен на сервере
    /// и удалить его локально. Очередь сохраняется до следующей привязки.
    pub fn unlink(&mut self) {
        self.linking = None;
        if let Some(token) = self.token() {
            let action = self.gate.on_stop();
            self.send_now_playing(action, None);
            let client = self.ports.client.clone().with_token(&token);
            let revoked = match self.ports.http.send(&client.sign_out()) {
                Ok(resp) => matches!(
                    client.parse_empty(&resp),
                    Ok(()) | Err(ApiError::Unauthorized)
                ),
                Err(_) => false,
            };
            if !revoked {
                self.observer.on_event(Event::Error(
                    "Устройство отвязано, но сервер Scrobb не ответил: отзовите его в «Приложениях» на сайте"
                        .to_owned(),
                ));
            }
        }
        if let Err(e) = self.ports.secrets.delete(SCROBB_TOKEN_KEY) {
            self.observer.on_event(Event::Error(format!(
                "Не удалось удалить токен из хранилища секретов ({e:?})"
            )));
        }
        self.backoff.on_success();
        self.revoked = None;
        self.linked = self.token().is_some();
        self.publish();
    }

    // --- «Сейчас играет» ---

    // Время — из самого факта (`at_ms`, монотонные часы), а не момент обработки.

    pub fn on_started(&mut self, track: TrackSnapshot, at_ms: u64) {
        self.playing = Some(track);
        self.paused = false;
        let action = self.gate.on_play(at_ms);
        self.send_now_playing(action, Some(0));
    }

    pub fn on_paused(&mut self, at_ms: u64) {
        self.paused = true;
        self.gate.on_pause(at_ms);
    }

    pub fn on_resumed(&mut self, position_ms: Option<u64>, at_ms: u64) {
        if self.playing.is_some() && self.paused {
            self.paused = false;
            let action = self.gate.on_play(at_ms);
            self.send_now_playing(action, position_ms);
        }
    }

    pub fn on_seeked(&mut self, position_ms: u64, at_ms: u64) {
        if self.playing.is_some() {
            let action = self.gate.on_change(at_ms);
            self.send_now_playing(action, Some(position_ms));
        }
    }

    pub fn on_stopped(&mut self) {
        let action = self.gate.on_stop();
        self.send_now_playing(action, None);
        self.playing = None;
        self.paused = false;
    }

    /// «Сейчас играет» не повторяется при ошибках: через минуту оно уже неважно.
    /// Но ответы `401` и `426` касаются всех запросов и останавливают отправку.
    fn send_now_playing(&mut self, action: Option<NowPlayingAction>, position_ms: Option<u64>) {
        let Some(action) = action else {
            return;
        };
        let Some(token) = self.usable_token() else {
            return;
        };
        let client = self.ports.client.clone().with_token(&token);
        let request = match (action, &self.playing) {
            (NowPlayingAction::Send, Some(track)) => {
                client.now_playing(&now_playing(track, position_ms))
            }
            (NowPlayingAction::Send, None) => return,
            (NowPlayingAction::Clear, _) => client.clear_now_playing(),
        };
        if let Ok(resp) = self.ports.http.send(&request)
            && let Err(e @ (ApiError::Unauthorized | ApiError::UpgradeRequired)) =
                client.parse_empty(&resp)
        {
            self.stop_sending(&e, token);
            self.publish();
        }
    }

    // --- Очередь засчитанного ---

    /// Отправить очередь пачками, пока сервис принимает (§18.8).
    pub fn deliver(&mut self) {
        let now = self.clock.monotonic_ms();
        let unix_now = self.clock.unix_ms();
        self.next_send_ms = now + SEND_INTERVAL_MS;
        let mut errors = Vec::new();
        if let Some(token) = self.usable_token().filter(|_| self.backoff.ready(now)) {
            match self.store.expire(TARGET_SCROBB, unix_now) {
                Ok(0) => {}
                Ok(n) => errors.push(format!(
                    "{n} прослушиваний не отправлены: очередь хранит до 14 дней и 10 000 записей"
                )),
                Err(e) => errors.push(e.to_string()),
            }
            let client = self.ports.client.clone().with_token(&token);
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
                let response = match self.ports.http.send(&request) {
                    Ok(response) => response,
                    Err(_) => {
                        self.backoff.on_network_error(now, jitter());
                        break;
                    }
                };
                let results = match client.parse_submit_listens(&response) {
                    Ok(answer) => answer.results,
                    Err(e) => {
                        self.stop_sending(&e, token);
                        break;
                    }
                };
                self.backoff.on_success();
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
            Ok(n) => self.pending = n,
            Err(e) => errors.push(e.to_string()),
        }
        for message in errors {
            self.observer.on_event(Event::Error(message));
        }
        self.publish();
    }

    // --- Общее ---

    fn token(&self) -> Option<String> {
        self.ports
            .secrets
            .get(SCROBB_TOKEN_KEY)
            .ok()
            .flatten()
            .filter(|t| !t.is_empty())
    }

    /// Токен, с которым можно обращаться к API. Заодно обновляет «привязано ли»
    /// и снимает остановку после `401`, если токен уже другой.
    fn usable_token(&mut self) -> Option<String> {
        let token = self.token();
        self.linked = token.is_some();
        let token = token?;
        if self.backoff.status() == SendStatus::Unauthorized
            && self.revoked.as_ref() != Some(&token)
        {
            self.backoff.on_success();
            self.revoked = None;
        }
        match self.backoff.status() {
            SendStatus::Unauthorized | SendStatus::UpgradeRequired => None,
            _ => Some(token),
        }
    }

    /// Ошибка API: повтор позже или остановка до действия пользователя.
    fn stop_sending(&mut self, error: &ApiError, token: String) {
        let now = self.clock.monotonic_ms();
        if self.backoff.on_error(error, now, jitter()) == SendStatus::Unauthorized {
            self.revoked = Some(token);
        }
    }

    fn status(&self) -> ScrobbleStatus {
        let state = if self.linking.is_some() {
            ScrobbleState::Linking
        } else if !self.linked {
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

    /// Сообщить оболочке состояние, если оно изменилось.
    fn publish(&mut self) {
        let status = self.status();
        if self.reported != Some(status) {
            self.reported = Some(status);
            self.observer.on_event(Event::Scrobbling(status));
        }
    }
}

/// Случайное число из `[0, 1)` для разброса повторов.
fn jitter() -> f64 {
    getrandom::u64().map_or(0.5, |r| (r >> 11) as f64 / (1u64 << 53) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_is_a_fraction() {
        assert!((0..100).all(|_| (0.0..1.0).contains(&jitter())));
    }
}
