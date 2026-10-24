//! Задержки повторной отправки (`docs/architecture.md` §18.8): экспоненциально
//! от 30 с до 1 ч с джиттером. Случайное число и время передаёт вызывающий
//! код, чтобы логика оставалась детерминированной и работала в wasm.

use crate::ApiError;

/// Минимальная задержка.
pub const BASE_DELAY_MS: u64 = 30_000;
/// Максимальная задержка.
pub const MAX_DELAY_MS: u64 = 3_600_000;

/// Задержка перед попыткой номер `attempt` (с нуля); `jitter` — случайное число из `[0, 1)`.
///
/// Задержка лежит в `[d, 2d)`, где `d = 30 с · 2^attempt`, и не превышает 1 ч.
pub fn delay_ms(attempt: u32, jitter: f64) -> u64 {
    let d = BASE_DELAY_MS.saturating_mul(1u64 << attempt.min(16));
    let spread = (d as f64 * jitter.clamp(0.0, 1.0)) as u64;
    d.saturating_add(spread).min(MAX_DELAY_MS)
}

/// Можно ли отправлять в сервис.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SendStatus {
    /// Можно.
    #[default]
    Ready,
    /// После ошибки: ждать до момента `until_ms` монотонных часов.
    Waiting {
        /// Когда можно повторить.
        until_ms: u64,
    },
    /// `401`: токен отозван — ждать новой привязки, очередь сохраняется.
    Unauthorized,
    /// `426`: клиент устарел — ждать обновления.
    UpgradeRequired,
}

/// Повторы отправки в один сервис (§18.8).
#[derive(Debug, Clone, Default)]
pub struct Backoff {
    attempt: u32,
    status: SendStatus,
}

impl Backoff {
    /// Текущее состояние.
    pub fn status(&self) -> SendStatus {
        self.status
    }

    /// Можно ли отправлять в момент `now_ms`.
    pub fn ready(&self, now_ms: u64) -> bool {
        match self.status {
            SendStatus::Ready => true,
            SendStatus::Waiting { until_ms } => now_ms >= until_ms,
            SendStatus::Unauthorized | SendStatus::UpgradeRequired => false,
        }
    }

    /// Запрос прошёл, или пользователь заново привязал устройство или обновился.
    pub fn on_success(&mut self) {
        *self = Self::default();
    }

    /// Сервис ответил ошибкой; `jitter` — случайное число из `[0, 1)`.
    pub fn on_error(&mut self, error: &ApiError, now_ms: u64, jitter: f64) -> SendStatus {
        self.status = match error {
            ApiError::Unauthorized => SendStatus::Unauthorized,
            ApiError::UpgradeRequired => SendStatus::UpgradeRequired,
            // Сервер сам сказал, сколько ждать: счётчик попыток не растёт.
            ApiError::RateLimited {
                retry_after_s: Some(s),
            } => SendStatus::Waiting {
                until_ms: now_ms.saturating_add(s.saturating_mul(1000)),
            },
            ApiError::RateLimited {
                retry_after_s: None,
            }
            | ApiError::Server(_)
            | ApiError::Unexpected(_)
            | ApiError::Decode(_)
            | ApiError::Invalid(_) => return self.on_network_error(now_ms, jitter),
        };
        self.status
    }

    /// Запрос не дошёл: нет сети, таймаут.
    pub fn on_network_error(&mut self, now_ms: u64, jitter: f64) -> SendStatus {
        self.status = SendStatus::Waiting {
            until_ms: now_ms.saturating_add(delay_ms(self.attempt, jitter)),
        };
        self.attempt = self.attempt.saturating_add(1);
        self.status
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_follows_server_answers() {
        let mut b = Backoff::default();
        assert!(b.ready(0));

        // Ошибки подряд — задержка растёт; успех сбрасывает счётчик.
        assert_eq!(
            b.on_network_error(1_000, 0.0),
            SendStatus::Waiting { until_ms: 31_000 }
        );
        assert!(!b.ready(30_999) && b.ready(31_000));
        assert_eq!(
            b.on_error(&ApiError::Server(503), 31_000, 0.0),
            SendStatus::Waiting { until_ms: 91_000 }
        );
        b.on_success();
        assert_eq!(b.status(), SendStatus::Ready);
        assert_eq!(
            b.on_error(&ApiError::Decode("мусор".into()), 0, 0.0),
            SendStatus::Waiting {
                until_ms: BASE_DELAY_MS
            }
        );

        // 429 с Retry-After — ровно столько, без роста счётчика.
        let limited = ApiError::RateLimited {
            retry_after_s: Some(120),
        };
        assert_eq!(
            b.on_error(&limited, 10_000, 0.9),
            SendStatus::Waiting { until_ms: 130_000 }
        );
        assert_eq!(
            b.on_network_error(0, 0.0),
            SendStatus::Waiting { until_ms: 60_000 },
            "вторая попытка после первой ошибки"
        );

        // 401 и 426 останавливают отправку до действия пользователя.
        assert_eq!(
            b.on_error(&ApiError::Unauthorized, 0, 0.0),
            SendStatus::Unauthorized
        );
        assert!(!b.ready(u64::MAX));
        assert_eq!(
            b.on_error(&ApiError::UpgradeRequired, 0, 0.0),
            SendStatus::UpgradeRequired
        );
        assert!(!b.ready(u64::MAX));
        b.on_success();
        assert!(b.ready(0));
    }

    #[test]
    fn stays_between_30_seconds_and_an_hour() {
        assert_eq!(delay_ms(0, 0.0), BASE_DELAY_MS);
        assert_eq!(delay_ms(1, 0.5), 90_000);
        assert_eq!(delay_ms(30, 0.0), MAX_DELAY_MS);
        assert!((0..40).all(|a| (BASE_DELAY_MS..=MAX_DELAY_MS).contains(&delay_ms(a, 0.99))));
    }
}
