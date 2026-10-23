//! Задержки повторной отправки (`docs/architecture.md` §18.8): экспоненциально
//! от 30 с до 1 ч с джиттером. Случайное число передаёт вызывающий код,
//! чтобы логика оставалась детерминированной и работала в wasm.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_between_30_seconds_and_an_hour() {
        assert_eq!(delay_ms(0, 0.0), BASE_DELAY_MS);
        assert_eq!(delay_ms(1, 0.5), 90_000);
        assert_eq!(delay_ms(30, 0.0), MAX_DELAY_MS);
        assert!((0..40).all(|a| (BASE_DELAY_MS..=MAX_DELAY_MS).contains(&delay_ms(a, 0.99))));
    }
}
