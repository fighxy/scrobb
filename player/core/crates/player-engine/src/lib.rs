//! Звуковой движок (заготовка): декодирование, ресемплинг, ReplayGain,
//! эквалайзер, кроссфейд, gapless и кольцевой буфер для аудиопотока.
//!
//! Устройство и ограничения — `docs/player-architecture.md` §3.2 и §3.4:
//! - декодер — Symphonia, недостающие кодеки — через порт `PlatformDecoder`;
//! - вывод — через порт `AudioOutput`, движок отдаёт ему `PcmSource`;
//! - в аудиопотоке — ни блокировок, ни аллокаций, ни ввода-вывода.
//!
//! Только для нативных целей: под wasm не собирается по смыслу.

/// Режим ReplayGain.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReplayGainMode {
    /// Выключен.
    Off,
    /// По трекам.
    #[default]
    Track,
    /// По альбомам.
    Album,
}

/// Настройки движка.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineConfig {
    /// ReplayGain.
    pub replay_gain: ReplayGainMode,
    /// Предусиление, дБ.
    pub preamp_db: f32,
    /// Кроссфейд; 0 — gapless без наложения.
    pub crossfade_ms: u32,
    /// Сколько декодировать вперёд.
    pub prebuffer_ms: u32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            replay_gain: ReplayGainMode::Track,
            preamp_db: 0.0,
            crossfade_ms: 0,
            prebuffer_ms: 3_000,
        }
    }
}
