//! Правило скроббла и политика «сейчас играет» (`docs/architecture.md` §18.7).
//!
//! Чистая логика без ввода-вывода и системного времени: время передаёт
//! вызывающий код в миллисекундах монотонных часов. Поэтому крейт одинаково
//! работает в плеере, в десктоп-клиенте и в браузере (wasm), а поведение
//! сверяется с языконезависимыми векторами `contracts/scrobble-rules`.

pub use scrobb_api::Thresholds;

/// Сколько трек должен прозвучать, чтобы засчитаться.
///
/// `None` — трек не скробблится никогда: он не длиннее `min_track_ms`.
pub fn required_play_ms(duration_ms: Option<u64>, t: &Thresholds) -> Option<u64> {
    match duration_ms {
        Some(d) if d <= t.min_track_ms => None,
        Some(d) => Some((d * u64::from(t.percent) / 100).min(t.max_wait_ms)),
        None => Some(t.max_wait_ms),
    }
}

/// Сработавший скроббл.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scrobble {
    /// Начало прослушивания, секунды Unix UTC.
    pub listened_at: i64,
    /// Сколько трек прозвучал к моменту срабатывания.
    pub played_ms: u64,
}

/// Счётчик одного прослушивания одного трека.
///
/// Считает время звучания, а не время на часах: пауза и остановка
/// останавливают счётчик, перемотка вперёд ничего не добавляет. Повтор трека
/// (сменился трек или позиция вернулась к началу) — новый `ListenTracker`.
#[derive(Debug, Clone)]
pub struct ListenTracker {
    listened_at: i64,
    required_ms: Option<u64>,
    played_ms: u64,
    playing_since_ms: Option<u64>,
    stopped: bool,
    scrobbled: bool,
}

impl ListenTracker {
    /// Трек начал играть в момент `now_ms`; `listened_at` — то же время в секундах Unix UTC.
    pub fn start(duration_ms: Option<u64>, listened_at: i64, now_ms: u64, t: &Thresholds) -> Self {
        Self {
            listened_at,
            required_ms: required_play_ms(duration_ms, t),
            played_ms: 0,
            playing_since_ms: Some(now_ms),
            stopped: false,
            scrobbled: false,
        }
    }

    /// Пауза.
    pub fn pause(&mut self, now_ms: u64) {
        self.played_ms = self.played_ms(now_ms);
        self.playing_since_ms = None;
    }

    /// Продолжение после паузы. После [`stop`](Self::stop) ничего не делает.
    pub fn resume(&mut self, now_ms: u64) {
        if !self.stopped && self.playing_since_ms.is_none() {
            self.playing_since_ms = Some(now_ms);
        }
    }

    /// Трек закончился или его сменили.
    pub fn stop(&mut self, now_ms: u64) {
        self.pause(now_ms);
        self.stopped = true;
    }

    /// Сколько трек прозвучал к моменту `now_ms`.
    pub fn played_ms(&self, now_ms: u64) -> u64 {
        let running = self
            .playing_since_ms
            .map_or(0, |since| now_ms.saturating_sub(since));
        self.played_ms + running
    }

    /// Проверка порога: возвращает скроббл ровно один раз, когда порог достигнут.
    pub fn poll(&mut self, now_ms: u64) -> Option<Scrobble> {
        let required = self.required_ms?;
        let played_ms = self.played_ms(now_ms);
        if self.scrobbled || played_ms < required {
            return None;
        }
        self.scrobbled = true;
        Some(Scrobble {
            listened_at: self.listened_at,
            played_ms,
        })
    }

    /// Через сколько миллисекунд звучания сработает порог; `None` — уже сработал или не сработает.
    /// Нужно, чтобы ставить таймер, а не опрашивать каждую секунду.
    pub fn remaining_ms(&self, now_ms: u64) -> Option<u64> {
        if self.scrobbled || self.stopped {
            return None;
        }
        Some(self.required_ms?.saturating_sub(self.played_ms(now_ms)))
    }
}

/// Что сделать с «сейчас играет».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NowPlayingAction {
    /// Отправить `POST /now-playing`.
    Send,
    /// Отправить `DELETE /now-playing`.
    Clear,
}

/// Политика «сейчас играет» (§18.7): отправка при старте и после продолжения,
/// при изменениях — не чаще раза в минуту, сброс после паузы дольше 5 минут.
#[derive(Debug, Clone, Default)]
pub struct NowPlayingGate {
    last_sent_ms: Option<u64>,
    paused_since_ms: Option<u64>,
    cleared: bool,
}

impl NowPlayingGate {
    /// Минимальный интервал между повторными отправками.
    pub const RESEND_INTERVAL_MS: u64 = 60_000;
    /// Через сколько паузы сбрасывать «сейчас играет».
    pub const CLEAR_AFTER_PAUSE_MS: u64 = 300_000;

    /// Начался новый трек или воспроизведение продолжилось.
    pub fn on_play(&mut self, now_ms: u64) -> Option<NowPlayingAction> {
        self.paused_since_ms = None;
        self.cleared = false;
        self.last_sent_ms = Some(now_ms);
        Some(NowPlayingAction::Send)
    }

    /// Изменились данные текущего трека (например, позиция после перемотки).
    pub fn on_change(&mut self, now_ms: u64) -> Option<NowPlayingAction> {
        if self.paused_since_ms.is_some() || self.cleared {
            return None;
        }
        match self.last_sent_ms {
            Some(last) if now_ms.saturating_sub(last) < Self::RESEND_INTERVAL_MS => None,
            _ => {
                self.last_sent_ms = Some(now_ms);
                Some(NowPlayingAction::Send)
            }
        }
    }

    /// Пауза.
    pub fn on_pause(&mut self, now_ms: u64) {
        self.paused_since_ms.get_or_insert(now_ms);
    }

    /// Плеер закрыт или очередь закончилась.
    pub fn on_stop(&mut self) -> Option<NowPlayingAction> {
        self.clear()
    }

    /// Когда [`poll`](Self::poll) может что-то вернуть: момент сброса после
    /// долгой паузы. `None` — проверять незачем. Нужно, чтобы ставить таймер.
    pub fn next_poll_ms(&self) -> Option<u64> {
        if self.cleared || self.last_sent_ms.is_none() {
            return None;
        }
        self.paused_since_ms
            .map(|since| since + Self::CLEAR_AFTER_PAUSE_MS)
    }

    /// Периодическая проверка: сброс после долгой паузы.
    pub fn poll(&mut self, now_ms: u64) -> Option<NowPlayingAction> {
        match self.paused_since_ms {
            Some(since) if now_ms.saturating_sub(since) >= Self::CLEAR_AFTER_PAUSE_MS => {
                self.clear()
            }
            _ => None,
        }
    }

    fn clear(&mut self) -> Option<NowPlayingAction> {
        if self.cleared || self.last_sent_ms.is_none() {
            return None;
        }
        self.cleared = true;
        Some(NowPlayingAction::Clear)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_counts_only_played_time() {
        let t = Thresholds::default();
        let mut tr = ListenTracker::start(Some(180_000), 0, 1_000, &t);
        tr.pause(31_000);
        assert_eq!(tr.remaining_ms(500_000), Some(60_000));
    }

    #[test]
    fn now_playing_resend_is_throttled() {
        let mut g = NowPlayingGate::default();
        assert_eq!(g.on_play(0), Some(NowPlayingAction::Send));
        assert_eq!(g.on_change(30_000), None);
        assert_eq!(g.on_change(60_000), Some(NowPlayingAction::Send));
    }

    #[test]
    fn now_playing_clears_once_after_long_pause() {
        let mut g = NowPlayingGate::default();
        g.on_play(0);
        assert_eq!(g.next_poll_ms(), None);
        g.on_pause(10_000);
        assert_eq!(g.next_poll_ms(), Some(310_000));
        assert_eq!(g.poll(309_999), None);
        assert_eq!(g.poll(310_000), Some(NowPlayingAction::Clear));
        assert_eq!(g.next_poll_ms(), None);
        assert_eq!(g.poll(400_000), None);
        assert_eq!(g.on_play(500_000), Some(NowPlayingAction::Send));
    }
}
