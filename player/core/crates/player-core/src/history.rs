//! Поток истории: по фактам от фасада («зазвучал», «пауза», «закончился так-то»)
//! ведёт счётчик прослушивания и пишет историю (`docs/player-history.md`,
//! `docs/architecture.md` §18.7). Связь со Scrobb — привязка, «сейчас играет»
//! и отправка засчитанного — в [`crate::scrobb`] и работает в этом же потоке.
//!
//! Таймеры порога, контрольных точек, опроса привязки и повторов отправки — в
//! одном цикле `recv_timeout`: любое изменение приходит сообщением, поэтому
//! срок ожидания всегда пересчитывается вовремя. Сеть блокирует только этот
//! поток; факты, пришедшие за это время, несут своё время и учитываются точно.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};

use player_domain::{Track, TrackId};
use player_history::{
    HistoryError, HistoryStore, NewPlay, Outcome, PlayId, TARGET_SCROBB, TrackSnapshot,
};
use player_ports::Clock;
use scrobb_api::Thresholds;
use scrobb_rules::ListenTracker;
use uuid::{ContextV7, Timestamp, Uuid};

use crate::scrobb::Remote;
use crate::{Event, Observer};

/// Как часто сохранять время звучания незакрытой записи: столько теряет падение.
const CHECKPOINT_MS: u64 = 15_000;

/// Момент события: монотонное время — для счётчика, Unix — для истории.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stamp {
    pub mono_ms: u64,
    pub unix_ms: i64,
}

/// Сообщение потоку истории: факт воспроизведения или команда оболочки.
#[derive(Debug)]
pub(crate) enum Message {
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
        position_ms: Option<u64>,
    },
    /// Перемотка внутри трека: для «сейчас играет».
    Seeked {
        position_ms: u64,
        at: Stamp,
    },
    /// Текущее прослушивание закончилось, воспроизведение продолжается.
    Ended {
        outcome: Outcome,
        at: Stamp,
    },
    /// Воспроизведение остановилось: закрыть запись и снять «сейчас играет».
    Stopped {
        at: Stamp,
    },
    /// Ошибка чтения трека: закрыть прослушивание, если звучал он.
    Failed {
        track: TrackId,
        at: Stamp,
    },
    /// Привязать устройство к аккаунту Scrobb.
    Link {
        device_name: String,
    },
    CancelLink,
    Unlink,
}

pub(crate) struct HistoryService {
    store: Arc<dyn HistoryStore>,
    clock: Arc<dyn Clock>,
    observer: Arc<dyn Observer>,
    thresholds: Thresholds,
    listening: Option<Listening>,
    remote: Option<Remote>,
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

impl HistoryService {
    pub fn new(
        store: Arc<dyn HistoryStore>,
        clock: Arc<dyn Clock>,
        observer: Arc<dyn Observer>,
        thresholds: Thresholds,
        remote: Option<Remote>,
    ) -> Self {
        Self {
            store,
            clock,
            observer,
            thresholds,
            listening: None,
            remote,
            ids: ContextV7::new(),
        }
    }

    pub fn spawn(self, messages: Receiver<Message>) -> JoinHandle<()> {
        thread::Builder::new()
            .name("player-core-history".into())
            .spawn(move || self.run(messages))
            .expect("поток истории запускается")
    }

    fn run(mut self, messages: Receiver<Message>) {
        let recovered = self.store.recover_interrupted();
        self.check(recovered);
        if let Some(remote) = self.remote.as_mut() {
            remote.deliver();
        }
        loop {
            let received = match self.next_deadline() {
                Some(at) => {
                    let wait = at.saturating_sub(self.clock.monotonic_ms());
                    messages.recv_timeout(self.clock.real_wait(wait))
                }
                None => messages.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(message) => self.on_message(message),
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

    fn on_message(&mut self, message: Message) {
        match message {
            Message::Started { track, at } => {
                self.end(Outcome::Finished, at);
                let snapshot = TrackSnapshot::from(&*track);
                self.begin(&track, snapshot.clone(), at);
                if let Some(remote) = self.remote.as_mut() {
                    remote.on_started(snapshot, at.mono_ms);
                }
            }
            Message::Paused { at } => {
                if let Some(mut l) = self.listening.take() {
                    if !l.paused {
                        l.tracker.pause(at.mono_ms);
                        l.paused = true;
                        self.poll(&mut l, at.mono_ms, true);
                    }
                    self.listening = Some(l);
                }
                if let Some(remote) = self.remote.as_mut() {
                    remote.on_paused(at.mono_ms);
                }
            }
            Message::Resumed { at, position_ms } => {
                if let Some(l) = self.listening.as_mut().filter(|l| l.paused) {
                    l.tracker.resume(at.mono_ms);
                    l.paused = false;
                }
                if let Some(remote) = self.remote.as_mut() {
                    remote.on_resumed(position_ms, at.mono_ms);
                }
            }
            Message::Seeked { position_ms, at } => {
                if let Some(remote) = self.remote.as_mut() {
                    remote.on_seeked(position_ms, at.mono_ms);
                }
            }
            Message::Ended { outcome, at } => self.end(outcome, at),
            Message::Stopped { at } => {
                self.end(Outcome::Stopped, at);
                if let Some(remote) = self.remote.as_mut() {
                    remote.on_stopped();
                }
            }
            Message::Failed { track, at } => {
                if self.listening.as_ref().is_some_and(|l| l.track == track) {
                    self.end(Outcome::Error, at);
                }
            }
            Message::Link { device_name } => {
                if let Some(remote) = self.remote.as_mut() {
                    remote.link(&device_name);
                }
            }
            Message::CancelLink => {
                if let Some(remote) = self.remote.as_mut() {
                    remote.cancel_link();
                }
            }
            Message::Unlink => {
                if let Some(remote) = self.remote.as_mut() {
                    remote.unlink();
                }
            }
        }
    }

    fn begin(&mut self, track: &Track, snapshot: TrackSnapshot, at: Stamp) {
        let play = NewPlay {
            id: new_play_id(&self.ids, at.unix_ms),
            started_at_ms: at.unix_ms,
            track: snapshot,
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
        let linked = self.remote.as_ref().is_some_and(Remote::is_linked);
        let targets: &[&str] = if linked { &[TARGET_SCROBB] } else { &[] };
        let marked = self
            .store
            .mark_scrobbled(id, played_ms, targets, self.clock.unix_ms());
        if self.check(marked).is_some()
            && linked
            && let Some(remote) = self.remote.as_mut()
        {
            remote.deliver();
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
        if let Some(remote) = self.remote.as_mut() {
            remote.tick();
        }
    }

    /// Ближайший момент, когда нужно что-то сделать без новых сообщений.
    fn next_deadline(&self) -> Option<u64> {
        let now = self.clock.monotonic_ms();
        let listening = self.listening.as_ref().filter(|l| !l.paused).map(|l| {
            let played = l.tracker.played_ms(now);
            let checkpoint = (l.checkpointed_ms + CHECKPOINT_MS).saturating_sub(played);
            let threshold = l.tracker.remaining_ms(now).unwrap_or(u64::MAX);
            now.saturating_add(checkpoint.min(threshold))
        });
        let remote = self.remote.as_ref().and_then(Remote::deadline);
        listening.into_iter().chain(remote).min()
    }

    /// Сообщить об ошибке хранилища; `None` — была ошибка.
    fn check<T>(&self, result: Result<T, HistoryError>) -> Option<T> {
        result
            .map_err(|e| self.observer.on_event(Event::Error(e.to_string())))
            .ok()
    }
}

/// UUIDv7: время начала, счётчик и случайные биты; уникален без согласования
/// между устройствами, а на одном устройстве растёт в порядке создания.
fn new_play_id(ids: &ContextV7, unix_ms: i64) -> PlayId {
    let ms = unix_ms.max(0) as u64;
    let ts = Timestamp::from_unix(ids, ms / 1000, (ms % 1000) as u32 * 1_000_000);
    PlayId(Uuid::new_v7(ts).to_string())
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
    }
}
